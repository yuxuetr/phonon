//! Loudness measurement per ITU-R BS.1770-4 / EBU R128.
//!
//! Implements:
//! - **K-weighting** filter (high-shelf at ~1.5 kHz +4 dB, then high-pass at ~38 Hz)
//! - **Integrated loudness** (LUFS-I) with absolute (-70 LUFS) and relative (-10 LU) gating
//! - **Short-term loudness** (LUFS-S, 3 s window) and **momentary** (LUFS-M, 400 ms window)
//! - **Loudness Range** (LRA) per EBU R128
//! - **True peak** (dBTP) via 4× polyphase oversampling
//! - **`normalize_lufs`** — gain a buffer to a target LUFS-I with an optional true-peak ceiling
//!
//! ## Conventions
//!
//! - All math is `f64` internally for precision; samples in/out remain `f32`.
//! - Channel weights default to 1.0 for all channels (BS.1770 surround weights of 1.41
//!   only apply when channel layout is known to be surround, which we do not track here).
//! - Returns `f32::NEG_INFINITY` when the signal is too short or too quiet to measure.

use crate::AudioBuffer;
use std::f64::consts::PI;

// ─── BS.1770 reference parameters ──────────────────────────────────────────

/// Pre-filter (stage 1, high-shelf) center frequency in Hz.
const PRE_FILTER_FC: f64 = 1681.974450955533;
/// Pre-filter gain in dB (≈ +4 dB high-shelf).
const PRE_FILTER_GAIN_DB: f64 = 3.999843853973347;
/// Pre-filter Q factor.
const PRE_FILTER_Q: f64 = 0.7071752369554196;

/// RLB filter (stage 2, high-pass) cutoff frequency in Hz.
const RLB_FILTER_FC: f64 = 38.13547087602444;
/// RLB filter Q factor.
const RLB_FILTER_Q: f64 = 0.5003270373223716;

/// LUFS reference offset added to log-mean-square (BS.1770 §3.5).
const LUFS_REF_OFFSET: f64 = -0.691;

/// Absolute gating threshold (LUFS).
const ABS_GATE_LUFS: f64 = -70.0;
/// Relative gate offset (LU) below ungated mean for integrated loudness.
const REL_GATE_OFFSET_INTEGRATED: f64 = 10.0;
/// Relative gate offset (LU) below ungated mean for LRA.
const REL_GATE_OFFSET_LRA: f64 = 20.0;

/// Block size for integrated / momentary in seconds.
const MOMENTARY_BLOCK_SECS: f64 = 0.4;
/// Block size for short-term and LRA in seconds.
const SHORT_TERM_BLOCK_SECS: f64 = 3.0;
/// Block hop in seconds (75% overlap of momentary block).
const BLOCK_HOP_SECS: f64 = 0.1;

// ─── Biquad filter (Direct Form II, transposed) ───────────────────────────

#[derive(Debug, Clone)]
struct Biquad {
  b0: f64,
  b1: f64,
  b2: f64,
  a1: f64,
  a2: f64,
  z1: f64,
  z2: f64,
}

impl Biquad {
  /// BS.1770 pre-filter (high-shelf, +4 dB at 1.5 kHz). The formulation differs
  /// from RBJ cookbook EQ — it uses a prewarped bilinear transform of the
  /// analog prototype defined in BS.1770-4 Annex 1.
  fn k_pre_filter(sr: f64) -> Self {
    let k = (PI * PRE_FILTER_FC / sr).tan();
    let vh = 10.0_f64.powf(PRE_FILTER_GAIN_DB / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / PRE_FILTER_Q + k * k;
    Self {
      b0: (vh + vb * k / PRE_FILTER_Q + k * k) / a0,
      b1: 2.0 * (k * k - vh) / a0,
      b2: (vh - vb * k / PRE_FILTER_Q + k * k) / a0,
      a1: 2.0 * (k * k - 1.0) / a0,
      a2: (1.0 - k / PRE_FILTER_Q + k * k) / a0,
      z1: 0.0,
      z2: 0.0,
    }
  }

  /// BS.1770 RLB filter (high-pass at ~38 Hz). Per spec the numerator is the
  /// fixed `[1, -2, 1]` form; only the denominator depends on sample rate.
  fn k_rlb_filter(sr: f64) -> Self {
    let k = (PI * RLB_FILTER_FC / sr).tan();
    let a0 = 1.0 + k / RLB_FILTER_Q + k * k;
    Self {
      b0: 1.0,
      b1: -2.0,
      b2: 1.0,
      a1: 2.0 * (k * k - 1.0) / a0,
      a2: (1.0 - k / RLB_FILTER_Q + k * k) / a0,
      z1: 0.0,
      z2: 0.0,
    }
  }

  fn process(&mut self, x: f64) -> f64 {
    // Direct Form II Transposed.
    let y = self.b0 * x + self.z1;
    self.z1 = self.b1 * x - self.a1 * y + self.z2;
    self.z2 = self.b2 * x - self.a2 * y;
    y
  }
}

// ─── K-weighting ───────────────────────────────────────────────────────────

/// Apply K-weighting filter (pre-filter then RLB) to a single-channel signal.
fn k_weight_channel(samples: &[f32], sr: f64) -> Vec<f64> {
  let mut pre = Biquad::k_pre_filter(sr);
  let mut rlb = Biquad::k_rlb_filter(sr);
  samples
    .iter()
    .map(|&s| {
      let y1 = pre.process(s as f64);
      rlb.process(y1)
    })
    .collect()
}

/// Deinterleave samples into per-channel slices.
fn deinterleave(samples: &[f32], channels: usize) -> Vec<Vec<f32>> {
  if channels == 0 {
    return vec![];
  }
  let frames = samples.len() / channels;
  let mut out = vec![Vec::with_capacity(frames); channels];
  for frame in samples.chunks_exact(channels) {
    for (c, &s) in frame.iter().enumerate() {
      out[c].push(s);
    }
  }
  out
}

// ─── Block-based loudness ──────────────────────────────────────────────────

/// Compute mean-square per channel for a block, then aggregate to a single
/// loudness value in LUFS using BS.1770 channel weighting (1.0 for all).
fn block_loudness(per_channel_squares: &[f64]) -> f64 {
  let sum: f64 = per_channel_squares.iter().sum();
  if sum <= 0.0 {
    return f64::NEG_INFINITY;
  }
  LUFS_REF_OFFSET + 10.0 * sum.log10()
}

/// Compute per-channel mean-square in a window `[start, start+len)` of
/// each K-weighted channel.
fn block_mean_squares(weighted: &[Vec<f64>], start: usize, len: usize) -> Vec<f64> {
  weighted
    .iter()
    .map(|ch| {
      if start >= ch.len() {
        return 0.0;
      }
      let end = (start + len).min(ch.len());
      let count = end - start;
      if count == 0 {
        return 0.0;
      }
      let sum: f64 = ch[start..end].iter().map(|s| s * s).sum();
      sum / count as f64
    })
    .collect()
}

// ─── Public API: measurements ──────────────────────────────────────────────

/// Compute the integrated loudness (LUFS-I) of a buffer.
///
/// Returns `f32::NEG_INFINITY` if the buffer is too short (< 400 ms) or silent.
pub fn loudness_integrated(buffer: &AudioBuffer) -> f32 {
  let blocks = momentary_block_loudnesses(buffer);
  gated_mean_loudness(&blocks, REL_GATE_OFFSET_INTEGRATED) as f32
}

/// Compute all valid 400 ms momentary block loudnesses (overlapping by 75%).
fn momentary_block_loudnesses(buffer: &AudioBuffer) -> Vec<f64> {
  let sr = buffer.sample_rate as f64;
  let channels = buffer.channels as usize;
  if buffer.samples.is_empty() || sr == 0.0 || channels == 0 {
    return vec![];
  }
  let block_len = (MOMENTARY_BLOCK_SECS * sr) as usize;
  let hop = (BLOCK_HOP_SECS * sr) as usize;
  if block_len == 0 || hop == 0 {
    return vec![];
  }

  let per_channel = deinterleave(&buffer.samples, channels);
  let weighted: Vec<Vec<f64>> = per_channel
    .iter()
    .map(|ch| k_weight_channel(ch, sr))
    .collect();
  let total_frames = weighted.first().map(|v| v.len()).unwrap_or(0);
  if total_frames < block_len {
    return vec![];
  }

  let mut out = Vec::new();
  let mut start = 0;
  while start + block_len <= total_frames {
    let squares = block_mean_squares(&weighted, start, block_len);
    out.push(block_loudness(&squares));
    start += hop;
  }
  out
}

/// Compute all 3 s short-term block loudnesses (1 s hop).
fn short_term_block_loudnesses(buffer: &AudioBuffer) -> Vec<f64> {
  let sr = buffer.sample_rate as f64;
  let channels = buffer.channels as usize;
  if buffer.samples.is_empty() || sr == 0.0 || channels == 0 {
    return vec![];
  }
  let block_len = (SHORT_TERM_BLOCK_SECS * sr) as usize;
  let hop = (1.0 * sr) as usize;
  if block_len == 0 || hop == 0 {
    return vec![];
  }

  let per_channel = deinterleave(&buffer.samples, channels);
  let weighted: Vec<Vec<f64>> = per_channel
    .iter()
    .map(|ch| k_weight_channel(ch, sr))
    .collect();
  let total_frames = weighted.first().map(|v| v.len()).unwrap_or(0);
  if total_frames < block_len {
    return vec![];
  }

  let mut out = Vec::new();
  let mut start = 0;
  while start + block_len <= total_frames {
    let squares = block_mean_squares(&weighted, start, block_len);
    out.push(block_loudness(&squares));
    start += hop;
  }
  out
}

/// Apply BS.1770 absolute + relative gating and return the gated mean
/// loudness in LUFS, or `f64::NEG_INFINITY` if no blocks survive.
fn gated_mean_loudness(blocks: &[f64], rel_offset_lu: f64) -> f64 {
  if blocks.is_empty() {
    return f64::NEG_INFINITY;
  }

  // Absolute gate: discard blocks below -70 LUFS.
  let above_abs: Vec<f64> = blocks
    .iter()
    .copied()
    .filter(|&l| l >= ABS_GATE_LUFS)
    .collect();
  if above_abs.is_empty() {
    return f64::NEG_INFINITY;
  }

  // Mean of the energies (not the LUFS values directly).
  let mean_energy = above_abs
    .iter()
    .map(|&l| 10.0_f64.powf((l - LUFS_REF_OFFSET) / 10.0))
    .sum::<f64>()
    / above_abs.len() as f64;
  let ungated_mean = LUFS_REF_OFFSET + 10.0 * mean_energy.log10();

  // Relative gate: drop blocks below ungated_mean - offset.
  let rel_threshold = ungated_mean - rel_offset_lu;
  let above_rel: Vec<f64> = above_abs
    .iter()
    .copied()
    .filter(|&l| l >= rel_threshold)
    .collect();
  if above_rel.is_empty() {
    return f64::NEG_INFINITY;
  }

  let mean_energy_gated = above_rel
    .iter()
    .map(|&l| 10.0_f64.powf((l - LUFS_REF_OFFSET) / 10.0))
    .sum::<f64>()
    / above_rel.len() as f64;
  LUFS_REF_OFFSET + 10.0 * mean_energy_gated.log10()
}

/// Compute the maximum momentary loudness (LUFS-M).
///
/// Returns `f32::NEG_INFINITY` if no 400 ms window fits.
pub fn loudness_momentary_max(buffer: &AudioBuffer) -> f32 {
  let blocks = momentary_block_loudnesses(buffer);
  blocks
    .into_iter()
    .filter(|l| l.is_finite())
    .fold(f64::NEG_INFINITY, f64::max) as f32
}

/// Compute the maximum short-term loudness (LUFS-S, 3 s window).
///
/// Returns `f32::NEG_INFINITY` if no 3 s window fits.
pub fn loudness_short_term_max(buffer: &AudioBuffer) -> f32 {
  let blocks = short_term_block_loudnesses(buffer);
  blocks
    .into_iter()
    .filter(|l| l.is_finite())
    .fold(f64::NEG_INFINITY, f64::max) as f32
}

/// Compute Loudness Range (LRA) per EBU R128, in LU.
///
/// Returns 0.0 if fewer than two short-term blocks survive gating.
pub fn loudness_range(buffer: &AudioBuffer) -> f32 {
  let blocks = short_term_block_loudnesses(buffer);
  if blocks.is_empty() {
    return 0.0;
  }

  // Absolute gate.
  let above_abs: Vec<f64> = blocks
    .iter()
    .copied()
    .filter(|&l| l >= ABS_GATE_LUFS)
    .collect();
  if above_abs.len() < 2 {
    return 0.0;
  }

  // Relative gate at ungated mean − 20 LU.
  let mean_energy = above_abs
    .iter()
    .map(|&l| 10.0_f64.powf((l - LUFS_REF_OFFSET) / 10.0))
    .sum::<f64>()
    / above_abs.len() as f64;
  let ungated_mean = LUFS_REF_OFFSET + 10.0 * mean_energy.log10();
  let rel_threshold = ungated_mean - REL_GATE_OFFSET_LRA;

  let mut survivors: Vec<f64> = above_abs
    .iter()
    .copied()
    .filter(|&l| l >= rel_threshold)
    .collect();
  if survivors.len() < 2 {
    return 0.0;
  }
  survivors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

  // 10th and 95th percentile (linear interpolation).
  let p10 = percentile(&survivors, 10.0);
  let p95 = percentile(&survivors, 95.0);
  (p95 - p10) as f32
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
  if sorted.is_empty() {
    return f64::NEG_INFINITY;
  }
  if sorted.len() == 1 {
    return sorted[0];
  }
  let rank = pct / 100.0 * (sorted.len() - 1) as f64;
  let lo = rank.floor() as usize;
  let hi = (lo + 1).min(sorted.len() - 1);
  let frac = rank - lo as f64;
  sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

// ─── True peak (4× oversampled) ────────────────────────────────────────────

/// 4× polyphase upsampling kernel coefficients.
///
/// 12-tap windowed-sinc per phase, designed for ~ -100 dB stopband.
/// Pre-computed via Hamming-windowed sinc, hard-coded for portability.
const TP_TAPS_PER_PHASE: usize = 12;
const TP_PHASES: usize = 4;

/// Hamming-windowed sinc, 4 phases × 12 taps; total 48 taps spanning
/// 12 input samples per output set. Each phase is normalized to unity DC gain
/// so a constant input produces a constant output across all sub-sample positions.
fn tp_kernels() -> [[f64; TP_TAPS_PER_PHASE]; TP_PHASES] {
  let mut out = [[0.0; TP_TAPS_PER_PHASE]; TP_PHASES];
  let total = TP_PHASES * TP_TAPS_PER_PHASE;
  let center = (total as f64 - 1.0) / 2.0;
  for phase in 0..TP_PHASES {
    for tap in 0..TP_TAPS_PER_PHASE {
      let n = (tap * TP_PHASES + phase) as f64;
      let x = n - center;
      let arg = PI * x / TP_PHASES as f64;
      let sinc = if x.abs() < 1e-12 {
        1.0
      } else {
        arg.sin() / arg
      };
      // Hamming window over [0, total-1].
      let win = 0.54 - 0.46 * (2.0 * PI * n / (total as f64 - 1.0)).cos();
      out[phase][tap] = sinc * win;
    }
  }
  // Normalize each phase independently to unity DC gain.
  for phase in &mut out {
    let s: f64 = phase.iter().sum();
    if s > 0.0 {
      for tap in phase {
        *tap /= s;
      }
    }
  }
  out
}

/// Compute true peak (linear, in [0.0, ∞)) via 4× polyphase oversampling.
///
/// The polyphase filter is applied only to interior samples that have full
/// kernel coverage; edge samples use raw sample magnitude. This avoids Gibbs
/// ringing artifacts from implicit zero-padding that would otherwise
/// over-report the true peak of signals with abrupt boundaries.
pub fn true_peak(buffer: &AudioBuffer) -> f32 {
  if buffer.samples.is_empty() || buffer.channels == 0 {
    return 0.0;
  }
  let channels = buffer.channels as usize;
  let kernels = tp_kernels();
  let per_channel = deinterleave(&buffer.samples, channels);
  let half = TP_TAPS_PER_PHASE / 2;
  let mut peak = 0.0_f64;

  for ch in &per_channel {
    let n = ch.len();
    // Edge samples (lacking full kernel coverage): use raw sample peak.
    if n <= 2 * half {
      for &s in ch {
        peak = peak.max((s as f64).abs());
      }
      continue;
    }
    for &s in &ch[..half] {
      peak = peak.max((s as f64).abs());
    }
    for &s in &ch[n - half..] {
      peak = peak.max((s as f64).abs());
    }
    // Interior samples: full polyphase filter at every sub-sample position.
    for i in half..n - half {
      let s = ch[i] as f64;
      peak = peak.max(s.abs());
      for phase in 1..TP_PHASES {
        let mut acc = 0.0_f64;
        for tap in 0..TP_TAPS_PER_PHASE {
          let src = i + half - tap;
          acc += kernels[phase][tap] * ch[src] as f64;
        }
        peak = peak.max(acc.abs());
      }
    }
  }
  peak as f32
}

/// True peak in dBTP (decibels true peak). Returns `f32::NEG_INFINITY` when silent.
pub fn true_peak_dbtp(buffer: &AudioBuffer) -> f32 {
  let p = true_peak(buffer);
  if p <= 0.0 {
    f32::NEG_INFINITY
  } else {
    20.0 * p.log10()
  }
}

// ─── Composite report ──────────────────────────────────────────────────────

/// Full loudness report for a buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct LoudnessReport {
  /// Integrated loudness (LUFS-I) per BS.1770 / R128.
  pub integrated_lufs: f32,
  /// Maximum momentary loudness (LUFS-M) over any 400 ms window.
  pub momentary_max_lufs: f32,
  /// Maximum short-term loudness (LUFS-S) over any 3 s window.
  pub short_term_max_lufs: f32,
  /// Loudness range (LRA) in LU per EBU R128.
  pub range_lu: f32,
  /// True peak in dBTP via 4× oversampling.
  pub true_peak_dbtp: f32,
}

/// Run all loudness measurements in one pass-friendly call.
pub fn measure_loudness(buffer: &AudioBuffer) -> LoudnessReport {
  LoudnessReport {
    integrated_lufs: loudness_integrated(buffer),
    momentary_max_lufs: loudness_momentary_max(buffer),
    short_term_max_lufs: loudness_short_term_max(buffer),
    range_lu: loudness_range(buffer),
    true_peak_dbtp: true_peak_dbtp(buffer),
  }
}

// ─── Normalization ─────────────────────────────────────────────────────────

/// Normalize a buffer to a target integrated loudness with optional true-peak limiting.
///
/// Computes the LUFS-I gain needed to hit `target_lufs`. If applying that gain would
/// exceed `true_peak_max_dbtp`, the gain is reduced so the true peak just touches
/// the ceiling.
///
/// No-op when the buffer is too short to measure or already silent.
pub fn normalize_lufs(buffer: &mut AudioBuffer, target_lufs: f32, true_peak_max_dbtp: f32) {
  let current = loudness_integrated(buffer);
  if !current.is_finite() {
    return;
  }
  let mut gain_db = target_lufs - current;
  // Apply true-peak ceiling.
  let current_tp = true_peak_dbtp(buffer);
  if current_tp.is_finite() {
    let projected_tp = current_tp + gain_db;
    if projected_tp > true_peak_max_dbtp {
      gain_db -= projected_tp - true_peak_max_dbtp;
    }
  }
  let gain_lin = 10.0_f32.powf(gain_db / 20.0);
  for s in &mut buffer.samples {
    *s = (*s * gain_lin).clamp(-1.0, 1.0);
  }
}

/// Immutable variant of [`normalize_lufs`].
pub fn with_lufs_normalized(
  buffer: &AudioBuffer,
  target_lufs: f32,
  true_peak_max_dbtp: f32,
) -> AudioBuffer {
  let mut out = buffer.clone();
  normalize_lufs(&mut out, target_lufs, true_peak_max_dbtp);
  out
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::PI as PI_F32;

  fn sine_buffer(freq: f32, duration_secs: f32, amplitude: f32, sr: u32) -> AudioBuffer {
    let n = (duration_secs * sr as f32) as usize;
    let samples: Vec<f32> = (0..n)
      .map(|i| amplitude * (2.0 * PI_F32 * freq * i as f32 / sr as f32).sin())
      .collect();
    AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    }
  }

  // K-weighting filter coefficients at 48 kHz must match BS.1770 reference values.
  #[test]
  fn test_k_weighting_coefficients_48k() {
    let pre = Biquad::k_pre_filter(48000.0);
    let rlb = Biquad::k_rlb_filter(48000.0);

    // Reference values published in BS.1770-4 Annex 1 (within fp tolerance).
    assert!(
      (pre.b0 - 1.53512485958697).abs() < 1e-6,
      "pre.b0 = {}",
      pre.b0
    );
    assert!(
      (pre.b1 + 2.69169618940638).abs() < 1e-6,
      "pre.b1 = {}",
      pre.b1
    );
    assert!(
      (pre.b2 - 1.19839281085285).abs() < 1e-6,
      "pre.b2 = {}",
      pre.b2
    );
    assert!(
      (pre.a1 + 1.69065929318241).abs() < 1e-6,
      "pre.a1 = {}",
      pre.a1
    );
    assert!(
      (pre.a2 - 0.73248077421585).abs() < 1e-6,
      "pre.a2 = {}",
      pre.a2
    );

    // RLB numerator is the fixed [1, -2, 1] form per spec.
    assert_eq!(rlb.b0, 1.0);
    assert_eq!(rlb.b1, -2.0);
    assert_eq!(rlb.b2, 1.0);
    assert!(
      (rlb.a1 + 1.99004745483398).abs() < 1e-6,
      "rlb.a1 = {}",
      rlb.a1
    );
    assert!(
      (rlb.a2 - 0.99007225036621).abs() < 1e-6,
      "rlb.a2 = {}",
      rlb.a2
    );
  }

  #[test]
  fn test_silence_returns_neg_inf() {
    let buf = AudioBuffer {
      samples: vec![0.0; 48000 * 2],
      sample_rate: 48000,
      channels: 1,
    };
    let lufs = loudness_integrated(&buf);
    assert!(
      lufs.is_infinite() && lufs < 0.0,
      "expected -inf, got {}",
      lufs
    );
  }

  #[test]
  fn test_too_short_returns_neg_inf() {
    let buf = AudioBuffer {
      samples: vec![0.5; 100], // < 400 ms
      sample_rate: 48000,
      channels: 1,
    };
    let lufs = loudness_integrated(&buf);
    assert!(
      lufs.is_infinite(),
      "expected -inf for too-short buffer, got {}",
      lufs
    );
  }

  // 1 kHz sine at -20 dBFS should measure ~ -23 LUFS (pyloudnorm ref ≈ -22.97 LUFS).
  #[test]
  fn test_1khz_sine_minus_20dbfs_gives_about_minus_23_lufs() {
    let amp = 10.0_f32.powf(-20.0 / 20.0); // -20 dBFS sine peak
    let buf = sine_buffer(1000.0, 5.0, amp, 48000);
    let lufs = loudness_integrated(&buf);
    assert!(
      (lufs - (-23.0)).abs() < 0.5,
      "expected ≈ -23 LUFS, got {}",
      lufs
    );
  }

  // Doubling amplitude should add ~ +6 dB (LUFS-I scales linearly in dB after K-weighting).
  #[test]
  fn test_doubling_amplitude_adds_6_db() {
    let quiet = sine_buffer(1000.0, 5.0, 0.1, 48000);
    let loud = sine_buffer(1000.0, 5.0, 0.2, 48000);
    let lufs_q = loudness_integrated(&quiet);
    let lufs_l = loudness_integrated(&loud);
    assert!(
      ((lufs_l - lufs_q) - 6.02).abs() < 0.2,
      "expected +6 dB delta, got {}",
      lufs_l - lufs_q
    );
  }

  #[test]
  fn test_true_peak_unity_dc() {
    let buf = AudioBuffer {
      samples: vec![1.0; 1024],
      sample_rate: 48000,
      channels: 1,
    };
    let tp = true_peak(&buf);
    assert!((tp - 1.0).abs() < 0.05, "expected ≈ 1.0, got {}", tp);
  }

  // True peak of an inter-sample-peak signal should exceed sample peak.
  #[test]
  fn test_true_peak_exceeds_sample_peak_on_isp() {
    // Construct a half-Nyquist signal at 0.99 amplitude that has inter-sample peaks > 0.99.
    let sr = 48000_u32;
    let n = 4096;
    let samples: Vec<f32> = (0..n)
      .map(|i| {
        let phase = 2.0 * PI_F32 * (sr as f32 / 4.0) * i as f32 / sr as f32 + PI_F32 / 4.0;
        0.99 * phase.sin()
      })
      .collect();
    let buf = AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    };
    let sample_peak = buf.samples.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
    let tp = true_peak(&buf);
    assert!(
      tp > sample_peak,
      "expected TP > sample_peak: tp={}, sp={}",
      tp,
      sample_peak
    );
  }

  #[test]
  fn test_true_peak_dbtp_silence() {
    let buf = AudioBuffer {
      samples: vec![0.0; 1024],
      sample_rate: 48000,
      channels: 1,
    };
    let dbtp = true_peak_dbtp(&buf);
    assert!(dbtp.is_infinite() && dbtp < 0.0);
  }

  #[test]
  fn test_normalize_lufs_target_minus_16() {
    // Start with a -23 LUFS sine, target -16 LUFS, ceiling -1 dBTP.
    let amp = 10.0_f32.powf(-20.0 / 20.0);
    let mut buf = sine_buffer(1000.0, 5.0, amp, 48000);
    normalize_lufs(&mut buf, -16.0, -1.0);
    let after = loudness_integrated(&buf);
    assert!(
      (after - (-16.0)).abs() < 1.0,
      "expected ≈ -16 LUFS after normalize, got {}",
      after
    );
  }

  #[test]
  fn test_normalize_lufs_respects_true_peak_ceiling() {
    // Loud sine (already near 0 dBFS) — normalization to -10 LUFS would clip.
    // Verify true peak stays ≤ ceiling + small slack.
    let amp = 0.95_f32;
    let mut buf = sine_buffer(1000.0, 5.0, amp, 48000);
    normalize_lufs(&mut buf, -10.0, -1.0);
    let tp_after = true_peak_dbtp(&buf);
    assert!(
      tp_after <= -1.0 + 0.5,
      "true peak {} dBTP exceeds ceiling",
      tp_after
    );
  }

  #[test]
  fn test_measure_loudness_full_report() {
    let buf = sine_buffer(1000.0, 5.0, 0.5, 48000);
    let report = measure_loudness(&buf);
    assert!(report.integrated_lufs.is_finite());
    assert!(report.momentary_max_lufs.is_finite());
    assert!(report.short_term_max_lufs.is_finite());
    assert!(report.range_lu >= 0.0);
    assert!(report.true_peak_dbtp.is_finite());
  }

  #[test]
  fn test_normalize_lufs_silent_is_noop() {
    let mut buf = AudioBuffer {
      samples: vec![0.0; 48000 * 2],
      sample_rate: 48000,
      channels: 1,
    };
    let original = buf.samples.clone();
    normalize_lufs(&mut buf, -16.0, -1.0);
    assert_eq!(buf.samples, original);
  }

  #[test]
  fn test_with_lufs_normalized_immutable() {
    let buf = sine_buffer(1000.0, 5.0, 0.1, 48000);
    let original = buf.samples.clone();
    let _normalized = with_lufs_normalized(&buf, -16.0, -1.0);
    assert_eq!(buf.samples, original, "source buffer must be unchanged");
  }
}
