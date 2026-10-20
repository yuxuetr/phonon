//! Real-time effect chain for playback.
//!
//! Effects are [`phonon_core::DspPlugin`]s — the same trait used by offline
//! processing, the CLI and the Studio plugin browser — applied in order to
//! each block the audio callback renders.

use phonon_core::DspPlugin;

/// A chain of plugins applied sequentially.
#[derive(Default)]
pub struct EffectChain {
  plugins: Vec<Box<dyn DspPlugin>>,
}

impl EffectChain {
  /// Creates an empty effect chain.
  pub fn new() -> Self {
    Self::default()
  }

  /// Adds a plugin to the end of the chain.
  pub fn push(&mut self, plugin: Box<dyn DspPlugin>) {
    self.plugins.push(plugin);
  }

  /// Removes all plugins from the chain.
  pub fn clear(&mut self) {
    self.plugins.clear();
  }

  /// Returns the number of plugins in the chain.
  pub fn len(&self) -> usize {
    self.plugins.len()
  }

  /// Returns `true` if the chain has no plugins.
  pub fn is_empty(&self) -> bool {
    self.plugins.is_empty()
  }

  /// Processes interleaved samples through all plugins in order.
  pub fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
    for plugin in &mut self.plugins {
      plugin.process(samples, sample_rate, channels);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_core::{LowPassPlugin, VolumePlugin};

  #[test]
  fn test_chain_applies_plugins_in_order() {
    let mut chain = EffectChain::new();
    chain.push(Box::new(VolumePlugin::new(2.0)));
    chain.push(Box::new(VolumePlugin::new(0.25)));
    assert_eq!(chain.len(), 2);

    let mut samples = vec![0.4f32, -0.4];
    chain.process(&mut samples, 44100, 1);
    assert!((samples[0] - 0.2).abs() < 1e-6);
    assert!((samples[1] + 0.2).abs() < 1e-6);
  }

  #[test]
  fn test_chain_lowpass_attenuates_high_freq() {
    let mut samples: Vec<f32> = (0..1024)
      .map(|i| (i as f64 / 44100.0 * 10_000.0 * 2.0 * std::f64::consts::PI).sin() as f32)
      .collect();
    let energy = |s: &[f32]| s.iter().map(|&x| (x as f64).powi(2)).sum::<f64>();
    let before = energy(&samples);

    let mut chain = EffectChain::new();
    chain.push(Box::new(LowPassPlugin::new(500.0)));
    chain.process(&mut samples, 44100, 1);
    assert!(energy(&samples) < before * 0.1);
  }

  #[test]
  fn test_clear_empties_chain() {
    let mut chain = EffectChain::new();
    chain.push(Box::new(VolumePlugin::new(0.5)));
    chain.clear();
    assert!(chain.is_empty());
  }
}
