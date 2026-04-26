//! Shared application state types and helpers.

use dioxus::prelude::*;
use phonon_core::{AudioBuffer, PluginRegistry, algorithms};
use phonon_engine::AudioPlayer;
use std::sync::Arc;

/// Lightweight display metadata (no raw samples — safe to clone on every render).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDisplayInfo {
  pub path: String,
  pub sample_rate: u32,
  pub channels: u16,
  pub duration: f64,
  pub sample_count: usize,
  pub waveform_path: String,
  pub rms: f32,
  pub peak: f32,
  pub peak_dbfs: f32,
  /// Pre-computed spectrum bins (magnitudes for first channel, downsampled to 64 bars).
  pub spectrum_bars: Vec<f32>,
}

/// Effect parameters controlled by the user.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectParams {
  pub normalize_target: f32,
  pub fade_in_secs: f64,
  pub fade_out_secs: f64,
  pub speed_factor: f64,
  pub compress_preset: String,
  /// EQ preset name ("none", "voice", "bass_heavy", "bright", "custom").
  pub eq_preset: String,
  /// Custom EQ band gains in dB (5 bands: sub, low, mid, high-mid, high).
  pub eq_gains: [f64; 5],
}

impl Default for EffectParams {
  fn default() -> Self {
    Self {
      normalize_target: 0.9,
      fade_in_secs: 0.0,
      fade_out_secs: 0.0,
      speed_factor: 1.0,
      compress_preset: "none".to_string(),
      eq_preset: "none".to_string(),
      eq_gains: [0.0; 5],
    }
  }
}

/// Which visualisation tab is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizMode {
  Waveform,
  Spectrum,
}

/// Sidebar panel visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanel {
  None,
  Effects,
  Eq,
  Export,
  Plugins,
}

/// Tracks an active plugin instance in the effect chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivePluginState {
  /// Plugin ID (e.g. `"builtin.volume"`).
  pub id: String,
  /// Human-readable name.
  pub name: String,
  /// Current parameter values: `(name, current_value, min, max, default)`.
  pub params: Vec<(String, f32, f32, f32, f32)>,
}

/// Global application state provided via context.
#[derive(Clone, Copy)]
pub struct AppState {
  pub display_info: Signal<Option<AudioDisplayInfo>>,
  pub audio_buffer: Signal<Option<Arc<AudioBuffer>>>,
  pub status_msg: Signal<String>,
  pub player_state: Signal<Option<AudioPlayer>>,
  pub effects: Signal<EffectParams>,
  pub side_panel: Signal<SidePanel>,
  pub export_bitrate: Signal<u32>,
  pub viz_mode: Signal<VizMode>,
  pub volume: Signal<f32>,
  pub plugin_registry: Signal<Arc<PluginRegistry>>,
  pub active_plugins: Signal<Vec<ActivePluginState>>,
}

impl AppState {
  pub fn init() -> Self {
    Self {
      display_info: Signal::new(None),
      audio_buffer: Signal::new(None),
      status_msg: Signal::new("Ready".to_string()),
      player_state: Signal::new(None),
      effects: Signal::new(EffectParams::default()),
      side_panel: Signal::new(SidePanel::None),
      export_bitrate: Signal::new(192),
      viz_mode: Signal::new(VizMode::Waveform),
      volume: Signal::new(1.0),
      plugin_registry: Signal::new(Arc::new(PluginRegistry::with_builtins())),
      active_plugins: Signal::new(Vec::new()),
    }
  }
}

/// Downsamples audio data for waveform visualization.
pub fn generate_waveform_path(samples: &[f32], width: f32, height: f32) -> String {
  if samples.is_empty() {
    return String::new();
  }

  let samples_per_pixel = (samples.len() as f32 / width).ceil() as usize;
  let step = samples_per_pixel.max(1);
  let mid_y = height / 2.0;
  let scale_y = height / 2.0;

  let mut path = String::with_capacity(width as usize * 20);
  path.push_str(&format!("M 0 {}", mid_y));

  for x in 0..width as usize {
    let start = x * step;
    let end = (start + step).min(samples.len());
    if start >= samples.len() {
      break;
    }

    let mut max_val = 0.0f32;
    for i in start..end {
      if let Some(s) = samples.get(i) {
        let abs = s.abs();
        if abs > max_val {
          max_val = abs;
        }
      }
    }
    let y = mid_y - (max_val * scale_y);
    path.push_str(&format!(" L {} {:.2}", x, y));
  }

  for x in (0..width as usize).rev() {
    let start = x * step;
    let end = (start + step).min(samples.len());
    if start >= samples.len() {
      continue;
    }

    let mut max_val = 0.0f32;
    for i in start..end {
      if let Some(s) = samples.get(i) {
        let abs = s.abs();
        if abs > max_val {
          max_val = abs;
        }
      }
    }
    let y = mid_y + (max_val * scale_y);
    path.push_str(&format!(" L {} {:.2}", x, y));
  }

  path.push_str(" Z");
  path
}

/// Computes spectrum bars (downsampled to `num_bars` buckets).
pub fn compute_spectrum_bars(buffer: &AudioBuffer, num_bars: usize) -> Vec<f32> {
  let spectra = match algorithms::compute_spectrum(buffer) {
    Ok(s) if !s.is_empty() => s,
    _ => return vec![0.0; num_bars],
  };

  let mags = &spectra[0];
  if mags.is_empty() {
    return vec![0.0; num_bars];
  }

  // Use logarithmic bucketing for a perceptually-even bar chart
  let useful_bins = mags.len(); // 0..nyquist
  let mut bars = Vec::with_capacity(num_bars);

  for bar_idx in 0..num_bars {
    // Logarithmic mapping: low bars cover fewer bins, high bars cover more
    let lo = ((bar_idx as f64 / num_bars as f64).powi(2) * useful_bins as f64) as usize;
    let hi = (((bar_idx + 1) as f64 / num_bars as f64).powi(2) * useful_bins as f64) as usize;
    let lo = lo.min(useful_bins);
    let hi = hi.max(lo + 1).min(useful_bins);

    let sum: f32 = mags[lo..hi].iter().sum();
    let avg = sum / (hi - lo) as f32;
    bars.push(avg);
  }

  // Normalise to 0..1 range
  let max_bar = bars.iter().fold(0.0f32, |a, &b| a.max(b));
  if max_bar > 0.0 {
    for b in &mut bars {
      *b /= max_bar;
    }
  }
  bars
}

/// Builds an `AudioDisplayInfo` from a loaded buffer.
pub fn build_display_info(buffer: &AudioBuffer, path: &str) -> AudioDisplayInfo {
  let waveform = generate_waveform_path(&buffer.samples, 1000.0, 200.0);
  let spectrum_bars = compute_spectrum_bars(buffer, 64);
  AudioDisplayInfo {
    path: path.to_string(),
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
    duration: buffer.duration_secs(),
    sample_count: buffer.samples.len(),
    waveform_path: waveform,
    rms: algorithms::rms(buffer),
    peak: algorithms::peak(buffer),
    peak_dbfs: algorithms::peak_dbfs(buffer),
    spectrum_bars,
  }
}
