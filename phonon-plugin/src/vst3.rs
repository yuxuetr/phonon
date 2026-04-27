//! VST3 plugin entry-point scaffold.
//!
//! Steinberg's VST3 SDK is a C++ COM-style API. Idiomatic Rust support
//! goes through `vst3-sys` (currently git-only) plus the SDK headers
//! built via `cmake` and `bindgen`. Until the workspace decides on a
//! distribution story for those, this module mirrors the architectural
//! seams of [`crate::clap`] so the bindings drop in cleanly when the
//! `vst3` feature is enabled.

use crate::PluginExport;

/// Descriptor for a plugin slot the VST3 factory will register.
pub struct Vst3PluginEntry {
  /// `PClassInfo.cid` — 16-byte class id derived from the plugin id.
  pub class_id: [u8; 16],
  /// Display name shown in the DAW.
  pub name: &'static str,
  /// Boxed plugin export (owned by the factory).
  pub export: Box<dyn PluginExport>,
}

/// Returns the list of plugins this binary should register with the
/// VST3 PluginFactory.
pub fn factory_entries() -> Vec<Vst3PluginEntry> {
  vec![Vst3PluginEntry {
    class_id: derive_class_id("net.phonon.volume"),
    name: "Phonon Volume",
    export: Box::new(crate::ExportedPlugin::new(
      Box::new(phonon_core::VolumePlugin::default()),
      "Phonon",
      "Linear-gain volume",
    )),
  }]
}

/// Deterministically derive a 16-byte VST3 class id from a reverse-DNS
/// plugin identifier. Uses a simple FNV-1a fold so the same id always
/// produces the same class id across builds.
fn derive_class_id(seed: &str) -> [u8; 16] {
  // FNV-1a 64-bit, repeated twice with a perturbation to fill 128 bits.
  fn fnv1a(bytes: &[u8], offset_basis: u64) -> u64 {
    let mut hash = offset_basis;
    for &b in bytes {
      hash ^= b as u64;
      hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
  }

  let lo = fnv1a(seed.as_bytes(), 0xcbf2_9ce4_8422_2325);
  let hi = fnv1a(seed.as_bytes(), 0x84222325cbf29ce4);

  let mut out = [0u8; 16];
  out[..8].copy_from_slice(&lo.to_be_bytes());
  out[8..].copy_from_slice(&hi.to_be_bytes());
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn factory_returns_at_least_one_entry() {
    let entries = factory_entries();
    assert!(!entries.is_empty());
    assert_eq!(entries[0].name, "Phonon Volume");
  }

  #[test]
  fn class_id_is_deterministic() {
    let a = derive_class_id("net.phonon.volume");
    let b = derive_class_id("net.phonon.volume");
    assert_eq!(a, b);
  }

  #[test]
  fn class_id_varies_with_seed() {
    let a = derive_class_id("net.phonon.volume");
    let b = derive_class_id("net.phonon.compressor");
    assert_ne!(a, b);
  }
}
