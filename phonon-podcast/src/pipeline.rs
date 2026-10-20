use crate::PodcastError;
use crate::config::{AssetSource, PipelineConfig};
use crate::export::{Episode, build_episode_metadata};
use crate::script::PodcastScript;
use phonon_ai::{AiError, SoundGenProvider, SoundPrompt, TtsProvider};
use phonon_core::{self, AudioBuffer};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{info, instrument};

/// Sentinel "no sound generator" type — the default `S` parameter for
/// [`PodcastPipeline`]. Calling [`SoundGenProvider::generate`] on this
/// returns an error so configs that ask for `AssetSource::Generated`
/// fail loudly rather than silently dropping the asset.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSoundGen;

impl SoundGenProvider for NoSoundGen {
  async fn generate(&self, _prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
    Err(AiError::Config(
      "AssetSource::Generated requires a SoundGenProvider — call \
       PodcastPipeline::with_sound_gen() first"
        .to_string(),
    ))
  }
}

/// The podcast generation pipeline.
///
/// Orchestrates TTS synthesis, audio processing, and assembly
/// to produce a complete podcast episode from a script.
///
/// The optional second type parameter `S` is a [`SoundGenProvider`].
/// By default it's [`NoSoundGen`] — set it via
/// [`PodcastPipeline::with_sound_gen`] when the script uses
/// `AssetSource::Generated` for BGM, intro, or outro.
pub struct PodcastPipeline<T: TtsProvider, S: SoundGenProvider = NoSoundGen> {
  tts: T,
  sound_gen: S,
}

impl<T: TtsProvider> PodcastPipeline<T, NoSoundGen> {
  /// Creates a new pipeline with the given TTS provider and no
  /// sound generator. Use [`Self::with_sound_gen`] to upgrade.
  pub fn new(tts: T) -> Self {
    Self {
      tts,
      sound_gen: NoSoundGen,
    }
  }
}

impl<T: TtsProvider, S: SoundGenProvider> PodcastPipeline<T, S> {
  /// Returns a new pipeline with the supplied sound generator,
  /// preserving the existing TTS provider.
  pub fn with_sound_gen<S2: SoundGenProvider>(self, sound_gen: S2) -> PodcastPipeline<T, S2> {
    PodcastPipeline {
      tts: self.tts,
      sound_gen,
    }
  }

  /// Generates a complete podcast episode as an `AudioBuffer`.
  ///
  /// # Pipeline Steps
  ///
  /// 1. Synthesize each segment via TTS
  /// 2. Post-process each segment (normalize)
  /// 3. Assemble segments with pauses/crossfades
  /// 4. Add intro/outro if configured
  /// 5. Mix with BGM if configured
  /// 6. Final normalize + fade in/out
  #[instrument(skip(self, script), fields(title = %script.title, segments = script.segment_count()))]
  pub async fn generate(&self, script: &PodcastScript) -> Result<AudioBuffer, PodcastError> {
    let config = &script.config;
    let target_rate = config.output_sample_rate;
    let target_channels = config.output_channels;

    info!("Starting podcast generation");

    // Step 1: Synthesize all segments concurrently
    let concurrency = config.tts_concurrency.max(1);

    // Collect owned (index, text, voice) tuples up front so we can synthesize concurrently.
    // If a segment has a language override, clone the speaker's voice config and apply it.
    let synthesis_items: Vec<_> = script
      .segments
      .iter()
      .enumerate()
      .map(|(i, segment)| {
        let voice = script
          .speakers
          .get(&segment.speaker)
          .ok_or_else(|| PodcastError::Script(format!("Unknown speaker: {}", segment.speaker)))
          .cloned()
          .map(|mut v| {
            // Apply segment-level language override if specified
            if let Some(ref lang) = segment.language {
              v.language = Some(lang.clone());
            }
            v
          });
        (i, segment.text.clone(), segment.speaker.clone(), voice)
      })
      .collect();

    // Validate all speakers before starting any synthesis.
    for (_, _, _, voice_result) in &synthesis_items {
      if let Err(e) = voice_result {
        return Err(PodcastError::Script(e.to_string()));
      }
    }

    info!(
      segments = synthesis_items.len(),
      concurrency, "Starting concurrent TTS synthesis"
    );

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let tts = &self.tts;

    let futures: Vec<_> = synthesis_items
      .into_iter()
      .map(|(i, text, speaker, voice_result)| {
        let sem = semaphore.clone();
        async move {
          // Safety: we validated all voices above, so this won't fail.
          let voice = voice_result.map_err(|e| PodcastError::Script(e.to_string()))?;

          let _permit = sem
            .acquire()
            .await
            .map_err(|e| PodcastError::Processing(format!("Semaphore error: {e}")))?;

          info!(segment = i, speaker = %speaker, "Synthesizing segment");

          let buffer = tts
            .synthesize(&text, &voice)
            .await
            .map_err(|e| PodcastError::Tts(e.to_string()))?;

          Ok::<AudioBuffer, PodcastError>(buffer)
        }
      })
      .collect();

    let results = futures::future::join_all(futures).await;
    let mut segment_buffers = Vec::with_capacity(results.len());
    for result in results {
      segment_buffers.push(result?);
    }

    info!(count = segment_buffers.len(), "All segments synthesized");

    // Step 2: Post-process each segment
    let segment_buffers =
      self.post_process_segments(segment_buffers, target_rate, target_channels)?;

    // Step 3: Assemble segments with pauses/crossfades
    let mut assembled = self.assemble_segments(segment_buffers, config)?;

    // Step 4: Add intro/outro
    assembled = self
      .add_intro_outro(assembled, config, target_rate, target_channels)
      .await?;

    // Step 5: Mix with BGM
    assembled = self
      .mix_bgm(assembled, config, target_rate, target_channels)
      .await?;

    // Step 6: Final normalize + fade
    phonon_core::normalize(&mut assembled, config.normalize_target);

    if config.fade_in_secs > 0.0 {
      phonon_core::fade_in(&mut assembled, config.fade_in_secs);
    }
    if config.fade_out_secs > 0.0 {
      phonon_core::fade_out(&mut assembled, config.fade_out_secs);
    }

    info!(
      duration = assembled.duration_secs(),
      samples = assembled.samples.len(),
      "Podcast generation complete"
    );

    Ok(assembled)
  }

  /// Generate a complete [`Episode`] — audio plus chapters and SRT
  /// reconstructed from per-segment durations. The returned episode has
  /// empty `tags`; populate them via [`Episode::with_tags`] before export.
  #[instrument(skip(self, script), fields(title = %script.title))]
  pub async fn generate_episode(&self, script: &PodcastScript) -> Result<Episode, PodcastError> {
    let config = &script.config;
    let target_rate = config.output_sample_rate;
    let target_channels = config.output_channels;

    // Synthesize each segment one-by-one. (Concurrent variant is harder
    // to reconcile with per-segment timing in the same pass; for now
    // generate_episode trades concurrency for accurate chapter timing.)
    let mut raw_buffers = Vec::with_capacity(script.segments.len());
    for segment in &script.segments {
      let mut voice = script
        .speakers
        .get(&segment.speaker)
        .ok_or_else(|| PodcastError::Script(format!("Unknown speaker: {}", segment.speaker)))?
        .clone();
      if let Some(ref lang) = segment.language {
        voice.language = Some(lang.clone());
      }
      let buf = self
        .tts
        .synthesize(&segment.text, &voice)
        .await
        .map_err(|e| PodcastError::Tts(e.to_string()))?;
      raw_buffers.push(buf);
    }

    // Resample / channel-convert / per-segment normalize.
    let processed = self.post_process_segments(raw_buffers, target_rate, target_channels)?;

    // Capture durations *after* post-processing — these are what the
    // listener will hear, so chapter / SRT timing must use them.
    let segment_durations: Vec<f64> = processed.iter().map(|b| b.duration_secs()).collect();

    let mut assembled = self.assemble_segments(processed, config)?;
    assembled = self
      .add_intro_outro(assembled, config, target_rate, target_channels)
      .await?;
    assembled = self
      .mix_bgm(assembled, config, target_rate, target_channels)
      .await?;

    phonon_core::normalize(&mut assembled, config.normalize_target);
    if config.fade_in_secs > 0.0 {
      phonon_core::fade_in(&mut assembled, config.fade_in_secs);
    }
    if config.fade_out_secs > 0.0 {
      phonon_core::fade_out(&mut assembled, config.fade_out_secs);
    }

    let (chapters, srt) = build_episode_metadata(
      script,
      &segment_durations,
      config.pause_between_segments_secs,
    );
    let episode = Episode::new(assembled)
      .with_chapters(chapters)
      .with_srt(srt);

    info!("Episode generation complete");
    Ok(episode)
  }

  /// Generates a podcast and writes it to a WAV file.
  #[instrument(skip(self, script, output), fields(output = %output.as_ref().display()))]
  pub async fn generate_to_file(
    &self,
    script: &PodcastScript,
    output: impl AsRef<Path>,
  ) -> Result<(), PodcastError> {
    let buffer = self.generate(script).await?;
    phonon_wav::write_wav(output, &buffer).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }

  /// Generates a single segment preview.
  #[instrument(skip(self, script), fields(segment_index))]
  pub async fn preview_segment(
    &self,
    script: &PodcastScript,
    segment_index: usize,
  ) -> Result<AudioBuffer, PodcastError> {
    let segment = script.segments.get(segment_index).ok_or_else(|| {
      PodcastError::Script(format!(
        "Segment index {} out of range (total: {})",
        segment_index,
        script.segments.len()
      ))
    })?;

    let voice = script
      .speakers
      .get(&segment.speaker)
      .ok_or_else(|| PodcastError::Script(format!("Unknown speaker: {}", segment.speaker)))?;

    let buffer = self
      .tts
      .synthesize(&segment.text, voice)
      .await
      .map_err(|e| PodcastError::Tts(e.to_string()))?;

    Ok(buffer)
  }

  /// Post-process segments: resample and channel-convert to target format.
  fn post_process_segments(
    &self,
    buffers: Vec<AudioBuffer>,
    target_rate: u32,
    target_channels: u16,
  ) -> Result<Vec<AudioBuffer>, PodcastError> {
    let mut processed = Vec::with_capacity(buffers.len());

    for (i, mut buf) in buffers.into_iter().enumerate() {
      // Resample if needed
      if buf.sample_rate != target_rate {
        buf = phonon_core::resample(&buf, target_rate)
          .map_err(|e| PodcastError::Processing(format!("Resample segment {}: {}", i, e)))?;
      }

      // Channel conversion if needed
      if buf.channels != target_channels {
        buf = phonon_core::ensure_channels(&buf, target_channels)
          .map_err(|e| PodcastError::Processing(format!("Channel convert segment {}: {}", i, e)))?;
      }

      // Per-segment normalize to consistent volume
      phonon_core::normalize(&mut buf, 0.9);

      processed.push(buf);
    }

    Ok(processed)
  }

  /// Assemble segments with silence pauses or crossfades between them.
  fn assemble_segments(
    &self,
    segments: Vec<AudioBuffer>,
    config: &PipelineConfig,
  ) -> Result<AudioBuffer, PodcastError> {
    if segments.is_empty() {
      return Err(PodcastError::Processing(
        "No segments to assemble".to_string(),
      ));
    }

    let target_rate = config.output_sample_rate;
    let target_channels = config.output_channels;
    let pause_secs = config.pause_between_segments_secs;
    let crossfade_secs = config.crossfade_secs;

    let mut iter = segments.into_iter();
    let mut result = iter
      .next()
      .ok_or_else(|| PodcastError::Processing("No segments".to_string()))?;

    for next_segment in iter {
      // Insert pause between segments (if configured)
      if pause_secs > 0.0 {
        let pause = phonon_core::silence(pause_secs, target_rate, target_channels)
          .map_err(|e| PodcastError::Processing(format!("Generate pause: {}", e)))?;
        result = phonon_core::merge(&[result, pause])
          .map_err(|e| PodcastError::Processing(format!("Merge pause: {}", e)))?;
      }

      // Append the next segment (with optional crossfade)
      if crossfade_secs > 0.0 {
        result = phonon_core::crossfade(&result, &next_segment, crossfade_secs)
          .map_err(|e| PodcastError::Processing(format!("Crossfade: {}", e)))?;
      } else {
        result = phonon_core::merge(&[result, next_segment])
          .map_err(|e| PodcastError::Processing(format!("Merge segment: {}", e)))?;
      }
    }

    Ok(result)
  }

  /// Resample + channel-convert an already-loaded buffer to the target.
  fn convert_to_target(
    &self,
    mut buf: AudioBuffer,
    target_rate: u32,
    target_channels: u16,
  ) -> Result<AudioBuffer, PodcastError> {
    if buf.sample_rate != target_rate {
      buf = phonon_core::resample(&buf, target_rate)
        .map_err(|e| PodcastError::Processing(e.to_string()))?;
    }
    if buf.channels != target_channels {
      buf = phonon_core::ensure_channels(&buf, target_channels)
        .map_err(|e| PodcastError::Processing(e.to_string()))?;
    }
    Ok(buf)
  }

  /// Resolve an [`AssetSource`] into a buffer in the target format,
  /// loading from disk or invoking the configured sound generator.
  async fn resolve_asset(
    &self,
    source: &AssetSource,
    target_rate: u32,
    target_channels: u16,
  ) -> Result<AudioBuffer, PodcastError> {
    let raw = match source {
      AssetSource::File { path } => phonon_io::read_audio(path)
        .map_err(|e| PodcastError::Io(format!("Failed to load '{}': {}", path, e)))?,
      AssetSource::Generated { prompt } => self
        .sound_gen
        .generate(prompt)
        .await
        .map_err(|e| PodcastError::Processing(format!("Sound generation failed: {}", e)))?,
    };
    self.convert_to_target(raw, target_rate, target_channels)
  }

  /// Add intro/outro audio to the assembled track.
  async fn add_intro_outro(
    &self,
    mut main: AudioBuffer,
    config: &PipelineConfig,
    target_rate: u32,
    target_channels: u16,
  ) -> Result<AudioBuffer, PodcastError> {
    if let Some(intro_source) = config.effective_intro() {
      let intro = self
        .resolve_asset(&intro_source, target_rate, target_channels)
        .await?;
      main = phonon_core::merge(&[intro, main])
        .map_err(|e| PodcastError::Processing(format!("Merge intro: {}", e)))?;
    }

    if let Some(outro_source) = config.effective_outro() {
      let outro = self
        .resolve_asset(&outro_source, target_rate, target_channels)
        .await?;
      main = phonon_core::merge(&[main, outro])
        .map_err(|e| PodcastError::Processing(format!("Merge outro: {}", e)))?;
    }

    Ok(main)
  }

  /// Mix background music with the main track. Loops or trims to length.
  async fn mix_bgm(
    &self,
    main: AudioBuffer,
    config: &PipelineConfig,
    target_rate: u32,
    target_channels: u16,
  ) -> Result<AudioBuffer, PodcastError> {
    let Some(bgm_source) = config.effective_bgm() else {
      return Ok(main);
    };

    let mut bgm = self
      .resolve_asset(&bgm_source, target_rate, target_channels)
      .await?;

    // Lower BGM volume
    phonon_core::adjust_volume(&mut bgm, config.bgm_volume);

    // Loop BGM to match main track length if needed
    if bgm.samples.len() < main.samples.len() {
      let mut looped = Vec::with_capacity(main.samples.len());
      while looped.len() < main.samples.len() {
        let remaining = main.samples.len() - looped.len();
        let take = remaining.min(bgm.samples.len());
        looped.extend_from_slice(&bgm.samples[..take]);
      }
      bgm.samples = looped;
    } else {
      bgm.samples.truncate(main.samples.len());
    }

    phonon_core::mix(&main, &bgm).map_err(|e| PodcastError::Processing(format!("Mix BGM: {}", e)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_ai::{AiError, VoiceConfig};
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  /// Mock SoundGenProvider that records prompt count and returns a quiet
  /// 1 s sine at 44.1 kHz mono — long enough to be looped/trimmed by the
  /// pipeline against a typical few-segment script output.
  struct MockSoundGen {
    calls: Arc<AtomicUsize>,
  }

  impl SoundGenProvider for MockSoundGen {
    fn generate(
      &self,
      _prompt: &SoundPrompt,
    ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send {
      let calls = self.calls.clone();
      async move {
        calls.fetch_add(1, Ordering::SeqCst);
        let sr = 44100_u32;
        let samples: Vec<f32> = (0..sr as usize)
          .map(|i| {
            let t = i as f64 / sr as f64;
            (t * 220.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.3
          })
          .collect();
        Ok(AudioBuffer {
          samples,
          sample_rate: sr,
          channels: 1,
        })
      }
    }
  }

  /// A mock TTS provider that generates a sine wave for any text.
  /// The duration scales with text length to simulate real TTS.
  struct MockTts;

  impl TtsProvider for MockTts {
    async fn synthesize(&self, text: &str, _voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
      // Generate a short sine tone, duration proportional to text length
      let sample_rate = 24000_u32; // Common TTS output rate
      let duration_secs = 0.5 + (text.len() as f64 * 0.02); // ~20ms per char
      let total_samples = (duration_secs * sample_rate as f64) as usize;

      let mut samples = Vec::with_capacity(total_samples);
      for i in 0..total_samples {
        let t = i as f64 / sample_rate as f64;
        let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
        samples.push(sample);
      }

      Ok(AudioBuffer {
        samples,
        sample_rate,
        channels: 1,
      })
    }
  }

  #[tokio::test]
  async fn test_pipeline_basic() {
    let json = r#"{
            "title": "Test Episode",
            "speakers": {
                "host": { "voice_id": "alloy" },
                "guest": { "voice_id": "echo" }
            },
            "segments": [
                { "speaker": "host", "text": "Welcome to the show!" },
                { "speaker": "guest", "text": "Thanks for having me." },
                { "speaker": "host", "text": "Let us dive in." }
            ]
        }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);

    let result = pipeline.generate(&script).await.unwrap();

    // Should have produced non-empty audio
    assert!(!result.samples.is_empty());
    assert_eq!(result.sample_rate, 44100); // Default output rate
    assert_eq!(result.channels, 1);
    // Duration should be > 0
    assert!(result.duration_secs() > 0.0);
  }

  #[tokio::test]
  async fn test_pipeline_with_config() {
    let json = r#"{
            "title": "Configured Episode",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "Hello world" },
                { "speaker": "host", "text": "Goodbye world" }
            ],
            "config": {
                "pause_between_segments_secs": 1.0,
                "normalize_target": 0.8,
                "fade_in_secs": 0.5,
                "fade_out_secs": 0.5
            }
        }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);

    let result = pipeline.generate(&script).await.unwrap();

    assert!(!result.samples.is_empty());
    // With 1s pause between 2 segments, total should be > 1s
    assert!(result.duration_secs() > 1.0);
  }

  #[tokio::test]
  async fn test_pipeline_preview_segment() {
    let json = r#"{
            "title": "Preview Test",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "First segment" },
                { "speaker": "host", "text": "Second segment" }
            ]
        }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);

    let seg0 = pipeline.preview_segment(&script, 0).await.unwrap();
    let seg1 = pipeline.preview_segment(&script, 1).await.unwrap();

    assert!(!seg0.samples.is_empty());
    assert!(!seg1.samples.is_empty());

    // Segment 2 has more chars → should be slightly longer
    // ("Second segment" vs "First segment")
    assert!(seg1.samples.len() >= seg0.samples.len());

    // Out of range should error
    assert!(pipeline.preview_segment(&script, 5).await.is_err());
  }

  #[tokio::test]
  async fn test_pipeline_generated_bgm_invokes_sound_gen() {
    let json = r#"{
      "title": "Generated BGM",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Welcome to the show" },
        { "speaker": "host", "text": "And here is more content" }
      ],
      "config": {
        "bgm_volume": 0.1,
        "bgm": {
          "kind": "generated",
          "prompt": { "prompt": "ambient pad" }
        }
      }
    }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = PodcastPipeline::new(MockTts).with_sound_gen(MockSoundGen {
      calls: calls.clone(),
    });

    let result = pipeline.generate(&script).await.unwrap();
    assert_eq!(
      calls.load(Ordering::SeqCst),
      1,
      "BGM gen should be invoked once"
    );
    assert!(!result.samples.is_empty());
  }

  #[tokio::test]
  async fn test_pipeline_generated_intro_without_sound_gen_errors() {
    let json = r#"{
      "title": "No Generator",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Hello" }
      ],
      "config": {
        "intro": {
          "kind": "generated",
          "prompt": { "prompt": "jingle" }
        }
      }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let err = pipeline.generate(&script).await.expect_err("should error");
    let s = err.to_string();
    assert!(
      s.contains("SoundGenProvider") || s.contains("sound generator"),
      "unexpected error: {}",
      s
    );
  }

  #[tokio::test]
  async fn test_pipeline_legacy_bgm_audio_still_works() {
    // bgm_audio (legacy path) still loads without a sound generator.
    // Use a non-existent path to verify we *try* to load the file
    // (and thus emit an Io error, not a "no sound gen" error).
    let json = r#"{
      "title": "Legacy",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Hello" }
      ],
      "config": {
        "bgm_audio": "/nonexistent/path/bgm.wav"
      }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let err = pipeline.generate(&script).await.expect_err("should error");
    let s = err.to_string();
    assert!(s.contains("Failed to load"), "unexpected error: {}", s);
  }

  #[tokio::test]
  async fn test_generate_episode_produces_chapters_and_srt() {
    let json = r#"{
      "title": "Episode 1",
      "speakers": {
        "alice": { "voice_id": "alloy" },
        "bob":   { "voice_id": "echo" }
      },
      "segments": [
        { "speaker": "alice", "text": "First half" },
        { "speaker": "bob",   "text": "Second half" }
      ]
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let episode = pipeline.generate_episode(&script).await.unwrap();

    assert!(!episode.audio.samples.is_empty());
    assert_eq!(
      episode.chapters.len(),
      2,
      "two distinct speakers → two chapters"
    );
    assert_eq!(episode.chapters[0].title, "alice");
    assert_eq!(episode.chapters[1].title, "bob");
    let srt = episode.srt.as_deref().unwrap_or("");
    assert!(srt.contains("First half"));
    assert!(srt.contains("Second half"));
  }

  #[tokio::test]
  async fn test_pipeline_generate_to_file() {
    let json = r#"{
            "title": "File Test",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "Hello world" }
            ]
        }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);

    let tmp = std::env::temp_dir().join("phonon_test_podcast.wav");
    pipeline.generate_to_file(&script, &tmp).await.unwrap();

    // File should exist and be non-empty
    let metadata = std::fs::metadata(&tmp).unwrap();
    assert!(metadata.len() > 0);

    // Clean up
    let _ = std::fs::remove_file(&tmp);
  }
}
