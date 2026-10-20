//! Plugin architecture for custom DSP effects.
//!
//! Provides a [`DspPlugin`] trait for writing audio processing plugins,
//! a [`PluginParam`] descriptor for exposable parameters, and a
//! [`PluginRegistry`] for registering / instantiating plugins by ID.

use crate::{AudioBuffer, PhononError, Result};
use std::collections::HashMap;

// ─── PluginParam ────────────────────────────────────────────────────

/// Describes a single adjustable parameter of a plugin.
#[derive(Debug, Clone)]
pub struct PluginParam {
  /// Machine-readable name (e.g. `"gain"`, `"cutoff_hz"`).
  pub name: String,
  /// Minimum allowed value.
  pub min: f32,
  /// Maximum allowed value.
  pub max: f32,
  /// Default value.
  pub default: f32,
  /// Current value.
  pub current: f32,
}

impl PluginParam {
  /// Create a new parameter with the given bounds and default.
  /// `current` is initialised to `default`.
  pub fn new(name: impl Into<String>, min: f32, max: f32, default: f32) -> Self {
    Self {
      name: name.into(),
      min,
      max,
      default,
      current: default,
    }
  }

  /// Clamp `value` to `[min, max]` and set `current`.
  pub fn set(&mut self, value: f32) {
    self.current = value.clamp(self.min, self.max);
  }

  /// Reset `current` to `default`.
  pub fn reset(&mut self) {
    self.current = self.default;
  }
}

// ─── DspPlugin trait ────────────────────────────────────────────────

/// A DSP plugin that processes audio and exposes tuneable parameters.
///
/// Implementations must be `Send + Sync` so they can be used across
/// threads (e.g. in an audio callback).
pub trait DspPlugin: Send + Sync {
  /// Unique identifier (e.g. `"builtin.volume"`).
  fn id(&self) -> &str;

  /// Human-readable display name.
  fn name(&self) -> &str;

  /// List of adjustable parameters.
  fn parameters(&self) -> Vec<PluginParam>;

  /// Set a parameter by name. Returns an error if the name is unknown.
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()>;

  /// Get the current value of a parameter, or `None` if unknown.
  fn get_parameter(&self, name: &str) -> Option<f32>;

  /// Process audio samples in-place.
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16);

  /// Reset any internal state (e.g. filter memory) without changing params.
  fn reset(&mut self);
}

// ─── PluginRegistry ─────────────────────────────────────────────────

type PluginFactory = Box<dyn Fn() -> Box<dyn DspPlugin> + Send + Sync>;

/// Registry that maps plugin IDs to factory functions.
pub struct PluginRegistry {
  factories: HashMap<String, PluginFactory>,
}

impl PluginRegistry {
  /// Create an empty registry.
  pub fn new() -> Self {
    Self {
      factories: HashMap::new(),
    }
  }

  /// Create a registry pre-loaded with all built-in plugins.
  pub fn with_builtins() -> Self {
    let mut reg = Self::new();
    reg.register(VolumePlugin::ID, || Box::new(VolumePlugin::default()));
    reg.register(LowPassPlugin::ID, || Box::new(LowPassPlugin::default()));
    reg.register(HighPassPlugin::ID, || Box::new(HighPassPlugin::default()));
    reg.register(CompressorPlugin::ID, || {
      Box::new(CompressorPlugin::default())
    });
    reg.register(ParametricEqPlugin::ID, || {
      Box::new(ParametricEqPlugin::default())
    });
    reg
  }

  /// Register a factory function for the given `id`.
  ///
  /// If an entry with the same `id` already exists it is replaced.
  pub fn register<F>(&mut self, id: &str, factory: F)
  where
    F: Fn() -> Box<dyn DspPlugin> + Send + Sync + 'static,
  {
    self.factories.insert(id.to_owned(), Box::new(factory));
  }

  /// Create a new instance of the plugin identified by `id`.
  pub fn create(&self, id: &str) -> Option<Box<dyn DspPlugin>> {
    self.factories.get(id).map(|f| f())
  }

  /// List all registered plugin IDs.
  pub fn list_ids(&self) -> Vec<&str> {
    self.factories.keys().map(String::as_str).collect()
  }

  /// Returns the number of registered plugins.
  pub fn len(&self) -> usize {
    self.factories.len()
  }

  /// Returns `true` if the registry is empty.
  pub fn is_empty(&self) -> bool {
    self.factories.is_empty()
  }
}

impl Default for PluginRegistry {
  fn default() -> Self {
    Self::new()
  }
}

// ─── Helper ─────────────────────────────────────────────────────────

fn find_param_mut<'a>(params: &'a mut [PluginParam], name: &str) -> Result<&'a mut PluginParam> {
  params
    .iter_mut()
    .find(|p| p.name == name)
    .ok_or_else(|| PhononError::InvalidParam(format!("unknown parameter: {name}")))
}

fn find_param(params: &[PluginParam], name: &str) -> Option<f32> {
  params.iter().find(|p| p.name == name).map(|p| p.current)
}

// ═══════════════════════════════════════════════════════════════════
// Built-in plugins
// ═══════════════════════════════════════════════════════════════════

// ─── Volume ─────────────────────────────────────────────────────────

/// Simple volume (gain) plugin.
pub struct VolumePlugin {
  params: Vec<PluginParam>,
}

impl VolumePlugin {
  pub const ID: &str = "builtin.volume";

  pub fn new(gain: f32) -> Self {
    Self {
      params: vec![PluginParam::new("gain", 0.0, 10.0, gain)],
    }
  }
}

impl Default for VolumePlugin {
  fn default() -> Self {
    Self::new(1.0)
  }
}

impl DspPlugin for VolumePlugin {
  fn id(&self) -> &str {
    Self::ID
  }
  fn name(&self) -> &str {
    "Volume"
  }
  fn parameters(&self) -> Vec<PluginParam> {
    self.params.clone()
  }
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()> {
    find_param_mut(&mut self.params, name)?.set(value);
    Ok(())
  }
  fn get_parameter(&self, name: &str) -> Option<f32> {
    find_param(&self.params, name)
  }
  fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
    let gain = self.params[0].current;
    for s in samples.iter_mut() {
      *s *= gain;
    }
  }
  fn reset(&mut self) {}
}

// ─── LowPass ────────────────────────────────────────────────────────

/// First-order low-pass filter plugin.
pub struct LowPassPlugin {
  params: Vec<PluginParam>,
  prev: [f64; 2],
}

impl LowPassPlugin {
  pub const ID: &str = "builtin.lowpass";

  pub fn new(cutoff_hz: f32) -> Self {
    Self {
      params: vec![PluginParam::new("cutoff_hz", 20.0, 20000.0, cutoff_hz)],
      prev: [0.0; 2],
    }
  }
}

impl Default for LowPassPlugin {
  fn default() -> Self {
    Self::new(1000.0)
  }
}

impl DspPlugin for LowPassPlugin {
  fn id(&self) -> &str {
    Self::ID
  }
  fn name(&self) -> &str {
    "Low-Pass Filter"
  }
  fn parameters(&self) -> Vec<PluginParam> {
    self.params.clone()
  }
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()> {
    find_param_mut(&mut self.params, name)?.set(value);
    Ok(())
  }
  fn get_parameter(&self, name: &str) -> Option<f32> {
    find_param(&self.params, name)
  }
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    let cutoff = self.params[0].current as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
    let dt = 1.0 / sample_rate as f64;
    let alpha = dt / (rc + dt);
    let ch = channels as usize;

    for i in (0..samples.len()).step_by(ch.max(1)) {
      for c in 0..ch.min(2) {
        let idx = i + c;
        if idx < samples.len() {
          let input = samples[idx] as f64;
          self.prev[c] += alpha * (input - self.prev[c]);
          samples[idx] = self.prev[c] as f32;
        }
      }
    }
  }
  fn reset(&mut self) {
    self.prev = [0.0; 2];
  }
}

// ─── HighPass ───────────────────────────────────────────────────────

/// First-order high-pass filter plugin.
pub struct HighPassPlugin {
  params: Vec<PluginParam>,
  prev_input: [f64; 2],
  prev_output: [f64; 2],
}

impl HighPassPlugin {
  pub const ID: &str = "builtin.highpass";

  pub fn new(cutoff_hz: f32) -> Self {
    Self {
      params: vec![PluginParam::new("cutoff_hz", 20.0, 20000.0, cutoff_hz)],
      prev_input: [0.0; 2],
      prev_output: [0.0; 2],
    }
  }
}

impl Default for HighPassPlugin {
  fn default() -> Self {
    Self::new(1000.0)
  }
}

impl DspPlugin for HighPassPlugin {
  fn id(&self) -> &str {
    Self::ID
  }
  fn name(&self) -> &str {
    "High-Pass Filter"
  }
  fn parameters(&self) -> Vec<PluginParam> {
    self.params.clone()
  }
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()> {
    find_param_mut(&mut self.params, name)?.set(value);
    Ok(())
  }
  fn get_parameter(&self, name: &str) -> Option<f32> {
    find_param(&self.params, name)
  }
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    let cutoff = self.params[0].current as f64;
    let rc = 1.0 / (2.0 * std::f64::consts::PI * cutoff);
    let dt = 1.0 / sample_rate as f64;
    let alpha = rc / (rc + dt);
    let ch = channels as usize;

    for i in (0..samples.len()).step_by(ch.max(1)) {
      for c in 0..ch.min(2) {
        let idx = i + c;
        if idx < samples.len() {
          let input = samples[idx] as f64;
          self.prev_output[c] = alpha * (self.prev_output[c] + input - self.prev_input[c]);
          self.prev_input[c] = input;
          samples[idx] = self.prev_output[c] as f32;
        }
      }
    }
  }
  fn reset(&mut self) {
    self.prev_input = [0.0; 2];
    self.prev_output = [0.0; 2];
  }
}

// ─── Compressor ─────────────────────────────────────────────────────

/// Hard-knee compressor plugin.
pub struct CompressorPlugin {
  params: Vec<PluginParam>,
  envelope: f32,
}

impl CompressorPlugin {
  pub const ID: &str = "builtin.compressor";

  pub fn new(threshold: f32, ratio: f32, attack_ms: f32, release_ms: f32) -> Self {
    Self {
      params: vec![
        PluginParam::new("threshold", 0.0, 1.0, threshold),
        PluginParam::new("ratio", 1.0, 20.0, ratio),
        PluginParam::new("attack_ms", 0.1, 500.0, attack_ms),
        PluginParam::new("release_ms", 1.0, 2000.0, release_ms),
      ],
      envelope: 0.0,
    }
  }
}

impl Default for CompressorPlugin {
  fn default() -> Self {
    Self::new(0.5, 4.0, 5.0, 50.0)
  }
}

impl DspPlugin for CompressorPlugin {
  fn id(&self) -> &str {
    Self::ID
  }
  fn name(&self) -> &str {
    "Compressor"
  }
  fn parameters(&self) -> Vec<PluginParam> {
    self.params.clone()
  }
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()> {
    find_param_mut(&mut self.params, name)?.set(value);
    Ok(())
  }
  fn get_parameter(&self, name: &str) -> Option<f32> {
    find_param(&self.params, name)
  }
  /// Channels are linked: one envelope per frame (driven by the frame's
  /// peak) and one gain for all its channels.
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    let threshold = self.params[0].current;
    let ratio = self.params[1].current;
    let attack_ms = self.params[2].current;
    let release_ms = self.params[3].current;

    let attack = (-1.0 / (attack_ms * 0.001 * sample_rate as f32)).exp();
    let release = (-1.0 / (release_ms * 0.001 * sample_rate as f32)).exp();

    for frame in samples.chunks_mut((channels as usize).max(1)) {
      let input_abs = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
      if input_abs > self.envelope {
        self.envelope = attack * self.envelope + (1.0 - attack) * input_abs;
      } else {
        self.envelope = release * self.envelope + (1.0 - release) * input_abs;
      }

      if self.envelope > threshold {
        let over = self.envelope - threshold;
        let compressed_over = over / ratio;
        let target = threshold + compressed_over;
        let gain = if self.envelope > 0.0 {
          target / self.envelope
        } else {
          1.0
        };
        for s in frame {
          *s *= gain;
        }
      }
    }
  }
  fn reset(&mut self) {
    self.envelope = 0.0;
  }
}

// ─── ParametricEq ───────────────────────────────────────────────────

/// 5-band parametric EQ plugin.
///
/// Provides 5 peaking EQ bands at standard frequencies (80, 250, 1k, 4k, 12k Hz).
/// Each band has a `gain_db` parameter named `band_N_gain` (0-indexed).
pub struct ParametricEqPlugin {
  params: Vec<PluginParam>,
  eq: Option<crate::ParametricEq>,
  last_sample_rate: u32,
  last_channels: u16,
  dirty: bool,
}

impl ParametricEqPlugin {
  pub const ID: &str = "builtin.parametric_eq";

  const FREQ: [f64; 5] = [80.0, 250.0, 1000.0, 4000.0, 12000.0];
  const Q: f64 = 1.0;

  pub fn new() -> Self {
    let params = (0..5)
      .map(|i| PluginParam::new(format!("band_{i}_gain"), -12.0, 12.0, 0.0))
      .collect();
    Self {
      params,
      eq: None,
      last_sample_rate: 0,
      last_channels: 0,
      dirty: true,
    }
  }

  fn rebuild(&mut self, sample_rate: u32, channels: u16) {
    let bands: Vec<crate::EqBand> = self
      .params
      .iter()
      .enumerate()
      .map(|(i, p)| crate::EqBand {
        filter_type: crate::FilterType::Peaking,
        freq_hz: Self::FREQ[i],
        q: Self::Q,
        gain_db: p.current as f64,
      })
      .collect();
    self.eq = Some(crate::ParametricEq::new(bands, sample_rate, channels));
    self.last_sample_rate = sample_rate;
    self.last_channels = channels;
    self.dirty = false;
  }
}

impl Default for ParametricEqPlugin {
  fn default() -> Self {
    Self::new()
  }
}

impl DspPlugin for ParametricEqPlugin {
  fn id(&self) -> &str {
    Self::ID
  }
  fn name(&self) -> &str {
    "Parametric EQ"
  }
  fn parameters(&self) -> Vec<PluginParam> {
    self.params.clone()
  }
  fn set_parameter(&mut self, name: &str, value: f32) -> Result<()> {
    find_param_mut(&mut self.params, name)?.set(value);
    self.dirty = true;
    Ok(())
  }
  fn get_parameter(&self, name: &str) -> Option<f32> {
    find_param(&self.params, name)
  }
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    if self.dirty
      || self.eq.is_none()
      || sample_rate != self.last_sample_rate
      || channels != self.last_channels
    {
      self.rebuild(sample_rate, channels);
    }
    if let Some(eq) = &mut self.eq {
      eq.process(samples);
    }
  }
  fn reset(&mut self) {
    self.eq = None;
    self.dirty = true;
  }
}

// ─── Convenience: apply a plugin to an AudioBuffer ──────────────────

/// Apply a plugin to an entire [`AudioBuffer`].
pub fn apply_plugin(plugin: &mut dyn DspPlugin, buffer: &mut AudioBuffer) {
  plugin.process(&mut buffer.samples, buffer.sample_rate, buffer.channels);
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_compressor_plugin_stereo_dynamics_match_mono() {
    let quiet = 2_400;
    let mono: Vec<f32> = (0..quiet * 2)
      .map(|i| if i < quiet { 0.05 } else { 0.9 })
      .collect();
    let mut stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
    let mut mono = mono;

    CompressorPlugin::default().process(&mut mono, 48_000, 1);
    CompressorPlugin::default().process(&mut stereo, 48_000, 2);

    for (i, (&m, frame)) in mono.iter().zip(stereo.chunks(2)).enumerate() {
      assert!(
        (m - frame[0]).abs() < 1e-6 && (m - frame[1]).abs() < 1e-6,
        "frame {i}: mono {m} vs stereo {frame:?}"
      );
    }
  }

  #[test]
  fn test_volume_plugin_default() {
    let mut p = VolumePlugin::default();
    assert_eq!(p.id(), "builtin.volume");
    assert_eq!(p.name(), "Volume");
    assert_eq!(p.parameters().len(), 1);
    assert!((p.get_parameter("gain").unwrap_or(0.0) - 1.0).abs() < 1e-6);

    let mut samples = vec![0.5_f32, -0.5];
    p.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_volume_plugin_boost() {
    let mut p = VolumePlugin::new(2.0);
    let mut samples = vec![0.3_f32, -0.3];
    p.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.6).abs() < 1e-6);
  }

  #[test]
  fn test_set_parameter() {
    let mut p = VolumePlugin::default();
    p.set_parameter("gain", 0.5).unwrap();
    assert!((p.get_parameter("gain").unwrap_or(0.0) - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_set_unknown_parameter() {
    let mut p = VolumePlugin::default();
    assert!(p.set_parameter("nonexistent", 1.0).is_err());
  }

  #[test]
  fn test_parameter_clamping() {
    let mut p = VolumePlugin::default();
    p.set_parameter("gain", 999.0).unwrap();
    assert!((p.get_parameter("gain").unwrap_or(0.0) - 10.0).abs() < 1e-6);
  }

  #[test]
  fn test_lowpass_plugin() {
    let mut lp = LowPassPlugin::new(500.0);
    // High-freq signal should be attenuated
    let mut samples: Vec<f32> = (0..1024)
      .map(|i| {
        let t = i as f64 / 44100.0;
        (t * 10000.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();
    let orig_energy: f32 = samples.iter().map(|s| s * s).sum();
    lp.process(&mut samples, 44100, 1);
    let filt_energy: f32 = samples.iter().map(|s| s * s).sum();
    assert!(filt_energy < orig_energy * 0.1);
  }

  #[test]
  fn test_highpass_plugin() {
    let mut hp = HighPassPlugin::new(1000.0);
    // Low-freq signal should be attenuated
    let mut samples: Vec<f32> = (0..4410)
      .map(|i| {
        let t = i as f64 / 44100.0;
        (t * 50.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();
    let orig_energy: f32 = samples.iter().map(|s| s * s).sum();
    hp.process(&mut samples, 44100, 1);
    let filt_energy: f32 = samples.iter().map(|s| s * s).sum();
    assert!(filt_energy < orig_energy * 0.3);
  }

  #[test]
  fn test_compressor_plugin() {
    let mut comp = CompressorPlugin::new(0.3, 4.0, 1.0, 10.0);
    let mut samples = vec![0.8_f32; 4410];
    comp.process(&mut samples, 44100, 1);
    let tail_peak = samples[2000..]
      .iter()
      .map(|s| s.abs())
      .fold(0.0_f32, f32::max);
    assert!(tail_peak < 0.8);
  }

  #[test]
  fn test_parametric_eq_plugin() {
    let mut eq = ParametricEqPlugin::new();
    assert_eq!(eq.parameters().len(), 5);
    eq.set_parameter("band_0_gain", 6.0).unwrap();
    assert!((eq.get_parameter("band_0_gain").unwrap_or(0.0) - 6.0).abs() < 1e-6);

    let mut samples = vec![0.5_f32; 4410];
    eq.process(&mut samples, 44100, 1);
    // Should produce output without panic
    assert!(!samples.is_empty());
  }

  #[test]
  fn test_registry_builtins() {
    let reg = PluginRegistry::with_builtins();
    assert_eq!(reg.len(), 5);

    let ids = reg.list_ids();
    assert!(ids.contains(&"builtin.volume"));
    assert!(ids.contains(&"builtin.lowpass"));
    assert!(ids.contains(&"builtin.highpass"));
    assert!(ids.contains(&"builtin.compressor"));
    assert!(ids.contains(&"builtin.parametric_eq"));
  }

  #[test]
  fn test_registry_create() {
    let reg = PluginRegistry::with_builtins();
    let plugin = reg.create("builtin.volume");
    assert!(plugin.is_some());
    let p = plugin.unwrap();
    assert_eq!(p.id(), "builtin.volume");
  }

  #[test]
  fn test_registry_create_unknown() {
    let reg = PluginRegistry::with_builtins();
    assert!(reg.create("nonexistent").is_none());
  }

  #[test]
  fn test_apply_plugin_to_buffer() {
    let mut plugin = VolumePlugin::new(0.5);
    let mut buffer = AudioBuffer {
      samples: vec![1.0, -1.0, 0.5],
      sample_rate: 44100,
      channels: 1,
    };
    apply_plugin(&mut plugin, &mut buffer);
    assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
    assert!((buffer.samples[1] - (-0.5)).abs() < 1e-6);
  }

  #[test]
  fn test_plugin_reset() {
    let mut lp = LowPassPlugin::new(500.0);
    let mut samples = vec![1.0_f32; 100];
    lp.process(&mut samples, 44100, 1);
    // After reset, state should be zeroed
    lp.reset();
    assert_eq!(lp.prev, [0.0; 2]);
  }

  #[test]
  fn test_custom_plugin_registration() {
    let mut reg = PluginRegistry::new();
    assert!(reg.is_empty());
    reg.register("custom.test", || Box::new(VolumePlugin::new(0.5)));
    assert_eq!(reg.len(), 1);
    let p = reg.create("custom.test").unwrap();
    assert_eq!(p.id(), "builtin.volume"); // same impl, different registry key
  }
}
