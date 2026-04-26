#[cfg(feature = "simd")]
pub(crate) mod simd_ops;

pub mod channels;
pub mod compressor;
pub mod crossfade;
pub mod eq;
pub mod fade;
pub mod filters;
pub mod generators;
pub mod loudness;
pub mod lufs;
pub mod merge;
pub mod mix;
pub mod normalize;
pub mod overlay;
pub mod repeat;
pub mod resample;
pub mod reverse;
pub mod silence;
pub mod spectrum;
pub mod speed;
pub mod trim;
pub mod volume;

pub use channels::ensure_channels;
pub use compressor::{CompressorConfig, compress, with_compression};
pub use crossfade::crossfade;
pub use eq::{EqBand, ParametricEq, apply_eq, with_eq};
pub use fade::{fade_in, fade_out};
pub use filters::{FilterType, filter};
pub use generators::{Waveform, generate_beep, generate_signal, generate_tone};
pub use loudness::{adjust_volume_db, dbfs, peak, peak_dbfs, rms, with_volume_db};
pub use lufs::{
  LoudnessReport, loudness_integrated, loudness_momentary_max, loudness_range,
  loudness_short_term_max, measure_loudness, normalize_lufs, true_peak, true_peak_dbtp,
  with_lufs_normalized,
};
pub use merge::merge;
pub use mix::mix;
pub use normalize::normalize;
pub use overlay::overlay_at_position;
pub use repeat::repeat;
pub use resample::{resample, resample_linear, resample_sinc};
pub use reverse::reverse;
pub use silence::silence;
pub use spectrum::compute_spectrum;
pub use speed::change_speed;
pub use trim::trim;
pub use volume::{adjust_volume, with_volume};
