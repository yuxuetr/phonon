//! In-memory `AudioHandle` registry.
//!
//! LLM tool calls reference audio buffers by UUID rather than moving raw
//! PCM bytes through the JSON-RPC channel. The registry stores `Arc<AudioBuffer>`
//! so handlers can clone-on-write cheaply.

use crate::error::McpError;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use uuid::Uuid;

/// A handle to an audio buffer stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct AudioHandle(pub String);

impl AudioHandle {
  pub fn new() -> Self {
    Self(Uuid::new_v4().to_string())
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl Default for AudioHandle {
  fn default() -> Self {
    Self::new()
  }
}

impl std::fmt::Display for AudioHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.0)
  }
}

/// Default cap on PCM held by one registry: 1 GiB of `f32` samples.
pub const DEFAULT_MAX_BYTES: usize = 1 << 30;

#[derive(Debug, Default)]
struct Store {
  buffers: HashMap<AudioHandle, Arc<AudioBuffer>>,
  bytes: usize,
}

fn pcm_bytes(buffer: &AudioBuffer) -> usize {
  buffer.samples.len() * std::mem::size_of::<f32>()
}

/// Thread-safe registry of audio buffers keyed by [`AudioHandle`].
///
/// Cloning the registry yields a handle to the same shared store
/// (cheap `Arc::clone`). All mutations go through an internal `Mutex`.
/// A poisoned lock is recovered: every critical section leaves the map and
/// its byte count consistent before anything that could panic.
///
/// Every DSP tool returns a new handle, so the total PCM held is capped;
/// inserts beyond the cap fail until the client drops handles.
#[derive(Debug, Clone)]
pub struct AssetRegistry {
  inner: Arc<Mutex<Store>>,
  max_bytes: usize,
}

impl Default for AssetRegistry {
  fn default() -> Self {
    Self::with_max_bytes(DEFAULT_MAX_BYTES)
  }
}

impl AssetRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  /// A registry holding at most `max_bytes` of PCM.
  pub fn with_max_bytes(max_bytes: usize) -> Self {
    Self {
      inner: Arc::default(),
      max_bytes,
    }
  }

  /// Insert a buffer and return its handle, or [`McpError::RegistryFull`]
  /// if it would exceed the cap.
  pub fn insert(&self, buffer: AudioBuffer) -> Result<AudioHandle, McpError> {
    let size = pcm_bytes(&buffer);
    let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    if g.bytes + size > self.max_bytes {
      return Err(McpError::RegistryFull(format!(
        "{} MB in use of {} MB, this buffer needs {} MB; free memory with audio_drop",
        g.bytes >> 20,
        self.max_bytes >> 20,
        size.div_ceil(1 << 20)
      )));
    }
    let handle = AudioHandle::new();
    g.buffers.insert(handle.clone(), Arc::new(buffer));
    g.bytes += size;
    Ok(handle)
  }

  /// Fetch a buffer by handle.
  pub fn get(&self, handle: &AudioHandle) -> Result<Arc<AudioBuffer>, McpError> {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.buffers
      .get(handle)
      .cloned()
      .ok_or_else(|| McpError::HandleNotFound(handle.0.clone()))
  }

  /// Remove a buffer; returns true if it was present.
  pub fn drop(&self, handle: &AudioHandle) -> bool {
    let removed = {
      let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
      let removed = g.buffers.remove(handle);
      if let Some(buffer) = &removed {
        g.bytes -= pcm_bytes(buffer);
      }
      removed
    };
    // The PCM (if no tool still holds it) is freed after unlocking.
    removed.is_some()
  }

  /// List all current handles (in unspecified order).
  pub fn handles(&self) -> Vec<AudioHandle> {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.buffers.keys().cloned().collect()
  }

  /// Number of buffers currently stored.
  pub fn len(&self) -> usize {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.buffers.len()
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn buf() -> AudioBuffer {
    AudioBuffer {
      samples: vec![0.0; 100],
      sample_rate: 16000,
      channels: 1,
    }
  }

  #[test]
  fn test_insert_and_get_roundtrip() {
    let r = AssetRegistry::new();
    let h = r.insert(buf()).unwrap();
    let got = r.get(&h).unwrap();
    assert_eq!(got.samples.len(), 100);
  }

  #[test]
  fn test_handles_are_unique() {
    let r = AssetRegistry::new();
    let h1 = r.insert(buf()).unwrap();
    let h2 = r.insert(buf()).unwrap();
    assert_ne!(h1, h2);
    assert_eq!(r.len(), 2);
  }

  #[test]
  fn test_drop_removes_entry() {
    let r = AssetRegistry::new();
    let h = r.insert(buf()).unwrap();
    assert!(r.drop(&h));
    assert!(r.get(&h).is_err());
    // double-drop returns false
    assert!(!r.drop(&h));
  }

  #[test]
  fn test_handle_not_found() {
    let r = AssetRegistry::new();
    let bogus = AudioHandle("not-a-real-uuid".into());
    let err = r.get(&bogus).unwrap_err();
    assert!(matches!(err, McpError::HandleNotFound(_)));
  }

  #[test]
  fn test_insert_beyond_cap_fails_until_dropped() {
    // Room for exactly two 100-sample buffers.
    let r = AssetRegistry::with_max_bytes(800);
    let h1 = r.insert(buf()).unwrap();
    r.insert(buf()).unwrap();
    assert!(matches!(r.insert(buf()), Err(McpError::RegistryFull(_))));
    assert!(r.drop(&h1));
    r.insert(buf()).unwrap();
    assert_eq!(r.len(), 2);
  }
}
