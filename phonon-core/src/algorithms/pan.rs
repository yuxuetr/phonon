//! Stereo panning utilities.
//!
//! Two operations:
//! - [`pan_mono_to_stereo`] — distribute a mono buffer across L/R using a
//!   constant-power pan law (-3 dB at center). The output is stereo.
//! - [`pan_stereo`] / [`pan_stereo_in_place`] — re-pan an existing stereo
//!   buffer, treating L+R as a mono mid signal then re-distributing.
//!
//! Constant-power panning preserves perceived loudness as a source moves
//! across the stereo field — a sine-cosine law is used (`cos(θ)` to L,
//! `sin(θ)` to R) so `L²+R² = 1` for all positions.

use crate::{AudioBuffer, PhononError, Result};
use std::f32::consts::FRAC_PI_2;

/// Compute (left_gain, right_gain) for a constant-power pan position.
///
/// `position`: -1.0 = full left, 0.0 = center, +1.0 = full right.
/// Values outside `[-1.0, 1.0]` are clamped.
fn constant_power_gains(position: f32) -> (f32, f32) {
  let p = position.clamp(-1.0, 1.0);
  // Map [-1, 1] to [0, π/2].
  let theta = (p + 1.0) * 0.5 * FRAC_PI_2;
  (theta.cos(), theta.sin())
}

/// Pan a mono buffer to stereo at the given position.
///
/// `position`: -1.0 (full left) to +1.0 (full right). Returns an error if
/// the input has more than one channel.
pub fn pan_mono_to_stereo(buffer: &AudioBuffer, position: f32) -> Result<AudioBuffer> {
  if buffer.channels != 1 {
    return Err(PhononError::ChannelMismatch {
      expected: 1,
      actual: buffer.channels,
    });
  }
  let (gl, gr) = constant_power_gains(position);
  let mut out = Vec::with_capacity(buffer.samples.len() * 2);
  for &s in &buffer.samples {
    out.push(s * gl);
    out.push(s * gr);
  }
  Ok(AudioBuffer {
    samples: out,
    sample_rate: buffer.sample_rate,
    channels: 2,
  })
}

/// Re-pan a stereo buffer in place: collapse to mid (L+R)/2 and redistribute.
///
/// Useful when the source is already stereo but you want to override its
/// pan position. Returns an error for non-stereo buffers.
pub fn pan_stereo_in_place(buffer: &mut AudioBuffer, position: f32) -> Result<()> {
  if buffer.channels != 2 {
    return Err(PhononError::ChannelMismatch {
      expected: 2,
      actual: buffer.channels,
    });
  }
  let (gl, gr) = constant_power_gains(position);
  for frame in buffer.samples.chunks_exact_mut(2) {
    let mid = (frame[0] + frame[1]) * 0.5;
    frame[0] = mid * gl;
    frame[1] = mid * gr;
  }
  Ok(())
}

/// Immutable variant of [`pan_stereo_in_place`].
pub fn pan_stereo(buffer: &AudioBuffer, position: f32) -> Result<AudioBuffer> {
  let mut out = buffer.clone();
  pan_stereo_in_place(&mut out, position)?;
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_gains_center_is_minus_3db() {
    let (l, r) = constant_power_gains(0.0);
    // Both channels should be sqrt(0.5) ≈ 0.7071.
    assert!((l - 0.7071068).abs() < 1e-5, "l = {}", l);
    assert!((r - 0.7071068).abs() < 1e-5, "r = {}", r);
    // Constant-power invariant.
    assert!(((l * l + r * r) - 1.0).abs() < 1e-5);
  }

  #[test]
  fn test_gains_full_left() {
    let (l, r) = constant_power_gains(-1.0);
    assert!((l - 1.0).abs() < 1e-5);
    assert!(r.abs() < 1e-5);
  }

  #[test]
  fn test_gains_full_right() {
    let (l, r) = constant_power_gains(1.0);
    assert!(l.abs() < 1e-5);
    assert!((r - 1.0).abs() < 1e-5);
  }

  #[test]
  fn test_gains_constant_power_invariant() {
    for &p in &[-1.0, -0.7, -0.3, 0.0, 0.2, 0.5, 0.9, 1.0_f32] {
      let (l, r) = constant_power_gains(p);
      let total = l * l + r * r;
      assert!((total - 1.0).abs() < 1e-5, "total power at p={}: {}", p, total);
    }
  }

  #[test]
  fn test_gains_clamp_out_of_range() {
    let (l_a, r_a) = constant_power_gains(-2.0);
    let (l_b, r_b) = constant_power_gains(-1.0);
    assert!((l_a - l_b).abs() < 1e-5);
    assert!((r_a - r_b).abs() < 1e-5);

    let (l_c, r_c) = constant_power_gains(2.0);
    let (l_d, r_d) = constant_power_gains(1.0);
    assert!((l_c - l_d).abs() < 1e-5);
    assert!((r_c - r_d).abs() < 1e-5);
  }

  #[test]
  fn test_pan_mono_to_stereo_center() -> Result<()> {
    let mono = AudioBuffer {
      samples: vec![1.0, 0.5, -0.3],
      sample_rate: 44100,
      channels: 1,
    };
    let stereo = pan_mono_to_stereo(&mono, 0.0)?;
    assert_eq!(stereo.channels, 2);
    assert_eq!(stereo.samples.len(), 6);
    // First frame: L=R=1.0 * sqrt(0.5)
    assert!((stereo.samples[0] - 0.7071068).abs() < 1e-5);
    assert!((stereo.samples[1] - 0.7071068).abs() < 1e-5);
    Ok(())
  }

  #[test]
  fn test_pan_mono_to_stereo_full_left() -> Result<()> {
    let mono = AudioBuffer {
      samples: vec![1.0, -0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let stereo = pan_mono_to_stereo(&mono, -1.0)?;
    assert!((stereo.samples[0] - 1.0).abs() < 1e-5); // L
    assert!(stereo.samples[1].abs() < 1e-5); // R
    assert!((stereo.samples[2] - (-0.5)).abs() < 1e-5);
    assert!(stereo.samples[3].abs() < 1e-5);
    Ok(())
  }

  #[test]
  fn test_pan_mono_to_stereo_rejects_stereo_input() {
    let stereo = AudioBuffer {
      samples: vec![0.0; 4],
      sample_rate: 44100,
      channels: 2,
    };
    assert!(pan_mono_to_stereo(&stereo, 0.0).is_err());
  }

  #[test]
  fn test_pan_stereo_in_place_collapses_to_mid() -> Result<()> {
    let mut buf = AudioBuffer {
      samples: vec![1.0, -1.0, 0.5, 0.5],
      sample_rate: 44100,
      channels: 2,
    };
    // Pan to center: each frame becomes mid * sqrt(0.5).
    pan_stereo_in_place(&mut buf, 0.0)?;
    // Frame 0: mid = (1.0 + -1.0)/2 = 0.0; output L=R=0.
    assert!(buf.samples[0].abs() < 1e-5);
    assert!(buf.samples[1].abs() < 1e-5);
    // Frame 1: mid = 0.5; L=R=0.5*sqrt(0.5) ≈ 0.3535.
    assert!((buf.samples[2] - 0.3535534).abs() < 1e-5);
    assert!((buf.samples[3] - 0.3535534).abs() < 1e-5);
    Ok(())
  }

  #[test]
  fn test_pan_stereo_rejects_mono_input() {
    let mono = AudioBuffer {
      samples: vec![0.0; 4],
      sample_rate: 44100,
      channels: 1,
    };
    assert!(pan_stereo(&mono, 0.0).is_err());
  }

  #[test]
  fn test_pan_stereo_immutable() -> Result<()> {
    let buf = AudioBuffer {
      samples: vec![1.0, 0.0, 0.0, 1.0],
      sample_rate: 44100,
      channels: 2,
    };
    let original = buf.samples.clone();
    let _panned = pan_stereo(&buf, -1.0)?;
    assert_eq!(buf.samples, original, "input must not change");
    Ok(())
  }
}
