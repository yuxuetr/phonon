//! SIMD-accelerated helpers for hot audio DSP operations.
//!
//! Provides 4-wide SIMD implementations (NEON on aarch64, SSE on x86_64)
//! with scalar fallback for remainder elements — and for the whole slice on
//! other architectures. Only compiled when the `simd` feature is enabled.

/// SIMD lane width (4 × f32 for both NEON and SSE).
const LANES: usize = 4;

/// Index where the scalar loop takes over: after the 4-wide chunks on
/// architectures with a SIMD path, or 0 (the whole slice) elsewhere.
const fn scalar_start(len: usize) -> usize {
  if cfg!(any(target_arch = "aarch64", target_arch = "x86_64")) {
    len - len % LANES
  } else {
    0
  }
}

// ─── Multiply + clamp ──────────────────────────────────────────────
// Used by: adjust_volume, normalize (scale phase)

/// Multiplies every sample by `gain` and clamps to `[-1.0, 1.0]`.
pub(crate) fn simd_mul_clamp(samples: &mut [f32], gain: f32) {
  let len = samples.len();
  let chunks = len / LANES;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    // SAFETY: aarch64 NEON is always available on Apple Silicon / ARMv8+.
    unsafe {
      let vgain = vdupq_n_f32(gain);
      let vmin = vdupq_n_f32(-1.0);
      let vmax = vdupq_n_f32(1.0);

      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = vld1q_f32(ptr.add(offset));
        let product = vmulq_f32(v, vgain);
        let clamped = vminq_f32(vmaxq_f32(product, vmin), vmax);
        vst1q_f32(ptr.add(offset), clamped);
      }
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    // SAFETY: SSE is baseline on x86_64.
    unsafe {
      let vgain = _mm_set1_ps(gain);
      let vmin = _mm_set1_ps(-1.0);
      let vmax = _mm_set1_ps(1.0);

      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = _mm_loadu_ps(ptr.add(offset));
        let product = _mm_mul_ps(v, vgain);
        let clamped = _mm_min_ps(_mm_max_ps(product, vmin), vmax);
        _mm_storeu_ps(ptr.add(offset), clamped);
      }
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks; // suppress unused warning

  // Scalar remainder
  let start = scalar_start(len);
  for s in &mut samples[start..] {
    *s = (*s * gain).clamp(-1.0, 1.0);
  }
}

// ─── Multiply (no clamp) ───────────────────────────────────────────
// Used by: normalize (scale phase where clamp is not needed)

/// Multiplies every sample by `gain` without clamping.
pub(crate) fn simd_mul(samples: &mut [f32], gain: f32) {
  let len = samples.len();
  let chunks = len / LANES;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let vgain = vdupq_n_f32(gain);
      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = vld1q_f32(ptr.add(offset));
        vst1q_f32(ptr.add(offset), vmulq_f32(v, vgain));
      }
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      let vgain = _mm_set1_ps(gain);
      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = _mm_loadu_ps(ptr.add(offset));
        _mm_storeu_ps(ptr.add(offset), _mm_mul_ps(v, vgain));
      }
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  let start = scalar_start(len);
  for s in &mut samples[start..] {
    *s *= gain;
  }
}

// ─── Add + clamp ───────────────────────────────────────────────────
// Used by: mix

/// Element-wise `a[i] + b[i]`, clamped to `[-1.0, 1.0]`, written to `out`.
///
/// `a`, `b`, and `out` must all have the same length.
pub(crate) fn simd_add_clamp(a: &[f32], b: &[f32], out: &mut [f32]) {
  debug_assert_eq!(a.len(), b.len());
  debug_assert_eq!(a.len(), out.len());

  let len = a.len();
  let chunks = len / LANES;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let vmin = vdupq_n_f32(-1.0);
      let vmax = vdupq_n_f32(1.0);
      let pa = a.as_ptr();
      let pb = b.as_ptr();
      let po = out.as_mut_ptr();
      for i in 0..chunks {
        let off = i * LANES;
        let va = vld1q_f32(pa.add(off));
        let vb = vld1q_f32(pb.add(off));
        let sum = vaddq_f32(va, vb);
        let clamped = vminq_f32(vmaxq_f32(sum, vmin), vmax);
        vst1q_f32(po.add(off), clamped);
      }
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      let vmin = _mm_set1_ps(-1.0);
      let vmax = _mm_set1_ps(1.0);
      let pa = a.as_ptr();
      let pb = b.as_ptr();
      let po = out.as_mut_ptr();
      for i in 0..chunks {
        let off = i * LANES;
        let va = _mm_loadu_ps(pa.add(off));
        let vb = _mm_loadu_ps(pb.add(off));
        let sum = _mm_add_ps(va, vb);
        let clamped = _mm_min_ps(_mm_max_ps(sum, vmin), vmax);
        _mm_storeu_ps(po.add(off), clamped);
      }
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  let start = scalar_start(len);
  for i in start..len {
    out[i] = (a[i] + b[i]).clamp(-1.0, 1.0);
  }
}

// ─── Max absolute value ────────────────────────────────────────────
// Used by: normalize (peak-find phase)

/// Returns the maximum absolute value in `samples`.
pub(crate) fn simd_max_abs(samples: &[f32]) -> f32 {
  let len = samples.len();
  let chunks = len / LANES;
  // Set by the SIMD block on aarch64/x86_64; elsewhere the scalar loop
  // covers the whole slice starting from 0.
  #[allow(unused_assignments)]
  let mut global_max = 0.0f32;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let mut vmax_acc = vdupq_n_f32(0.0);
      let ptr = samples.as_ptr();
      for i in 0..chunks {
        let v = vld1q_f32(ptr.add(i * LANES));
        let vabs = vabsq_f32(v);
        vmax_acc = vmaxq_f32(vmax_acc, vabs);
      }
      // Horizontal max across 4 lanes.
      global_max = vmaxvq_f32(vmax_acc);
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      // Mask to clear the sign bit: abs via AND.
      let abs_mask = _mm_castsi128_ps(_mm_set1_epi32(0x7FFF_FFFFu32 as i32));
      let mut vmax_acc = _mm_setzero_ps();
      let ptr = samples.as_ptr();
      for i in 0..chunks {
        let v = _mm_loadu_ps(ptr.add(i * LANES));
        let vabs = _mm_and_ps(v, abs_mask);
        vmax_acc = _mm_max_ps(vmax_acc, vabs);
      }
      // Horizontal max: reduce 4 lanes to scalar.
      let shuf1 = _mm_movehdup_ps(vmax_acc); // [1,1,3,3]
      let max1 = _mm_max_ps(vmax_acc, shuf1);
      let shuf2 = _mm_movehl_ps(max1, max1);
      let max2 = _mm_max_ss(max1, shuf2);
      global_max = _mm_cvtss_f32(max2);
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  // Scalar remainder
  let start = scalar_start(len);
  for &s in &samples[start..] {
    let a = s.abs();
    if a > global_max {
      global_max = a;
    }
  }

  global_max
}

// ─── Sum of squares (f64 accumulator) ─────────────────────────────
// Used by: loudness::rms, LUFS K-weighted block energy.

/// Returns Σ (samples[i] as f64)².
///
/// Accumulates in `f64` for numerical stability across long buffers.
pub(crate) fn simd_sum_sq(samples: &[f32]) -> f64 {
  let len = samples.len();
  let chunks = len / LANES;
  // Set by the SIMD block on aarch64/x86_64; elsewhere the scalar loop
  // covers the whole slice starting from 0.
  #[allow(unused_assignments)]
  let mut sum: f64 = 0.0;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let mut acc = vdupq_n_f64(0.0);
      let ptr = samples.as_ptr();
      for i in 0..chunks {
        let v = vld1q_f32(ptr.add(i * LANES));
        // Promote each f32 lane to f64 for the multiply, accumulating
        // separately on the low and high halves to keep precision.
        let lo = vcvt_f64_f32(vget_low_f32(v));
        let hi = vcvt_high_f64_f32(v);
        acc = vfmaq_f64(acc, lo, lo);
        acc = vfmaq_f64(acc, hi, hi);
      }
      sum = vaddvq_f64(acc);
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      let mut acc = _mm_setzero_pd();
      let ptr = samples.as_ptr();
      for i in 0..chunks {
        let v = _mm_loadu_ps(ptr.add(i * LANES));
        let lo = _mm_cvtps_pd(v); // lanes 0,1 as f64
        // Move lanes 2,3 to low half then convert.
        let hi_ps = _mm_movehl_ps(v, v);
        let hi = _mm_cvtps_pd(hi_ps); // lanes 2,3 as f64
        acc = _mm_add_pd(acc, _mm_mul_pd(lo, lo));
        acc = _mm_add_pd(acc, _mm_mul_pd(hi, hi));
      }
      // Horizontal add of the 2 f64 lanes.
      let shuf = _mm_unpackhi_pd(acc, acc);
      let total = _mm_add_pd(acc, shuf);
      sum = _mm_cvtsd_f64(total);
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  // Scalar remainder.
  let start = scalar_start(len);
  for &s in &samples[start..] {
    sum += (s as f64) * (s as f64);
  }

  sum
}

// ─── Multiply interleaved pairs with two gains ────────────────────
// Used by: stereo pan / per-channel gain on interleaved [L, R, L, R, …] data.

/// Multiplies even-indexed samples by `g_even` and odd-indexed samples
/// by `g_odd` in place.
///
/// For stereo data laid out as `[L0, R0, L1, R1, …]` this applies a
/// constant per-channel gain pair (e.g. a pan position).
pub(crate) fn simd_mul_interleaved_pair(samples: &mut [f32], g_even: f32, g_odd: f32) {
  let len = samples.len();
  let chunks = len / LANES;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let gains: [f32; 4] = [g_even, g_odd, g_even, g_odd];
      let vgains = vld1q_f32(gains.as_ptr());
      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let off = i * LANES;
        let v = vld1q_f32(ptr.add(off));
        vst1q_f32(ptr.add(off), vmulq_f32(v, vgains));
      }
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      // Note: _mm_set_ps takes lanes in reverse order.
      let vgains = _mm_set_ps(g_odd, g_even, g_odd, g_even);
      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let off = i * LANES;
        let v = _mm_loadu_ps(ptr.add(off));
        _mm_storeu_ps(ptr.add(off), _mm_mul_ps(v, vgains));
      }
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  // Scalar tail — `start` is a multiple of LANES (even), so `start + i`
  // keeps the even/odd channel assignment.
  let start = scalar_start(len);
  for (i, s) in samples[start..].iter_mut().enumerate() {
    *s *= if (start + i) % 2 == 0 { g_even } else { g_odd };
  }
}

// ─── Multiply by linear ramp ──────────────────────────────────────
// Used by: fade_in, fade_out

/// Multiplies `samples[i]` by `(start_gain + i * gain_step)`.
///
/// The caller is responsible for computing correct `start_gain` and
/// `gain_step` values for the slice being processed.
pub(crate) fn simd_mul_ramp(samples: &mut [f32], start_gain: f32, gain_step: f32) {
  let len = samples.len();
  let chunks = len / LANES;

  #[cfg(target_arch = "aarch64")]
  {
    use std::arch::aarch64::*;
    unsafe {
      let vstep4 = vdupq_n_f32(gain_step * LANES as f32);
      // Initial gain vector: [start, start+step, start+2*step, start+3*step]
      let offsets: [f32; 4] = [0.0, 1.0, 2.0, 3.0];
      let voff = vld1q_f32(offsets.as_ptr());
      let vstart = vdupq_n_f32(start_gain);
      let vstep1 = vdupq_n_f32(gain_step);
      let mut vgain = vaddq_f32(vstart, vmulq_f32(voff, vstep1));

      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = vld1q_f32(ptr.add(offset));
        vst1q_f32(ptr.add(offset), vmulq_f32(v, vgain));
        vgain = vaddq_f32(vgain, vstep4);
      }
    }
  }

  #[cfg(target_arch = "x86_64")]
  {
    use std::arch::x86_64::*;
    unsafe {
      let vstep4 = _mm_set1_ps(gain_step * LANES as f32);
      let vgain_init = _mm_set_ps(
        start_gain + 3.0 * gain_step,
        start_gain + 2.0 * gain_step,
        start_gain + gain_step,
        start_gain,
      );
      let mut vgain = vgain_init;

      let ptr = samples.as_mut_ptr();
      for i in 0..chunks {
        let offset = i * LANES;
        let v = _mm_loadu_ps(ptr.add(offset));
        _mm_storeu_ps(ptr.add(offset), _mm_mul_ps(v, vgain));
        vgain = _mm_add_ps(vgain, vstep4);
      }
    }
  }

  #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
  let _ = chunks;

  // Scalar remainder
  let start = scalar_start(len);
  for (i, s) in samples[start..].iter_mut().enumerate() {
    let gain = start_gain + ((start + i) as f32) * gain_step;
    *s *= gain;
  }
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
  use super::*;

  /// Reference scalar implementations for comparison.
  fn ref_mul_clamp(samples: &mut [f32], gain: f32) {
    for s in samples.iter_mut() {
      *s = (*s * gain).clamp(-1.0, 1.0);
    }
  }

  fn ref_add_clamp(a: &[f32], b: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
      out[i] = (a[i] + b[i]).clamp(-1.0, 1.0);
    }
  }

  fn ref_max_abs(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
  }

  fn ref_mul_ramp(samples: &mut [f32], start_gain: f32, gain_step: f32) {
    for (i, s) in samples.iter_mut().enumerate() {
      *s *= start_gain + i as f32 * gain_step;
    }
  }

  fn approx_eq(a: &[f32], b: &[f32], tol: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
  }

  // ── simd_mul_clamp ──

  #[test]
  fn test_mul_clamp_aligned() {
    let mut simd_buf = vec![0.3, -0.5, 0.8, -0.9, 0.1, 0.6, -0.2, 0.4];
    let mut ref_buf = simd_buf.clone();
    simd_mul_clamp(&mut simd_buf, 2.0);
    ref_mul_clamp(&mut ref_buf, 2.0);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-6));
  }

  #[test]
  fn test_mul_clamp_unaligned() {
    let mut simd_buf = vec![0.3, -0.5, 0.8, -0.9, 0.1];
    let mut ref_buf = simd_buf.clone();
    simd_mul_clamp(&mut simd_buf, 1.5);
    ref_mul_clamp(&mut ref_buf, 1.5);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-6));
  }

  #[test]
  fn test_mul_clamp_empty() {
    let mut buf: Vec<f32> = vec![];
    simd_mul_clamp(&mut buf, 2.0);
    assert!(buf.is_empty());
  }

  #[test]
  fn test_mul_clamp_clamping() {
    let mut buf = vec![0.8, -0.8];
    simd_mul_clamp(&mut buf, 2.0);
    assert!((buf[0] - 1.0).abs() < 1e-6);
    assert!((buf[1] - (-1.0)).abs() < 1e-6);
  }

  // ── simd_mul ──

  #[test]
  fn test_mul_no_clamp() {
    let mut buf = vec![0.5, -0.3, 0.8, 0.1, 0.2, -0.7, 0.4, 0.6, 0.9];
    let mut ref_buf = buf.clone();
    simd_mul(&mut buf, 0.5);
    for s in &mut ref_buf {
      *s *= 0.5;
    }
    assert!(approx_eq(&buf, &ref_buf, 1e-6));
  }

  // ── simd_add_clamp ──

  #[test]
  fn test_add_clamp_aligned() {
    let a = vec![0.3, -0.5, 0.8, -0.9, 0.1, 0.6, -0.2, 0.4];
    let b = vec![0.4, 0.7, -0.3, -0.5, 0.9, 0.6, 0.1, -0.8];
    let mut simd_out = vec![0.0; 8];
    let mut ref_out = vec![0.0; 8];
    simd_add_clamp(&a, &b, &mut simd_out);
    ref_add_clamp(&a, &b, &mut ref_out);
    assert!(approx_eq(&simd_out, &ref_out, 1e-6));
  }

  #[test]
  fn test_add_clamp_unaligned() {
    let a = vec![0.5, 0.7, 0.3];
    let b = vec![0.6, 0.5, -0.1];
    let mut simd_out = vec![0.0; 3];
    let mut ref_out = vec![0.0; 3];
    simd_add_clamp(&a, &b, &mut simd_out);
    ref_add_clamp(&a, &b, &mut ref_out);
    assert!(approx_eq(&simd_out, &ref_out, 1e-6));
  }

  // ── simd_max_abs ──

  #[test]
  fn test_max_abs_aligned() {
    let samples = vec![0.1, -0.5, 0.3, 0.9, -0.2, 0.4, -0.8, 0.7];
    let simd_res = simd_max_abs(&samples);
    let ref_res = ref_max_abs(&samples);
    assert!((simd_res - ref_res).abs() < 1e-6);
  }

  #[test]
  fn test_max_abs_unaligned() {
    let samples = vec![-0.3, 0.7, 0.1, -0.9, 0.2];
    let simd_res = simd_max_abs(&samples);
    let ref_res = ref_max_abs(&samples);
    assert!((simd_res - ref_res).abs() < 1e-6);
  }

  #[test]
  fn test_max_abs_empty() {
    let result = simd_max_abs(&[]);
    assert!((result - 0.0).abs() < 1e-6);
  }

  #[test]
  fn test_max_abs_single() {
    assert!((simd_max_abs(&[-0.42]) - 0.42).abs() < 1e-6);
  }

  // ── simd_mul_ramp ──

  #[test]
  fn test_mul_ramp_aligned() {
    let mut simd_buf = vec![1.0; 8];
    let mut ref_buf = vec![1.0; 8];
    let step = 1.0 / 8.0;
    simd_mul_ramp(&mut simd_buf, 0.0, step);
    ref_mul_ramp(&mut ref_buf, 0.0, step);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-5));
  }

  #[test]
  fn test_mul_ramp_unaligned() {
    let mut simd_buf = vec![1.0; 5];
    let mut ref_buf = vec![1.0; 5];
    let step = 0.2;
    simd_mul_ramp(&mut simd_buf, 0.0, step);
    ref_mul_ramp(&mut ref_buf, 0.0, step);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-5));
  }

  #[test]
  fn test_mul_ramp_fade_out() {
    // Decreasing ramp
    let mut simd_buf = vec![0.5; 8];
    let mut ref_buf = vec![0.5; 8];
    let step = -1.0 / 8.0;
    simd_mul_ramp(&mut simd_buf, 1.0, step);
    ref_mul_ramp(&mut ref_buf, 1.0, step);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-5));
  }

  // ── Large buffer test ──

  #[test]
  fn test_large_buffer_mul_clamp() {
    let n = 100_003; // not aligned
    let mut simd_buf: Vec<f32> = (0..n).map(|i| (i as f32 / n as f32) * 2.0 - 1.0).collect();
    let mut ref_buf = simd_buf.clone();
    simd_mul_clamp(&mut simd_buf, 1.3);
    ref_mul_clamp(&mut ref_buf, 1.3);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-6));
  }

  #[test]
  fn test_large_buffer_max_abs() {
    let n = 100_003;
    let samples: Vec<f32> = (0..n).map(|i| (i as f32 / n as f32) * 2.0 - 1.0).collect();
    let simd_res = simd_max_abs(&samples);
    let ref_res = ref_max_abs(&samples);
    assert!((simd_res - ref_res).abs() < 1e-5);
  }

  // ── simd_sum_sq ──

  fn ref_sum_sq(samples: &[f32]) -> f64 {
    samples.iter().map(|&s| (s as f64) * (s as f64)).sum()
  }

  #[test]
  fn test_sum_sq_aligned() {
    let s = vec![0.3, -0.5, 0.8, -0.9, 0.1, 0.6, -0.2, 0.4];
    let simd_res = simd_sum_sq(&s);
    let ref_res = ref_sum_sq(&s);
    assert!((simd_res - ref_res).abs() < 1e-9);
  }

  #[test]
  fn test_sum_sq_unaligned() {
    let s = vec![0.5, -0.5, 1.0, -1.0, 0.25];
    let simd_res = simd_sum_sq(&s);
    let ref_res = ref_sum_sq(&s);
    assert!((simd_res - ref_res).abs() < 1e-9);
  }

  #[test]
  fn test_sum_sq_empty() {
    assert_eq!(simd_sum_sq(&[]), 0.0);
  }

  #[test]
  fn test_sum_sq_known_values() {
    // 1²+2²+3²+4² = 30
    let s = vec![1.0, 2.0, 3.0, 4.0];
    assert!((simd_sum_sq(&s) - 30.0).abs() < 1e-9);
  }

  #[test]
  fn test_sum_sq_large_buffer() {
    let n = 100_003;
    let s: Vec<f32> = (0..n).map(|i| (i as f32 / n as f32) - 0.5).collect();
    let simd_res = simd_sum_sq(&s);
    let ref_res = ref_sum_sq(&s);
    // Allow a small relative tolerance for SIMD vs naive accumulation order.
    let tol = ref_res.abs() * 1e-7 + 1e-7;
    assert!(
      (simd_res - ref_res).abs() < tol,
      "simd={simd_res}, ref={ref_res}"
    );
  }

  // ── simd_mul_interleaved_pair ──

  fn ref_mul_interleaved_pair(samples: &mut [f32], g_even: f32, g_odd: f32) {
    for (i, s) in samples.iter_mut().enumerate() {
      *s *= if i % 2 == 0 { g_even } else { g_odd };
    }
  }

  #[test]
  fn test_mul_interleaved_pair_aligned() {
    let mut simd_buf = vec![1.0, 1.0, 0.5, -0.5, 0.25, 0.25, -1.0, 1.0];
    let mut ref_buf = simd_buf.clone();
    simd_mul_interleaved_pair(&mut simd_buf, 0.7, 0.3);
    ref_mul_interleaved_pair(&mut ref_buf, 0.7, 0.3);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-6));
  }

  #[test]
  fn test_mul_interleaved_pair_unaligned() {
    let mut simd_buf = vec![1.0, 1.0, 0.5, -0.5, 0.2];
    let mut ref_buf = simd_buf.clone();
    simd_mul_interleaved_pair(&mut simd_buf, 0.6, 0.4);
    ref_mul_interleaved_pair(&mut ref_buf, 0.6, 0.4);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-6));
  }

  #[test]
  fn test_mul_interleaved_pair_full_pan() {
    // g_even=1, g_odd=0 zeros the right channel of [L,R,L,R].
    let mut buf = vec![1.0, 1.0, 0.5, 0.5];
    simd_mul_interleaved_pair(&mut buf, 1.0, 0.0);
    assert_eq!(buf, vec![1.0, 0.0, 0.5, 0.0]);
  }

  #[test]
  fn test_mul_interleaved_pair_large_buffer() {
    let n = 100_003;
    let mut simd_buf: Vec<f32> = (0..n).map(|i| (i as f32) * 0.001 - 0.5).collect();
    let mut ref_buf = simd_buf.clone();
    simd_mul_interleaved_pair(&mut simd_buf, 0.5, 1.5);
    ref_mul_interleaved_pair(&mut ref_buf, 0.5, 1.5);
    assert!(approx_eq(&simd_buf, &ref_buf, 1e-5));
  }
}
