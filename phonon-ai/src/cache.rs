//! Filesystem cache for TTS synthesis results.
//!
//! [`CachingTts`] is a decorator over any [`TtsProvider`]. Synthesis requests
//! are first looked up by a content-addressable key derived from the
//! `(provider_id, voice_config, text)` tuple. A hit returns the cached WAV
//! immediately; a miss delegates to the inner provider and persists the
//! result before returning.
//!
//! The cache is intentionally simple — flat directory of `<blake3>.wav`
//! files. WAV is used so cached audio is lossless and can be loaded by
//! any reader.

use crate::AiError;
use crate::tts::TtsProvider;
use crate::voice::VoiceConfig;
use phonon_core::AudioBuffer;
use std::future::Future;
use std::path::{Path, PathBuf};
use tracing::{debug, info, instrument, warn};

/// Caching wrapper around an inner [`TtsProvider`].
///
/// `provider_id` namespaces the cache so that different providers
/// (e.g. OpenAI vs ElevenLabs) producing the "same" voice don't
/// collide. Use a stable string per provider, e.g. `"openai-tts-1"`.
pub struct CachingTts<T: TtsProvider> {
  inner: T,
  cache_dir: PathBuf,
  provider_id: String,
}

impl<T: TtsProvider> CachingTts<T> {
  /// Build a `CachingTts` using the platform default cache directory.
  ///
  /// Default location:
  /// - Linux: `~/.cache/phonon/tts`
  /// - macOS: `~/Library/Caches/phonon/tts`
  /// - Windows: `%LOCALAPPDATA%\phonon\cache\tts`
  pub fn new(inner: T, provider_id: impl Into<String>) -> Result<Self, AiError> {
    let base = dirs::cache_dir()
      .ok_or_else(|| AiError::Config("No platform cache directory available".to_string()))?;
    let cache_dir = base.join("phonon").join("tts");
    Self::with_dir(inner, provider_id, cache_dir)
  }

  /// Build a `CachingTts` with an explicit cache directory.
  pub fn with_dir(
    inner: T,
    provider_id: impl Into<String>,
    cache_dir: PathBuf,
  ) -> Result<Self, AiError> {
    std::fs::create_dir_all(&cache_dir)
      .map_err(|e| AiError::Io(format!("create cache dir: {}", e)))?;
    Ok(Self {
      inner,
      cache_dir,
      provider_id: provider_id.into(),
    })
  }

  /// Compute the content-addressable key for a request.
  fn key_for(&self, text: &str, voice: &VoiceConfig) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(self.provider_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(voice.voice_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(voice.model.as_bytes());
    hasher.update(b"\0");
    hasher.update(&voice.speed.to_le_bytes());
    hasher.update(b"\0");
    if let Some(e) = &voice.emotion {
      hasher.update(e.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(s) = &voice.style {
      hasher.update(s.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(l) = &voice.language {
      hasher.update(l.as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(text.as_bytes());
    hasher.finalize().to_hex().to_string()
  }

  fn path_for(&self, key: &str) -> PathBuf {
    self.cache_dir.join(format!("{}.wav", key))
  }

  /// Path to the cache file for a given (text, voice). Useful for tests
  /// and for callers that want to inspect or pre-populate the cache.
  pub fn cache_path(&self, text: &str, voice: &VoiceConfig) -> PathBuf {
    self.path_for(&self.key_for(text, voice))
  }

  /// Cache directory path.
  pub fn cache_dir(&self) -> &Path {
    &self.cache_dir
  }

  /// Returns a reference to the inner provider (for tests / introspection).
  pub fn inner(&self) -> &T {
    &self.inner
  }
}

impl<T: TtsProvider> TtsProvider for CachingTts<T> {
  #[instrument(skip(self, text, voice), fields(text_len = text.len(), voice = %voice.voice_id))]
  fn synthesize(
    &self,
    text: &str,
    voice: &VoiceConfig,
  ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send {
    async move {
      let key = self.key_for(text, voice);
      let path = self.path_for(&key);

      if path.exists() {
        match phonon_wav::read_wav(&path) {
          Ok(buf) => {
            info!(key = %key, "TTS cache hit");
            return Ok(buf);
          }
          Err(e) => {
            warn!(key = %key, error = %e, "TTS cache file unreadable, re-synthesizing");
          }
        }
      } else {
        debug!(key = %key, "TTS cache miss");
      }

      let buffer = self.inner.synthesize(text, voice).await?;

      // Best-effort write — a cache failure should not fail synthesis.
      if let Err(e) = phonon_wav::write_wav(&path, &buffer) {
        warn!(key = %key, error = %e, "TTS cache write failed");
      } else {
        debug!(key = %key, "TTS cache stored");
      }
      Ok(buffer)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use tempfile::TempDir;

  /// In-memory TTS provider that records how many times it was called.
  /// Returns a buffer whose first sample encodes the text length so we can
  /// verify the same data flows back from cache.
  struct CountingTts {
    calls: Arc<AtomicUsize>,
  }

  impl TtsProvider for CountingTts {
    fn synthesize(
      &self,
      text: &str,
      _voice: &VoiceConfig,
    ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send {
      let calls = self.calls.clone();
      let len = text.len() as f32 / 100.0;
      async move {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(AudioBuffer {
          samples: vec![len, len, len, len],
          sample_rate: 16000,
          channels: 1,
        })
      }
    }
  }

  fn temp_caching() -> (CachingTts<CountingTts>, Arc<AtomicUsize>, TempDir) {
    let calls = Arc::new(AtomicUsize::new(0));
    let inner = CountingTts {
      calls: calls.clone(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let cache =
      CachingTts::with_dir(inner, "test-provider", tmp.path().to_path_buf()).expect("init");
    (cache, calls, tmp)
  }

  #[tokio::test]
  async fn test_first_call_hits_inner_provider() {
    let (cache, calls, _tmp) = temp_caching();
    let voice = VoiceConfig::new("alloy");
    let _ = cache.synthesize("Hello world", &voice).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn test_second_call_uses_cache() {
    let (cache, calls, _tmp) = temp_caching();
    let voice = VoiceConfig::new("alloy");
    let buf1 = cache.synthesize("Hello world", &voice).await.unwrap();
    let buf2 = cache.synthesize("Hello world", &voice).await.unwrap();
    assert_eq!(
      calls.load(Ordering::SeqCst),
      1,
      "second call must hit cache"
    );
    assert_eq!(buf1.sample_rate, buf2.sample_rate);
    assert_eq!(buf1.samples.len(), buf2.samples.len());
    // WAV roundtrip is 16-bit PCM, so a tiny quantization delta is expected.
    for (a, b) in buf1.samples.iter().zip(buf2.samples.iter()) {
      assert!(
        (a - b).abs() < 1e-3,
        "cache roundtrip drift: {} vs {}",
        a,
        b
      );
    }
  }

  #[tokio::test]
  async fn test_different_text_misses_cache() {
    let (cache, calls, _tmp) = temp_caching();
    let voice = VoiceConfig::new("alloy");
    let _ = cache.synthesize("Foo", &voice).await.unwrap();
    let _ = cache.synthesize("Bar", &voice).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
  }

  #[tokio::test]
  async fn test_different_voice_id_misses_cache() {
    let (cache, calls, _tmp) = temp_caching();
    let v1 = VoiceConfig::new("alloy");
    let v2 = VoiceConfig::new("nova");
    let _ = cache.synthesize("Same text", &v1).await.unwrap();
    let _ = cache.synthesize("Same text", &v2).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
  }

  #[tokio::test]
  async fn test_different_speed_misses_cache() {
    let (cache, calls, _tmp) = temp_caching();
    let v1 = VoiceConfig::new("alloy").with_speed(1.0);
    let v2 = VoiceConfig::new("alloy").with_speed(1.5);
    let _ = cache.synthesize("Same text", &v1).await.unwrap();
    let _ = cache.synthesize("Same text", &v2).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
  }

  #[tokio::test]
  async fn test_cache_file_created() {
    let (cache, _calls, _tmp) = temp_caching();
    let voice = VoiceConfig::new("alloy");
    let path = cache.cache_path("Hello", &voice);
    assert!(!path.exists());
    let _ = cache.synthesize("Hello", &voice).await.unwrap();
    assert!(path.exists(), "cache file should be created at {:?}", path);
  }

  #[test]
  fn test_key_stable_across_instances() {
    let tmp = TempDir::new().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache_a = CachingTts::with_dir(
      CountingTts {
        calls: calls.clone(),
      },
      "test",
      tmp.path().to_path_buf(),
    )
    .unwrap();
    let cache_b = CachingTts::with_dir(
      CountingTts {
        calls: calls.clone(),
      },
      "test",
      tmp.path().to_path_buf(),
    )
    .unwrap();
    let voice = VoiceConfig::new("alloy");
    assert_eq!(
      cache_a.key_for("Hello", &voice),
      cache_b.key_for("Hello", &voice)
    );
  }

  #[test]
  fn test_provider_id_isolates_cache() {
    let tmp = TempDir::new().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache_a = CachingTts::with_dir(
      CountingTts {
        calls: calls.clone(),
      },
      "openai",
      tmp.path().to_path_buf(),
    )
    .unwrap();
    let cache_b = CachingTts::with_dir(
      CountingTts {
        calls: calls.clone(),
      },
      "elevenlabs",
      tmp.path().to_path_buf(),
    )
    .unwrap();
    let voice = VoiceConfig::new("alloy");
    assert_ne!(
      cache_a.key_for("Hello", &voice),
      cache_b.key_for("Hello", &voice)
    );
  }
}
