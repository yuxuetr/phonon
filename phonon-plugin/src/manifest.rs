//! Host-agnostic plugin manifest derived from a [`phonon_core::DspPlugin`].

use phonon_core::DspPlugin;

/// One adjustable parameter exposed to the host.
///
/// Maps directly onto:
/// * CLAP's `clap_param_info_t` (id, name, min, max, default).
/// * VST3's `IParameterInfo` (id, title, units, defaultNormalizedValue,
///   stepCount, flags).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
  /// Stable identifier (matches the underlying `PluginParam::name`).
  pub id: String,
  /// Display name shown in the host UI.
  pub display_name: String,
  /// Minimum value.
  pub min: f32,
  /// Maximum value.
  pub max: f32,
  /// Default value the host should snap to on reset.
  pub default: f32,
}

/// Host-agnostic descriptor for a plugin instance.
///
/// All the metadata a CLAP factory or VST3 class registry needs to
/// surface a Phonon plugin to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginManifest {
  /// Stable identifier (e.g. `"net.phonon.volume"`). Reused as the
  /// CLAP plugin id and the VST3 class id seed.
  pub id: String,
  /// Display name (e.g. `"Phonon Volume"`).
  pub name: String,
  /// Vendor / author shown in the DAW.
  pub vendor: String,
  /// Semantic version (taken from the workspace package version).
  pub version: String,
  /// Short tagline / description.
  pub description: String,
  /// Parameter list in declaration order.
  pub params: Vec<ParamInfo>,
}

impl PluginManifest {
  /// Returns the parameter with the given id, if any.
  pub fn param(&self, id: &str) -> Option<&ParamInfo> {
    self.params.iter().find(|p| p.id == id)
  }
}

/// Derive a [`PluginManifest`] from a [`DspPlugin`] instance.
///
/// `vendor` and `description` aren't part of the `DspPlugin` trait
/// (since they're per-product, not per-effect), so the caller supplies
/// them. The crate version comes from `CARGO_PKG_VERSION`.
pub fn manifest_from_plugin(
  plugin: &dyn DspPlugin,
  vendor: impl Into<String>,
  description: impl Into<String>,
) -> PluginManifest {
  let params = plugin
    .parameters()
    .into_iter()
    .map(|p| ParamInfo {
      id: p.name.clone(),
      display_name: p.name.clone(),
      min: p.min,
      max: p.max,
      default: p.default,
    })
    .collect();

  PluginManifest {
    id: plugin.id().to_string(),
    name: plugin.name().to_string(),
    vendor: vendor.into(),
    version: env!("CARGO_PKG_VERSION").to_string(),
    description: description.into(),
    params,
  }
}

/// Trait that lets a host adapter pull descriptor metadata, an audio
/// process callback, and a parameter setter from a boxed `DspPlugin`
/// without owning it directly.
///
/// Built-in implementations are provided for `Box<dyn DspPlugin>` so
/// you can pass any phonon-core plugin straight through to a CLAP /
/// VST3 wrapper.
pub trait PluginExport: Send + Sync {
  /// Static manifest describing this plugin.
  fn manifest(&self) -> PluginManifest;

  /// Set a named parameter to `value`. Returns `true` on success.
  fn set_param(&mut self, id: &str, value: f32) -> bool;

  /// Get the current value of a named parameter.
  fn get_param(&self, id: &str) -> Option<f32>;

  /// Process a buffer of interleaved samples in place.
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16);

  /// Reset internal state without changing parameters.
  fn reset(&mut self);
}

/// Wrapper that exposes any `DspPlugin` as a `PluginExport` while also
/// carrying the host-only metadata (vendor / description) the trait
/// needs for the manifest.
pub struct ExportedPlugin {
  inner: Box<dyn DspPlugin>,
  vendor: String,
  description: String,
}

impl ExportedPlugin {
  /// Wrap `plugin` with host-facing metadata.
  pub fn new(
    plugin: Box<dyn DspPlugin>,
    vendor: impl Into<String>,
    description: impl Into<String>,
  ) -> Self {
    Self {
      inner: plugin,
      vendor: vendor.into(),
      description: description.into(),
    }
  }
}

impl PluginExport for ExportedPlugin {
  fn manifest(&self) -> PluginManifest {
    manifest_from_plugin(
      self.inner.as_ref(),
      self.vendor.clone(),
      self.description.clone(),
    )
  }

  fn set_param(&mut self, id: &str, value: f32) -> bool {
    self.inner.set_parameter(id, value).is_ok()
  }

  fn get_param(&self, id: &str) -> Option<f32> {
    self.inner.get_parameter(id)
  }

  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }

  fn reset(&mut self) {
    self.inner.reset();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_core::{CompressorPlugin, ParametricEqPlugin, VolumePlugin};

  #[test]
  fn manifest_from_volume_plugin() {
    let p = VolumePlugin::new(0.8);
    let m = manifest_from_plugin(&p, "Phonon", "Linear gain");
    assert_eq!(m.id, "builtin.volume");
    assert_eq!(m.name, "Volume");
    assert_eq!(m.vendor, "Phonon");
    assert_eq!(m.params.len(), 1);
    assert_eq!(m.params[0].id, "gain");
    assert!((m.params[0].default - 0.8).abs() < 1e-6);
    assert!((m.params[0].min - 0.0).abs() < 1e-6);
    assert!((m.params[0].max - 10.0).abs() < 1e-6);
  }

  #[test]
  fn manifest_from_compressor_plugin_has_four_params() {
    let p = CompressorPlugin::new(0.4, 4.0, 5.0, 50.0);
    let m = manifest_from_plugin(&p, "Phonon", "");
    let ids: Vec<_> = m.params.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["threshold", "ratio", "attack_ms", "release_ms"]);
  }

  #[test]
  fn manifest_param_lookup() {
    let p = ParametricEqPlugin::new();
    let m = manifest_from_plugin(&p, "Phonon", "");
    assert!(m.param("band_0_gain").is_some());
    assert!(m.param("does.not.exist").is_none());
  }

  #[test]
  fn exported_plugin_processes_audio() {
    let plug: Box<dyn DspPlugin> = Box::new(VolumePlugin::new(0.5));
    let mut export = ExportedPlugin::new(plug, "Phonon", "Volume");

    let mut samples = vec![1.0_f32, -1.0];
    export.process(&mut samples, 44_100, 1);
    assert!((samples[0] - 0.5).abs() < 1e-6);
    assert!((samples[1] + 0.5).abs() < 1e-6);
  }

  #[test]
  fn exported_plugin_param_round_trip() {
    let plug: Box<dyn DspPlugin> = Box::new(VolumePlugin::new(1.0));
    let mut export = ExportedPlugin::new(plug, "Phonon", "Volume");
    assert!(export.set_param("gain", 0.25));
    assert!(!export.set_param("does.not.exist", 0.0));
    assert!((export.get_param("gain").unwrap_or(0.0) - 0.25).abs() < 1e-6);
  }

  #[test]
  fn manifest_version_matches_crate() {
    let p = VolumePlugin::default();
    let m = manifest_from_plugin(&p, "Phonon", "");
    assert_eq!(m.version, env!("CARGO_PKG_VERSION"));
  }
}
