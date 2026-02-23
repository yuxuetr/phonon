pub mod algorithms;

pub use algorithms::trim::trim;
pub use algorithms::mix::mix;
pub use algorithms::normalize::normalize;
pub use algorithms::fade::{fade_in, fade_out};
pub use algorithms::merge::merge;
pub use algorithms::resample::resample;
pub use algorithms::channels::ensure_channels;
pub use algorithms::filters::{filter, FilterType};
pub use algorithms::spectrum::compute_spectrum;
pub use algorithms::speed::change_speed;
pub use algorithms::silence::silence;
pub use algorithms::volume::{adjust_volume, with_volume};
pub use algorithms::crossfade::crossfade;
pub use algorithms::overlay::overlay_at_position;
pub use algorithms::reverse::reverse;
pub use algorithms::repeat::repeat;
pub use algorithms::loudness::{dbfs, rms, peak, peak_dbfs, adjust_volume_db, with_volume_db};
pub use algorithms::compressor::{compress, with_compression, CompressorConfig};
pub use algorithms::generators::{generate_signal, generate_tone, generate_beep, Waveform};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PhononError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("WAV format error: {0}")]
    Wav(String),

    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(u32),

    #[error("Channel count mismatch: expected {expected}, got {actual}")]
    ChannelMismatch { expected: u16, actual: u16 },

    #[error("Invalid parameter: {0}")]
    InvalidParam(String),
}

pub type Result<T> = std::result::Result<T, PhononError>;

/// Core audio buffer holding floating point samples.
///
/// Internal representation is always `Vec<f32>` with range `[-1.0, 1.0]`.
///
/// # Example
///
/// ```
/// use phonon_core::AudioBuffer;
///
/// let buffer = AudioBuffer {
///     samples: vec![0.0; 44100 * 2],
///     sample_rate: 44100,
///     channels: 2,
/// };
/// assert!((buffer.duration_secs() - 1.0).abs() < 1e-6);
/// ```
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Audio samples. Interleaved for multi-channel.
    pub samples: Vec<f32>,

    /// Sample rate in Hz (e.g. 44100).
    pub sample_rate: u32,

    /// Number of channels (1 = Mono, 2 = Stereo).
    pub channels: u16,
}

impl AudioBuffer {
    /// Create a new empty AudioBuffer
    pub fn new(sample_rate: u32, channels: u16, capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            sample_rate,
            channels,
        }
    }

    /// Duration in seconds
    pub fn duration_secs(&self) -> f64 {
        if self.channels == 0 || self.sample_rate == 0 {
            return 0.0;
        }
        (self.samples.len() as f64) / (self.channels as f64 * self.sample_rate as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_buffer_duration() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 44100 * 2], // 1 second of stereo audio
            sample_rate: 44100,
            channels: 2,
        };
        assert!((buffer.duration_secs() - 1.0).abs() < 1e-6);
    }
}
