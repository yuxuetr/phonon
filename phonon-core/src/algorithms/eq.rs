//! Multi-band parametric equalizer.
//!
//! Built on top of the biquad filter infrastructure. Each band is an independent
//! biquad filter with its own type, frequency, Q, and gain. Bands are applied
//! in series (cascaded).

use crate::AudioBuffer;
use crate::algorithms::filters::{Coefficients, FilterType, calculate_coefficients};

/// A single EQ band definition.
#[derive(Debug, Clone)]
pub struct EqBand {
  /// Filter type for this band.
  pub filter_type: FilterType,
  /// Center/cutoff frequency in Hz.
  pub freq_hz: f64,
  /// Q factor (quality / bandwidth).
  pub q: f64,
  /// Gain in dB (used for Peaking, LowShelf, HighShelf).
  pub gain_db: f64,
}

impl EqBand {
  /// Creates a new EQ band.
  pub fn new(filter_type: FilterType, freq_hz: f64, q: f64, gain_db: f64) -> Self {
    Self {
      filter_type,
      freq_hz,
      q,
      gain_db,
    }
  }

  /// Low-shelf bass boost: +6 dB at 100 Hz.
  pub fn bass_boost() -> Self {
    Self::new(FilterType::LowShelf, 100.0, 0.707, 6.0)
  }

  /// High-shelf treble cut: -4 dB at 8 kHz.
  pub fn treble_cut() -> Self {
    Self::new(FilterType::HighShelf, 8000.0, 0.707, -4.0)
  }

  /// Peaking presence boost for voice: +3 dB at 3 kHz, Q=1.5.
  pub fn voice_presence() -> Self {
    Self::new(FilterType::Peaking, 3000.0, 1.5, 3.0)
  }

  /// High-pass rumble filter: remove below 80 Hz.
  pub fn rumble_filter() -> Self {
    Self::new(FilterType::HighPass, 80.0, 0.707, 0.0)
  }

  /// De-ess: notch at 7 kHz to reduce sibilance.
  pub fn de_ess() -> Self {
    Self::new(FilterType::Peaking, 7000.0, 2.0, -5.0)
  }
}

/// Per-band biquad processing state (delay line per channel).
struct BandState {
  coeffs: Coefficients,
  x1: Vec<f64>,
  x2: Vec<f64>,
  y1: Vec<f64>,
  y2: Vec<f64>,
}

impl BandState {
  fn new(coeffs: Coefficients, channels: usize) -> Self {
    Self {
      coeffs,
      x1: vec![0.0; channels],
      x2: vec![0.0; channels],
      y1: vec![0.0; channels],
      y2: vec![0.0; channels],
    }
  }

  /// Process one sample for a given channel, returning the filtered value.
  #[inline]
  fn process_sample(&mut self, x0: f64, ch: usize) -> f64 {
    let y0 = self.coeffs.b0 * x0 + self.coeffs.b1 * self.x1[ch] + self.coeffs.b2 * self.x2[ch]
      - self.coeffs.a1 * self.y1[ch]
      - self.coeffs.a2 * self.y2[ch];

    self.x2[ch] = self.x1[ch];
    self.x1[ch] = x0;
    self.y2[ch] = self.y1[ch];
    self.y1[ch] = y0;

    y0
  }
}

/// Multi-band parametric equalizer.
///
/// Applies multiple biquad filter bands in series. Each band has its own
/// coefficients and per-channel delay state.
///
/// # Example
///
/// ```
/// use phonon_core::algorithms::eq::{EqBand, ParametricEq};
///
/// let bands = vec![EqBand::bass_boost(), EqBand::voice_presence()];
/// let mut eq = ParametricEq::new(bands, 44100, 1);
/// let mut samples = vec![0.5f32; 4410];
/// eq.process(&mut samples);
/// ```
pub struct ParametricEq {
  bands: Vec<BandState>,
  channels: usize,
}

impl ParametricEq {
  /// Creates a new parametric EQ with the given bands.
  ///
  /// Pre-computes biquad coefficients for each band at the specified sample rate.
  pub fn new(bands: Vec<EqBand>, sample_rate: u32, channels: u16) -> Self {
    let ch = channels as usize;
    let sr = sample_rate as f64;
    let band_states = bands
      .iter()
      .map(|band| {
        let coeffs =
          calculate_coefficients(band.filter_type, band.freq_hz, sr, band.q, band.gain_db);
        BandState::new(coeffs, ch)
      })
      .collect();

    Self {
      bands: band_states,
      channels: ch,
    }
  }

  /// Processes interleaved audio samples in-place through all EQ bands.
  pub fn process(&mut self, samples: &mut [f32]) {
    let ch = self.channels;
    if ch == 0 || samples.is_empty() {
      return;
    }

    let frames = samples.len() / ch;

    for i in 0..frames {
      for c in 0..ch {
        let idx = i * ch + c;
        let mut val = samples[idx] as f64;

        for band in &mut self.bands {
          val = band.process_sample(val, c);
        }

        samples[idx] = val as f32;
      }
    }
  }

  /// Resets all band delay states to zero.
  pub fn reset(&mut self) {
    for band in &mut self.bands {
      band.x1.fill(0.0);
      band.x2.fill(0.0);
      band.y1.fill(0.0);
      band.y2.fill(0.0);
    }
  }

  /// Returns the number of bands.
  pub fn band_count(&self) -> usize {
    self.bands.len()
  }
}

/// Applies a parametric EQ to an `AudioBuffer` in-place.
///
/// Convenience function that creates a `ParametricEq`, processes the buffer,
/// and discards the state.
///
/// # Example
///
/// ```
/// use phonon_core::AudioBuffer;
/// use phonon_core::algorithms::eq::{EqBand, apply_eq};
///
/// let mut buffer = AudioBuffer {
///     samples: vec![0.5; 44100],
///     sample_rate: 44100,
///     channels: 1,
/// };
/// let bands = vec![EqBand::bass_boost()];
/// apply_eq(&mut buffer, &bands);
/// ```
pub fn apply_eq(buffer: &mut AudioBuffer, bands: &[EqBand]) {
  if bands.is_empty() || buffer.samples.is_empty() {
    return;
  }
  let mut eq = ParametricEq::new(bands.to_vec(), buffer.sample_rate, buffer.channels);
  eq.process(&mut buffer.samples);
}

/// Returns a new buffer with the EQ applied (non-mutating variant).
pub fn with_eq(buffer: &AudioBuffer, bands: &[EqBand]) -> AudioBuffer {
  let mut result = buffer.clone();
  apply_eq(&mut result, bands);
  result
}

/// Common EQ presets returning a `Vec<EqBand>`.
pub mod presets {
  use super::EqBand;
  use crate::algorithms::filters::FilterType;

  /// Flat EQ — no bands, pass-through.
  pub fn flat() -> Vec<EqBand> {
    vec![]
  }

  /// Voice enhancement: rumble filter + presence boost + de-ess.
  pub fn voice() -> Vec<EqBand> {
    vec![
      EqBand::rumble_filter(),
      EqBand::voice_presence(),
      EqBand::de_ess(),
    ]
  }

  /// Bass-heavy: low shelf boost + slight high cut.
  pub fn bass_heavy() -> Vec<EqBand> {
    vec![
      EqBand::new(FilterType::LowShelf, 120.0, 0.707, 8.0),
      EqBand::new(FilterType::Peaking, 250.0, 1.0, 3.0),
      EqBand::new(FilterType::HighShelf, 6000.0, 0.707, -3.0),
    ]
  }

  /// Bright/treble boost: slight bass cut + high shelf boost.
  pub fn bright() -> Vec<EqBand> {
    vec![
      EqBand::new(FilterType::LowShelf, 200.0, 0.707, -2.0),
      EqBand::new(FilterType::Peaking, 4000.0, 1.2, 3.0),
      EqBand::new(FilterType::HighShelf, 10000.0, 0.707, 5.0),
    ]
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::AudioBuffer;

  /// Generate a pure tone at the given frequency.
  fn make_tone(freq_hz: f64, duration_secs: f64, sample_rate: u32) -> Vec<f32> {
    let n = (duration_secs * sample_rate as f64) as usize;
    (0..n)
      .map(|i| {
        let t = i as f64 / sample_rate as f64;
        (t * freq_hz * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect()
  }

  fn rms(samples: &[f32]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / samples.len() as f64).sqrt()
  }

  #[test]
  fn test_eq_band_presets() {
    let bass = EqBand::bass_boost();
    assert_eq!(bass.filter_type, FilterType::LowShelf);
    assert!((bass.freq_hz - 100.0).abs() < 1e-6);

    let treble = EqBand::treble_cut();
    assert_eq!(treble.filter_type, FilterType::HighShelf);
    assert!(treble.gain_db < 0.0);

    let voice = EqBand::voice_presence();
    assert_eq!(voice.filter_type, FilterType::Peaking);
  }

  #[test]
  fn test_parametric_eq_lowpass_attenuates_high_freq() {
    let sr = 44100;
    // Mix 100 Hz + 10 kHz
    let low: Vec<f32> = make_tone(100.0, 0.5, sr);
    let high: Vec<f32> = make_tone(10000.0, 0.5, sr);
    let mixed: Vec<f32> = low.iter().zip(&high).map(|(a, b)| a + b).collect();

    let mut buffer = AudioBuffer {
      samples: mixed,
      sample_rate: sr,
      channels: 1,
    };

    let bands = vec![EqBand::new(FilterType::LowPass, 500.0, 0.707, 0.0)];
    apply_eq(&mut buffer, &bands);

    // After LP at 500 Hz, the 10 kHz component should be heavily attenuated.
    // The buffer should mainly contain the 100 Hz tone.
    // Check that RMS is roughly half of original (lost the high component).
    let output_rms = rms(&buffer.samples);
    // A single sine at amplitude 1.0 has RMS ≈ 0.707
    // We expect output to be close to just the 100 Hz tone's RMS
    assert!(
      output_rms < 0.8,
      "Expected RMS < 0.8 after LP, got {output_rms:.4}"
    );
    assert!(
      output_rms > 0.3,
      "Expected RMS > 0.3 (100 Hz passes), got {output_rms:.4}"
    );
  }

  #[test]
  fn test_parametric_eq_bass_boost() {
    let sr = 44100;
    let samples = make_tone(80.0, 0.5, sr);
    let original_rms = rms(&samples);

    let mut buffer = AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    };

    apply_eq(&mut buffer, &[EqBand::bass_boost()]);

    let boosted_rms = rms(&buffer.samples);
    assert!(
      boosted_rms > original_rms,
      "Bass boost should increase 80 Hz RMS: {original_rms:.4} -> {boosted_rms:.4}"
    );
  }

  #[test]
  fn test_parametric_eq_multi_band() {
    let sr = 44100;
    let samples = make_tone(440.0, 0.5, sr);

    let mut buffer = AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    };

    // Apply 3-band EQ
    let bands = vec![
      EqBand::new(FilterType::LowShelf, 200.0, 0.707, -3.0),
      EqBand::new(FilterType::Peaking, 440.0, 2.0, 6.0),
      EqBand::new(FilterType::HighShelf, 8000.0, 0.707, -2.0),
    ];
    apply_eq(&mut buffer, &bands);

    // 440 Hz should be boosted by the peaking band
    let boosted_rms = rms(&buffer.samples);
    // Original sine RMS ≈ 0.707; with +6 dB peaking, should be higher
    assert!(
      boosted_rms > 0.8,
      "440 Hz with +6 dB peaking should have RMS > 0.8, got {boosted_rms:.4}"
    );
  }

  #[test]
  fn test_parametric_eq_stereo() {
    let sr = 44100;
    let mono = make_tone(1000.0, 0.2, sr);
    // Interleave: L=tone, R=tone
    let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();

    let mut buffer = AudioBuffer {
      samples: stereo,
      sample_rate: sr,
      channels: 2,
    };

    let bands = vec![EqBand::new(FilterType::Peaking, 1000.0, 1.0, 6.0)];
    apply_eq(&mut buffer, &bands);

    // Check both channels are affected equally
    let left_rms = rms(
      &buffer
        .samples
        .iter()
        .step_by(2)
        .copied()
        .collect::<Vec<_>>(),
    );
    let right_rms = rms(
      &buffer
        .samples
        .iter()
        .skip(1)
        .step_by(2)
        .copied()
        .collect::<Vec<_>>(),
    );
    assert!(
      (left_rms - right_rms).abs() < 0.01,
      "L/R should be equal: L={left_rms:.4}, R={right_rms:.4}"
    );
    assert!(left_rms > 0.8, "Boosted 1 kHz should have high RMS");
  }

  #[test]
  fn test_apply_eq_empty_bands_noop() {
    let samples = vec![0.5f32; 100];
    let mut buffer = AudioBuffer {
      samples: samples.clone(),
      sample_rate: 44100,
      channels: 1,
    };
    apply_eq(&mut buffer, &[]);
    assert_eq!(buffer.samples, samples);
  }

  #[test]
  fn test_apply_eq_empty_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    apply_eq(&mut buffer, &[EqBand::bass_boost()]);
    assert!(buffer.samples.is_empty());
  }

  #[test]
  fn test_with_eq_immutable() {
    let buffer = AudioBuffer {
      samples: make_tone(440.0, 0.1, 44100),
      sample_rate: 44100,
      channels: 1,
    };
    let original_rms = rms(&buffer.samples);

    let processed = with_eq(
      &buffer,
      &[EqBand::new(FilterType::Peaking, 440.0, 1.0, 12.0)],
    );

    // Original unchanged
    let after_rms = rms(&buffer.samples);
    assert!((original_rms - after_rms).abs() < 1e-6);

    // Processed is different
    let proc_rms = rms(&processed.samples);
    assert!(proc_rms > original_rms);
  }

  #[test]
  fn test_presets_voice() {
    let bands = presets::voice();
    assert_eq!(bands.len(), 3);
    // First band should be HP (rumble filter)
    assert_eq!(bands[0].filter_type, FilterType::HighPass);
  }

  #[test]
  fn test_presets_flat_is_empty() {
    assert!(presets::flat().is_empty());
  }

  #[test]
  fn test_parametric_eq_reset() {
    let sr = 44100;
    let mut eq = ParametricEq::new(vec![EqBand::bass_boost()], sr, 1);

    // Process some data
    let mut samples = make_tone(100.0, 0.1, sr);
    eq.process(&mut samples);

    // Reset state
    eq.reset();
    assert_eq!(eq.band_count(), 1);

    // Process again — should behave as if freshly created
    let mut samples2 = make_tone(100.0, 0.1, sr);
    let mut eq2 = ParametricEq::new(vec![EqBand::bass_boost()], sr, 1);
    eq.process(&mut samples2);

    let mut samples3 = make_tone(100.0, 0.1, sr);
    eq2.process(&mut samples3);

    // After reset, processing should match a fresh EQ
    let diff: f32 = samples2
      .iter()
      .zip(&samples3)
      .map(|(a, b)| (a - b).abs())
      .fold(0.0f32, f32::max);
    assert!(
      diff < 1e-6,
      "Reset EQ should match fresh EQ, max diff={diff}"
    );
  }
}
