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

/// Thread-safe registry of audio buffers keyed by [`AudioHandle`].
///
/// Cloning the registry yields a handle to the same shared store
/// (cheap `Arc::clone`). All mutations go through an internal `Mutex`.
/// A poisoned lock is recovered: every critical section is a single
/// `HashMap` call, so a panic elsewhere cannot leave the map half-updated.
#[derive(Debug, Clone, Default)]
pub struct AssetRegistry {
  inner: Arc<Mutex<HashMap<AudioHandle, Arc<AudioBuffer>>>>,
}

impl AssetRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  /// Insert a buffer and return its handle.
  pub fn insert(&self, buffer: AudioBuffer) -> AudioHandle {
    let handle = AudioHandle::new();
    let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.insert(handle.clone(), Arc::new(buffer));
    handle
  }

  /// Insert a buffer with a caller-provided handle, replacing any prior
  /// entry. Used by tests and for fixed-id assets.
  pub fn insert_with(&self, handle: AudioHandle, buffer: AudioBuffer) {
    let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.insert(handle, Arc::new(buffer));
  }

  /// Fetch a buffer by handle.
  pub fn get(&self, handle: &AudioHandle) -> Result<Arc<AudioBuffer>, McpError> {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.get(handle)
      .cloned()
      .ok_or_else(|| McpError::HandleNotFound(handle.0.clone()))
  }

  /// Remove a buffer; returns true if it was present.
  pub fn drop(&self, handle: &AudioHandle) -> bool {
    let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.remove(handle).is_some()
  }

  /// List all current handles (in unspecified order).
  pub fn handles(&self) -> Vec<AudioHandle> {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.keys().cloned().collect()
  }

  /// Number of buffers currently stored.
  pub fn len(&self) -> usize {
    let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
    g.len()
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
    let h = r.insert(buf());
    let got = r.get(&h).unwrap();
    assert_eq!(got.samples.len(), 100);
  }

  #[test]
  fn test_handles_are_unique() {
    let r = AssetRegistry::new();
    let h1 = r.insert(buf());
    let h2 = r.insert(buf());
    assert_ne!(h1, h2);
    assert_eq!(r.len(), 2);
  }

  #[test]
  fn test_drop_removes_entry() {
    let r = AssetRegistry::new();
    let h = r.insert(buf());
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
}
