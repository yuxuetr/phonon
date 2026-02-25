use phonon_core::AudioBuffer;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// A single audio processing operation, deserialized from JSON.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
  Normalize,
  Reverse,
  FadeIn { duration_secs: f64 },
  FadeOut { duration_secs: f64 },
  Volume { factor: f32 },
  VolumeDb { db: f32 },
  Speed { factor: f64 },
  Trim { start_secs: f64, end_secs: f64 },
  Silence { duration_secs: f64 },
  Eq { bands: Vec<EqBandConfig> },
}

#[derive(Debug, Deserialize)]
struct EqBandConfig {
  filter_type: String,
  freq_hz: f64,
  q: f64,
  gain_db: f64,
}

fn parse_filter_type(s: &str) -> std::result::Result<phonon_core::FilterType, String> {
  match s.to_lowercase().as_str() {
    "lowpass" | "low_pass" => Ok(phonon_core::FilterType::LowPass),
    "highpass" | "high_pass" => Ok(phonon_core::FilterType::HighPass),
    "bandpass" | "band_pass" => Ok(phonon_core::FilterType::BandPass),
    "notch" => Ok(phonon_core::FilterType::Notch),
    "peaking" | "peak" => Ok(phonon_core::FilterType::Peaking),
    "lowshelf" | "low_shelf" => Ok(phonon_core::FilterType::LowShelf),
    "highshelf" | "high_shelf" => Ok(phonon_core::FilterType::HighShelf),
    other => Err(format!("unknown filter type: {other}")),
  }
}

/// Apply a chain of audio processing operations.
///
/// # Arguments
/// * `samples` — interleaved f32 audio samples
/// * `sample_rate` — sample rate in Hz
/// * `channels` — number of channels (1 = mono, 2 = stereo)
/// * `ops_json` — JSON array of operation objects, e.g.
///   `[{"op":"normalize"},{"op":"fade_in","duration_secs":0.5}]`
///
/// # Returns
/// Processed samples as a `Float32Array`.
#[wasm_bindgen]
pub fn process_audio(
  samples: &[f32],
  sample_rate: u32,
  channels: u16,
  ops_json: &str,
) -> std::result::Result<Vec<f32>, JsError> {
  let ops: Vec<Operation> =
    serde_json::from_str(ops_json).map_err(|e| JsError::new(&format!("Invalid ops JSON: {e}")))?;

  let mut buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };

  for op in &ops {
    buffer = apply_operation(buffer, op).map_err(|e| JsError::new(&e))?;
  }

  Ok(buffer.samples)
}

/// Normalize audio to peak amplitude.
#[wasm_bindgen]
pub fn normalize(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32> {
  let mut buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::normalize(&mut buffer, 1.0);
  buffer.samples
}

/// Reverse audio.
#[wasm_bindgen]
pub fn reverse(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32> {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::reverse(&buffer).samples
}

/// Apply a fade-in effect.
#[wasm_bindgen]
pub fn fade_in(samples: &[f32], sample_rate: u32, channels: u16, duration_secs: f64) -> Vec<f32> {
  let mut buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::fade_in(&mut buffer, duration_secs);
  buffer.samples
}

/// Apply a fade-out effect.
#[wasm_bindgen]
pub fn fade_out(samples: &[f32], sample_rate: u32, channels: u16, duration_secs: f64) -> Vec<f32> {
  let mut buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::fade_out(&mut buffer, duration_secs);
  buffer.samples
}

/// Adjust volume by a linear factor.
#[wasm_bindgen]
pub fn adjust_volume(samples: &[f32], sample_rate: u32, channels: u16, factor: f32) -> Vec<f32> {
  let mut buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::adjust_volume(&mut buffer, factor);
  buffer.samples
}

/// Change playback speed.
#[wasm_bindgen]
pub fn change_speed(
  samples: &[f32],
  sample_rate: u32,
  channels: u16,
  factor: f64,
) -> std::result::Result<Vec<f32>, JsError> {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  let result =
    phonon_core::change_speed(&buffer, factor).map_err(|e| JsError::new(&format!("{e}")))?;
  Ok(result.samples)
}

/// Compute spectrum magnitudes (for visualization).
///
/// Returns `num_bins` magnitude values by averaging the full FFT output
/// of the first channel into the requested number of bins.
#[wasm_bindgen]
pub fn compute_spectrum(
  samples: &[f32],
  sample_rate: u32,
  channels: u16,
  num_bins: usize,
) -> Vec<f32> {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  let spectra = match phonon_core::compute_spectrum(&buffer) {
    Ok(s) => s,
    Err(_) => return vec![0.0; num_bins],
  };
  if spectra.is_empty() {
    return vec![0.0; num_bins];
  }
  let mags = &spectra[0];
  if mags.is_empty() || num_bins == 0 {
    return vec![0.0; num_bins];
  }
  // Downsample FFT bins into num_bins by averaging
  let bin_size = mags.len() as f64 / num_bins as f64;
  (0..num_bins)
    .map(|i| {
      let start = (i as f64 * bin_size) as usize;
      let end = (((i + 1) as f64) * bin_size) as usize;
      let end = end.min(mags.len());
      if start >= end {
        0.0
      } else {
        mags[start..end].iter().sum::<f32>() / (end - start) as f32
      }
    })
    .collect()
}

/// Get audio duration in seconds.
#[wasm_bindgen]
pub fn duration_secs(num_samples: usize, sample_rate: u32, channels: u16) -> f64 {
  if channels == 0 || sample_rate == 0 {
    return 0.0;
  }
  (num_samples as f64) / (channels as f64 * sample_rate as f64)
}

/// Get RMS level.
#[wasm_bindgen]
pub fn rms(samples: &[f32], sample_rate: u32, channels: u16) -> f32 {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::rms(&buffer)
}

/// Get peak amplitude.
#[wasm_bindgen]
pub fn peak(samples: &[f32], sample_rate: u32, channels: u16) -> f32 {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::peak(&buffer)
}

/// Get peak level in dBFS.
#[wasm_bindgen]
pub fn peak_dbfs(samples: &[f32], sample_rate: u32, channels: u16) -> f32 {
  let buffer = AudioBuffer {
    samples: samples.to_vec(),
    sample_rate,
    channels,
  };
  phonon_core::peak_dbfs(&buffer)
}

fn apply_operation(
  mut buffer: AudioBuffer,
  op: &Operation,
) -> std::result::Result<AudioBuffer, String> {
  match op {
    Operation::Normalize => {
      phonon_core::normalize(&mut buffer, 1.0);
    }
    Operation::Reverse => {
      buffer = phonon_core::reverse(&buffer);
    }
    Operation::FadeIn { duration_secs } => {
      phonon_core::fade_in(&mut buffer, *duration_secs);
    }
    Operation::FadeOut { duration_secs } => {
      phonon_core::fade_out(&mut buffer, *duration_secs);
    }
    Operation::Volume { factor } => {
      phonon_core::adjust_volume(&mut buffer, *factor);
    }
    Operation::VolumeDb { db } => {
      phonon_core::adjust_volume_db(&mut buffer, *db);
    }
    Operation::Speed { factor } => {
      buffer = phonon_core::change_speed(&buffer, *factor).map_err(|e| format!("{e}"))?;
    }
    Operation::Trim {
      start_secs,
      end_secs,
    } => {
      buffer = phonon_core::trim(&buffer, *start_secs, *end_secs).map_err(|e| format!("{e}"))?;
    }
    Operation::Silence { duration_secs } => {
      buffer = phonon_core::silence(*duration_secs, buffer.sample_rate, buffer.channels)
        .map_err(|e| format!("{e}"))?;
    }
    Operation::Eq { bands } => {
      let eq_bands: std::result::Result<Vec<phonon_core::EqBand>, String> = bands
        .iter()
        .map(|b| {
          let ft = parse_filter_type(&b.filter_type)?;
          Ok(phonon_core::EqBand {
            filter_type: ft,
            freq_hz: b.freq_hz,
            q: b.q,
            gain_db: b.gain_db,
          })
        })
        .collect();
      phonon_core::apply_eq(&mut buffer, &eq_bands?);
    }
  }
  Ok(buffer)
}
