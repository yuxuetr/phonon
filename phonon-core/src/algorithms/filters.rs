use crate::{AudioBuffer, Result};
use std::f64::consts::PI;

/// Filter types available for Biquad filter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
  LowPass,
  HighPass,
  BandPass,
  Notch,
  Peaking,
  LowShelf,
  HighShelf,
}

/// Biquad filter coefficients.
#[derive(Debug, Clone)]
pub(crate) struct Coefficients {
  pub(crate) b0: f64,
  pub(crate) b1: f64,
  pub(crate) b2: f64,
  pub(crate) a1: f64,
  pub(crate) a2: f64,
}

/// Applies a Biquad filter to the audio buffer.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to filter (in-place).
/// * `filter_type` - The type of filter to apply.
/// * `cutoff_hz` - The cutoff or center frequency in Hz.
/// * `q_value` - The Q factor (quality factor). Controls bandwidth/resonance.
/// * `gain_db` - The gain in Decibels (used for Peaking, LowShelf, HighShelf).
///
/// # Returns
///
/// None. Modifies the buffer in place.
pub fn filter(
  buffer: &mut AudioBuffer,
  filter_type: FilterType,
  cutoff_hz: f64,
  q_value: f64,
  gain_db: f64,
) -> Result<()> {
  if buffer.sample_rate == 0 {
    return Ok(());
  }

  let sample_rate = buffer.sample_rate as f64;
  let coeffs = calculate_coefficients(filter_type, cutoff_hz, sample_rate, q_value, gain_db);

  // Apply filter per channel
  // Since channels are interleaved, we need state for each channel.
  let channels = buffer.channels as usize;
  if channels == 0 {
    return Ok(());
  }

  let mut x1 = vec![0.0; channels]; // x[n-1]
  let mut x2 = vec![0.0; channels]; // x[n-2]
  let mut y1 = vec![0.0; channels]; // y[n-1]
  let mut y2 = vec![0.0; channels]; // y[n-2]

  let b0 = coeffs.b0;
  let b1 = coeffs.b1;
  let b2 = coeffs.b2;
  let a1 = coeffs.a1;
  let a2 = coeffs.a2;

  for i in 0..(buffer.samples.len() / channels) {
    for ch in 0..channels {
      let idx = i * channels + ch;
      let x0 = buffer.samples[idx] as f64;

      // Direct Form I difference equation:
      // y[n] = b0*x[n] + b1*x[n-1] + b2*x[n-2] - a1*y[n-1] - a2*y[n-2]
      let y0 = b0 * x0 + b1 * x1[ch] + b2 * x2[ch] - a1 * y1[ch] - a2 * y2[ch];

      // Update states
      x2[ch] = x1[ch];
      x1[ch] = x0;
      y2[ch] = y1[ch];
      y1[ch] = y0;

      buffer.samples[idx] = y0 as f32;
    }
  }

  Ok(())
}

pub(crate) fn calculate_coefficients(
  filter_type: FilterType,
  cutoff_hz: f64,
  sample_rate: f64,
  q: f64,
  gain_db: f64,
) -> Coefficients {
  let w0 = 2.0 * PI * cutoff_hz / sample_rate;
  let cos_w0 = w0.cos();
  let sin_w0 = w0.sin();
  let alpha = sin_w0 / (2.0 * q);

  // Convert gain from dB to linear
  let a = 10.0_f64.powf(gain_db / 40.0); // For shelf/peaking. Audio EQ Cookbook uses A = 10^(dB/40)

  let (b0, b1, b2, a0, a1, a2) = match filter_type {
    FilterType::LowPass => (
      (1.0 - cos_w0) / 2.0,
      1.0 - cos_w0,
      (1.0 - cos_w0) / 2.0,
      1.0 + alpha,
      -2.0 * cos_w0,
      1.0 - alpha,
    ),
    FilterType::HighPass => (
      (1.0 + cos_w0) / 2.0,
      -(1.0 + cos_w0),
      (1.0 + cos_w0) / 2.0,
      1.0 + alpha,
      -2.0 * cos_w0,
      1.0 - alpha,
    ),
    FilterType::BandPass => {
      // Constant Skirt Gain, peak gain = Q
      (
        sin_w0 / 2.0,
        0.0,
        -sin_w0 / 2.0,
        1.0 + alpha,
        -2.0 * cos_w0,
        1.0 - alpha,
      )
    }
    FilterType::Notch => (
      1.0,
      -2.0 * cos_w0,
      1.0,
      1.0 + alpha,
      -2.0 * cos_w0,
      1.0 - alpha,
    ),
    FilterType::Peaking => (
      1.0 + alpha * a,
      -2.0 * cos_w0,
      1.0 - alpha * a,
      1.0 + alpha / a,
      -2.0 * cos_w0,
      1.0 - alpha / a,
    ),
    FilterType::LowShelf => {
      let sqrt_a = a.sqrt();
      (
        a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha),
        2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
        a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha),
        (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha,
        -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
        (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha,
      )
    }
    FilterType::HighShelf => {
      let sqrt_a = a.sqrt();
      (
        a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha),
        -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
        a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha),
        (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha,
        2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
        (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha,
      )
    }
  };

  // Normalize coefficients by a0
  Coefficients {
    b0: b0 / a0,
    b1: b1 / a0,
    b2: b2 / a0,
    a1: a1 / a0,
    a2: a2 / a0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_low_pass_filter() {
    // Create a buffer with high frequency noise (Nyquist/2) and DC
    let sample_rate = 44100;
    let mut buffer = AudioBuffer {
      samples: vec![0.0; 100],
      sample_rate,
      channels: 1,
    };

    // Add DC (1.0) and Nyquist (1.0 * (-1)^n)
    for i in 0..100 {
      buffer.samples[i] = 1.0 + if i % 2 == 0 { 0.5 } else { -0.5 };
    }

    // Apply Low Pass at 1000Hz (well below Nyquist 22050Hz)
    filter(&mut buffer, FilterType::LowPass, 1000.0, 0.707, 0.0).unwrap();

    // The high freq component should be attenuated heavily.
    // The DC component (1.0) should pass through (mostly).

    // Check middle samples (to avoid startup transient)
    let mid = 50;
    let val = buffer.samples[mid];

    // DC gain is 1.0 for LPF.
    // High freq gain is ~0.
    // So expected value is around 1.0
    assert!(
      (val - 1.0).abs() < 0.1,
      "Value {} is not close to DC 1.0",
      val
    );

    // Ensure oscillation is reduced
    let val_next = buffer.samples[mid + 1];
    assert!(
      (val - val_next).abs() < 0.1,
      "Oscillation {} vs {} is too high",
      val,
      val_next
    );
  }
}
