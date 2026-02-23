//! Real-time audio effect processing.
//!
//! Provides an `AudioEffect` trait and built-in effects that can be chained
//! together in an `EffectChain` and applied during playback.

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

// ─── Built-in Effects ────────────────────────────────────────────────

/// Volume adjustment effect.
#[derive(Debug)]
pub struct VolumeEffect {
    /// Gain multiplier (0.0 = mute, 1.0 = unity, >1.0 = boost).
    pub gain: f32,
}

impl VolumeEffect {
    pub fn new(gain: f32) -> Self {
        Self { gain }
    }
}

impl AudioEffect for VolumeEffect {
    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, _channels: u16) {
        for s in samples.iter_mut() {
            *s *= self.gain;
        }
    }

    fn name(&self) -> &str {
        "Volume"
    }
}

/// Simple first-order low-pass filter effect.
#[derive(Debug)]
pub struct LowPassEffect {
    cutoff_hz: f64,
    prev: [f64; 2], // per-channel state (max 2 channels)
}

impl LowPassEffect {
    pub fn new(cutoff_hz: f64) -> Self {
        Self {
            cutoff_hz,
            prev: [0.0; 2],
        }
    }
}

impl AudioEffect for LowPassEffect {
    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let rc = 1.0 / (2.0 * std::f64::consts::PI * self.cutoff_hz);
        let dt = 1.0 / sample_rate as f64;
        let alpha = dt / (rc + dt);
        let ch = channels as usize;

        for i in (0..samples.len()).step_by(ch) {
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

    fn name(&self) -> &str {
        "LowPass"
    }
}

/// Simple hard-knee compressor effect.
#[derive(Debug)]
pub struct CompressorEffect {
    threshold: f32,
    ratio: f32,
    envelope: f32,
    attack_coeff: f32,
    release_coeff: f32,
}

impl CompressorEffect {
    /// Creates a compressor effect.
    ///
    /// * `threshold` — amplitude above which compression starts (0.0–1.0).
    /// * `ratio` — compression ratio (e.g. 4.0 = 4:1).
    /// * `attack_ms` — attack time in milliseconds.
    /// * `release_ms` — release time in milliseconds.
    pub fn new(threshold: f32, ratio: f32, attack_ms: f32, release_ms: f32) -> Self {
        // Coefficients will be recomputed per-block based on sample rate
        Self {
            threshold,
            ratio,
            envelope: 0.0,
            attack_coeff: attack_ms,
            release_coeff: release_ms,
        }
    }
}

impl AudioEffect for CompressorEffect {
    fn process(&mut self, samples: &mut [f32], sample_rate: u32, _channels: u16) {
        let attack = (-1.0 / (self.attack_coeff * 0.001 * sample_rate as f32)).exp();
        let release = (-1.0 / (self.release_coeff * 0.001 * sample_rate as f32)).exp();

        for s in samples.iter_mut() {
            let input_abs = s.abs();

            // Envelope follower
            if input_abs > self.envelope {
                self.envelope = attack * self.envelope + (1.0 - attack) * input_abs;
            } else {
                self.envelope = release * self.envelope + (1.0 - release) * input_abs;
            }

            // Compute gain reduction
            if self.envelope > self.threshold {
                let over = self.envelope - self.threshold;
                let compressed_over = over / self.ratio;
                let target = self.threshold + compressed_over;
                let gain = if self.envelope > 0.0 {
                    target / self.envelope
                } else {
                    1.0
                };
                *s *= gain;
            }
        }
    }

    fn name(&self) -> &str {
        "Compressor"
    }
}

/// High-pass filter effect (first-order).
#[derive(Debug)]
pub struct HighPassEffect {
    cutoff_hz: f64,
    prev_input: [f64; 2],
    prev_output: [f64; 2],
}

impl HighPassEffect {
    pub fn new(cutoff_hz: f64) -> Self {
        Self {
            cutoff_hz,
            prev_input: [0.0; 2],
            prev_output: [0.0; 2],
        }
    }
}

impl AudioEffect for HighPassEffect {
    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        let rc = 1.0 / (2.0 * std::f64::consts::PI * self.cutoff_hz);
        let dt = 1.0 / sample_rate as f64;
        let alpha = rc / (rc + dt);
        let ch = channels as usize;

        for i in (0..samples.len()).step_by(ch) {
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

    fn name(&self) -> &str {
        "HighPass"
    }
}

#[cfg(test)]
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
        // Generate a high-frequency signal (10kHz at 44100Hz)
        let mut high_freq: Vec<f32> = (0..1024)
            .map(|i| {
                let t = i as f64 / 44100.0;
                (t * 10000.0 * 2.0 * std::f64::consts::PI).sin() as f32
            })
            .collect();

        let original_rms: f64 = high_freq.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / high_freq.len() as f64;

        let mut lp = LowPassEffect::new(500.0);
        lp.process(&mut high_freq, 44100, 1);

        let filtered_rms: f64 = high_freq.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / high_freq.len() as f64;

        assert!(
            filtered_rms < original_rms * 0.1,
            "Low-pass should attenuate 10kHz signal significantly"
        );
    }

    #[test]
    fn test_effect_chain() {
        let mut chain = EffectChain::new();
        chain.push(Box::new(VolumeEffect::new(2.0)));
        chain.push(Box::new(VolumeEffect::new(0.5)));

        let mut samples = vec![0.4f32, -0.4];
        chain.process(&mut samples, 44100, 1);

        // 2.0 * 0.5 = 1.0 overall gain
        assert!((samples[0] - 0.4).abs() < 1e-6);
        assert!((samples[1] - (-0.4)).abs() < 1e-6);
    }

    #[test]
    fn test_compressor_reduces_loud_signal() {
        let mut comp = CompressorEffect::new(0.3, 4.0, 1.0, 10.0);
        let mut samples = vec![0.8f32; 4410]; // 0.1 seconds at 44100
        comp.process(&mut samples, 44100, 1);

        // Check the last portion where envelope has settled
        let tail = &samples[2000..];
        let peak = tail.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.8, "Compressor should reduce peaks above threshold, got {peak}");
    }

    #[test]
    fn test_highpass_attenuates_low_freq() {
        // Generate a low-frequency signal (50Hz at 44100Hz)
        let mut low_freq: Vec<f32> = (0..4410)
            .map(|i| {
                let t = i as f64 / 44100.0;
                (t * 50.0 * 2.0 * std::f64::consts::PI).sin() as f32
            })
            .collect();

        let original_rms: f64 = low_freq.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / low_freq.len() as f64;

        let mut hp = HighPassEffect::new(1000.0);
        hp.process(&mut low_freq, 44100, 1);

        let filtered_rms: f64 = low_freq.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / low_freq.len() as f64;

        assert!(
            filtered_rms < original_rms * 0.3,
            "High-pass should attenuate 50Hz signal"
        );
    }
}
