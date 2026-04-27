//! Real-time audio effect processing.
//!
//! Provides an [`AudioEffect`] trait, an [`EffectChain`] runtime, and two
//! adapters that bridge it to [`phonon_core::DspPlugin`]:
//!
//! * [`PluginAdapter`] — a `DspPlugin` used as an `AudioEffect`. The
//!   preferred path: build effects from the core plugin registry
//!   ([`phonon_core::PluginRegistry`]) and push them via
//!   [`AudioPlayer::add_plugin`](crate::AudioPlayer::add_plugin).
//! * [`EffectAsPlugin`] — an `AudioEffect` exposed as a parameter-less
//!   `DspPlugin`, useful when you need to put a real-time-only effect
//!   into a plugin-shaped slot (e.g. saved-chain serialization).
//!
//! The legacy in-engine effect types (`VolumeEffect`, `LowPassEffect`,
//! `HighPassEffect`, `CompressorEffect`, `ParametricEqEffect`) remain
//! as deprecated thin wrappers around the equivalent core plugins so
//! existing call sites still compile while users migrate.

use phonon_core::{
  CompressorPlugin, DspPlugin, EqBand, HighPassPlugin, LowPassPlugin, ParametricEq, VolumePlugin,
};

/// A real-time audio effect that processes samples in-place.
pub trait AudioEffect: Send + Sync {
  /// Process a buffer of interleaved audio samples in-place.
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16);

  /// Returns a human-readable name for this effect.
  fn name(&self) -> &str;
}

/// A chain of audio effects applied sequentially.
pub struct EffectChain {
  effects: Vec<Box<dyn AudioEffect>>,
}

impl EffectChain {
  /// Creates an empty effect chain.
  pub fn new() -> Self {
    Self {
      effects: Vec::new(),
    }
  }

  /// Adds an effect to the end of the chain.
  pub fn push(&mut self, effect: Box<dyn AudioEffect>) {
    self.effects.push(effect);
  }

  /// Adds a [`phonon_core::DspPlugin`] to the chain via [`PluginAdapter`].
  pub fn push_plugin(&mut self, plugin: Box<dyn DspPlugin>) {
    self.effects.push(Box::new(PluginAdapter::new(plugin)));
  }

  /// Removes all effects from the chain.
  pub fn clear(&mut self) {
    self.effects.clear();
  }

  /// Returns the number of effects in the chain.
  pub fn len(&self) -> usize {
    self.effects.len()
  }

  /// Returns `true` if the chain has no effects.
  pub fn is_empty(&self) -> bool {
    self.effects.is_empty()
  }

  /// Processes samples through all effects in order.
  pub fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    for effect in &mut self.effects {
      effect.process(samples, sample_rate, channels);
    }
  }
}

impl Default for EffectChain {
  fn default() -> Self {
    Self::new()
  }
}

// ─── DspPlugin → AudioEffect adapter ────────────────────────────────

/// Adapter that wraps a [`phonon_core::DspPlugin`] as an [`AudioEffect`],
/// allowing any plugin from phonon-core to be used in an [`EffectChain`].
pub struct PluginAdapter {
  inner: Box<dyn DspPlugin>,
}

impl PluginAdapter {
  /// Wraps a `DspPlugin` so it can be used as an `AudioEffect`.
  pub fn new(plugin: Box<dyn DspPlugin>) -> Self {
    Self { inner: plugin }
  }

  /// Returns a mutable reference to the inner plugin (for parameter tweaks).
  pub fn plugin_mut(&mut self) -> &mut dyn DspPlugin {
    self.inner.as_mut()
  }
}

impl AudioEffect for PluginAdapter {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }

  fn name(&self) -> &str {
    self.inner.name()
  }
}

// ─── AudioEffect → DspPlugin adapter ────────────────────────────────

/// Adapter that exposes an [`AudioEffect`] as a parameter-less
/// [`DspPlugin`].
///
/// The plugin's `id` and `name` are taken from the wrapped effect's
/// `name()`. The parameter list is empty; `set_parameter` always errors.
pub struct EffectAsPlugin {
  inner: Box<dyn AudioEffect>,
  id: String,
}

impl EffectAsPlugin {
  /// Wraps an `AudioEffect` and assigns it the given plugin id (e.g.
  /// `"custom.fuzz"`). The plugin's display name comes from the
  /// effect's [`AudioEffect::name`].
  pub fn new(effect: Box<dyn AudioEffect>, id: impl Into<String>) -> Self {
    Self {
      inner: effect,
      id: id.into(),
    }
  }
}

impl DspPlugin for EffectAsPlugin {
  fn id(&self) -> &str {
    &self.id
  }
  fn name(&self) -> &str {
    self.inner.name()
  }
  fn parameters(&self) -> Vec<phonon_core::PluginParam> {
    Vec::new()
  }
  fn set_parameter(&mut self, name: &str, _value: f32) -> phonon_core::Result<()> {
    Err(phonon_core::PhononError::InvalidParam(format!(
      "{}: no adjustable parameters",
      name
    )))
  }
  fn get_parameter(&self, _name: &str) -> Option<f32> {
    None
  }
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }
  fn reset(&mut self) {}
}

// ─── Legacy built-in effects (deprecated) ───────────────────────────
//
// These types are retained as thin wrappers over the equivalent
// phonon-core plugins so call sites keep compiling. Prefer constructing
// the core plugin directly and pushing it via `add_plugin` /
// `EffectChain::push_plugin`.

/// Legacy volume effect — wraps [`VolumePlugin`].
#[deprecated(
  since = "0.7.0",
  note = "use phonon_core::VolumePlugin via AudioPlayer::add_plugin"
)]
pub struct VolumeEffect {
  inner: VolumePlugin,
}

#[allow(deprecated)]
impl VolumeEffect {
  pub fn new(gain: f32) -> Self {
    Self {
      inner: VolumePlugin::new(gain),
    }
  }
}

#[allow(deprecated)]
impl AudioEffect for VolumeEffect {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }
  fn name(&self) -> &str {
    "Volume"
  }
}

/// Legacy low-pass effect — wraps [`LowPassPlugin`].
#[deprecated(
  since = "0.7.0",
  note = "use phonon_core::LowPassPlugin via AudioPlayer::add_plugin"
)]
pub struct LowPassEffect {
  inner: LowPassPlugin,
}

#[allow(deprecated)]
impl LowPassEffect {
  pub fn new(cutoff_hz: f64) -> Self {
    Self {
      inner: LowPassPlugin::new(cutoff_hz as f32),
    }
  }
}

#[allow(deprecated)]
impl AudioEffect for LowPassEffect {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }
  fn name(&self) -> &str {
    "LowPass"
  }
}

/// Legacy high-pass effect — wraps [`HighPassPlugin`].
#[deprecated(
  since = "0.7.0",
  note = "use phonon_core::HighPassPlugin via AudioPlayer::add_plugin"
)]
pub struct HighPassEffect {
  inner: HighPassPlugin,
}

#[allow(deprecated)]
impl HighPassEffect {
  pub fn new(cutoff_hz: f64) -> Self {
    Self {
      inner: HighPassPlugin::new(cutoff_hz as f32),
    }
  }
}

#[allow(deprecated)]
impl AudioEffect for HighPassEffect {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }
  fn name(&self) -> &str {
    "HighPass"
  }
}

/// Legacy compressor effect — wraps [`CompressorPlugin`].
#[deprecated(
  since = "0.7.0",
  note = "use phonon_core::CompressorPlugin via AudioPlayer::add_plugin"
)]
pub struct CompressorEffect {
  inner: CompressorPlugin,
}

#[allow(deprecated)]
impl CompressorEffect {
  pub fn new(threshold: f32, ratio: f32, attack_ms: f32, release_ms: f32) -> Self {
    Self {
      inner: CompressorPlugin::new(threshold, ratio, attack_ms, release_ms),
    }
  }
}

#[allow(deprecated)]
impl AudioEffect for CompressorEffect {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    self.inner.process(samples, sample_rate, channels);
  }
  fn name(&self) -> &str {
    "Compressor"
  }
}

/// Legacy parametric EQ effect — wraps [`ParametricEqPlugin`] when the
/// caller wants a fixed-band EQ but does not need the named parameter
/// API; otherwise prefer [`ParametricEqPlugin`] directly.
///
/// Unlike the plugin (which uses fixed bands), this effect lets the
/// caller supply arbitrary [`EqBand`]s up-front.
#[deprecated(
  since = "0.7.0",
  note = "use phonon_core::ParametricEq directly inside a custom DspPlugin, or push the ParametricEqPlugin via AudioPlayer::add_plugin"
)]
pub struct ParametricEqEffect {
  bands: Vec<EqBand>,
  eq: Option<ParametricEq>,
  last_sample_rate: u32,
  last_channels: u16,
}

#[allow(deprecated)]
impl ParametricEqEffect {
  pub fn new(bands: Vec<EqBand>) -> Self {
    Self {
      bands,
      eq: None,
      last_sample_rate: 0,
      last_channels: 0,
    }
  }
}

#[allow(deprecated)]
impl AudioEffect for ParametricEqEffect {
  fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    if self.eq.is_none() || sample_rate != self.last_sample_rate || channels != self.last_channels {
      self.eq = Some(ParametricEq::new(self.bands.clone(), sample_rate, channels));
      self.last_sample_rate = sample_rate;
      self.last_channels = channels;
    }
    if let Some(eq) = &mut self.eq {
      eq.process(samples);
    }
  }
  fn name(&self) -> &str {
    "ParametricEQ"
  }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
  use super::*;

  #[test]
  fn test_volume_effect() {
    let mut effect = VolumeEffect::new(0.5);
    let mut samples = vec![1.0f32, -1.0, 0.5, -0.5];
    effect.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.5).abs() < 1e-6);
    assert!((samples[1] - (-0.5)).abs() < 1e-6);
  }

  #[test]
  fn test_lowpass_attenuates_high_freq() {
    let mut high_freq: Vec<f32> = (0..1024)
      .map(|i| {
        let t = i as f64 / 44100.0;
        (t * 10000.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();

    let original_rms: f64 = high_freq
      .iter()
      .map(|&s| (s as f64) * (s as f64))
      .sum::<f64>()
      / high_freq.len() as f64;

    let mut lp = LowPassEffect::new(500.0);
    lp.process(&mut high_freq, 44100, 1);

    let filtered_rms: f64 = high_freq
      .iter()
      .map(|&s| (s as f64) * (s as f64))
      .sum::<f64>()
      / high_freq.len() as f64;

    assert!(filtered_rms < original_rms * 0.1);
  }

  #[test]
  fn test_effect_chain() {
    let mut chain = EffectChain::new();
    chain.push(Box::new(VolumeEffect::new(2.0)));
    chain.push(Box::new(VolumeEffect::new(0.5)));

    let mut samples = vec![0.4f32, -0.4];
    chain.process(&mut samples, 44100, 1);

    assert!((samples[0] - 0.4).abs() < 1e-6);
    assert!((samples[1] - (-0.4)).abs() < 1e-6);
  }

  #[test]
  fn test_plugin_adapter_in_chain() {
    let mut chain = EffectChain::new();
    let plugin = phonon_core::VolumePlugin::new(0.5);
    chain.push_plugin(Box::new(plugin));

    let mut samples = vec![1.0f32, -1.0];
    chain.process(&mut samples, 44100, 1);

    assert!((samples[0] - 0.5).abs() < 1e-6);
    assert!((samples[1] - (-0.5)).abs() < 1e-6);
  }

  #[test]
  fn test_plugin_adapter_name() {
    let plugin = phonon_core::LowPassPlugin::new(1000.0);
    let adapter = PluginAdapter::new(Box::new(plugin));
    assert_eq!(adapter.name(), "Low-Pass Filter");
  }

  #[test]
  fn test_compressor_reduces_loud_signal() {
    let mut comp = CompressorEffect::new(0.3, 4.0, 1.0, 10.0);
    let mut samples = vec![0.8f32; 4410];
    comp.process(&mut samples, 44100, 1);

    let tail = &samples[2000..];
    let peak = tail.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak < 0.8, "Compressor should reduce peaks, got {peak}");
  }

  #[test]
  fn test_highpass_attenuates_low_freq() {
    let mut low_freq: Vec<f32> = (0..4410)
      .map(|i| {
        let t = i as f64 / 44100.0;
        (t * 50.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();

    let original_rms: f64 = low_freq
      .iter()
      .map(|&s| (s as f64) * (s as f64))
      .sum::<f64>()
      / low_freq.len() as f64;

    let mut hp = HighPassEffect::new(1000.0);
    hp.process(&mut low_freq, 44100, 1);

    let filtered_rms: f64 = low_freq
      .iter()
      .map(|&s| (s as f64) * (s as f64))
      .sum::<f64>()
      / low_freq.len() as f64;

    assert!(filtered_rms < original_rms * 0.3);
  }

  #[test]
  fn test_effect_as_plugin_processes() {
    // Wrap an AudioEffect as a DspPlugin and run it through the plugin API.
    let effect: Box<dyn AudioEffect> = Box::new(VolumeEffect::new(0.5));
    let mut plug = EffectAsPlugin::new(effect, "test.volume");
    assert_eq!(plug.id(), "test.volume");
    assert_eq!(plug.name(), "Volume");
    assert!(plug.parameters().is_empty());
    assert!(plug.set_parameter("gain", 0.0).is_err());

    let mut samples = vec![1.0_f32, -1.0];
    plug.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_plugin_adapter_round_trip() {
    // DspPlugin → PluginAdapter → AudioEffect → EffectAsPlugin → DspPlugin
    // should still process correctly.
    let core = phonon_core::VolumePlugin::new(2.0);
    let as_effect: Box<dyn AudioEffect> = Box::new(PluginAdapter::new(Box::new(core)));
    let mut as_plugin = EffectAsPlugin::new(as_effect, "round.trip");

    let mut samples = vec![0.25_f32, -0.25];
    as_plugin.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.5).abs() < 1e-6);
  }
}
