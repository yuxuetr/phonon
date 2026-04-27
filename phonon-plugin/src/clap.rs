//! CLAP plugin entry-point scaffold.
//!
//! Building a loadable `.clap` binary requires:
//! 1. A `clap_entry` symbol of type `clap_plugin_entry_t` exporting
//!    `init`/`deinit`/`get_factory`.
//! 2. A `clap_plugin_factory_t` whose `get_plugin_count`/
//!    `get_plugin_descriptor`/`create_plugin` methods enumerate the
//!    plugins this binary ships.
//! 3. Per-plugin `clap_plugin_t` vtable + `params`/`audio_ports`/
//!    `state` extensions.
//!
//! Once `clap-sys = "0.5"` is wired in, the [`PluginExport`] trait in
//! the parent module provides the audio-process and parameter
//! callbacks we'd hand off to the host. This module currently keeps
//! the architectural seams explicit so the bindings drop in cleanly.

use crate::PluginExport;

/// Descriptor for a plugin slot the CLAP factory will offer to the
/// host. Wraps an [`PluginExport`] together with the C-string fields
/// CLAP needs verbatim.
pub struct ClapPluginEntry {
  /// `clap_plugin_descriptor_t.id` — UTF-8 reverse-DNS string.
  pub clap_id: &'static str,
  /// Boxed plugin export (owned by the factory).
  pub export: Box<dyn PluginExport>,
}

/// Returns the list of plugins this binary should expose to a CLAP host.
///
/// Wire this from the `clap_entry`'s factory `get_plugin_count` and
/// `get_plugin_descriptor` callbacks once `clap-sys` is enabled.
pub fn factory_entries() -> Vec<ClapPluginEntry> {
  // Default bundle — phonon-core's built-in volume plugin. Downstream
  // crates can build their own factory by composing `ExportedPlugin`s.
  vec![ClapPluginEntry {
    clap_id: "net.phonon.volume",
    export: Box::new(crate::ExportedPlugin::new(
      Box::new(phonon_core::VolumePlugin::default()),
      "Phonon",
      "Linear-gain volume",
    )),
  }]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn factory_returns_at_least_one_entry() {
    let entries = factory_entries();
    assert!(!entries.is_empty());
    let first = &entries[0];
    assert_eq!(first.clap_id, "net.phonon.volume");
    let m = first.export.manifest();
    assert_eq!(m.name, "Volume");
  }
}
