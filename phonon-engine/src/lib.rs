use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use phonon_core::AudioBuffer;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{error, info, instrument};

pub mod effects;
pub use effects::*;

#[derive(Error, Debug)]
pub enum EngineError {
  #[error("Audio device error: {0}")]
  Device(String),
  #[error("Stream configuration error: {0}")]
  Config(String),
  #[error("Play error: {0}")]
  Play(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
  /// No audio loaded or playback finished.
  Stopped,
  /// Actively playing audio.
  Playing,
  /// Playback paused; can be resumed.
  Paused,
}

/// Shared state between the audio thread and the control API.
struct SharedState {
  /// Audio samples buffer (the complete track).
  samples: Mutex<Vec<f32>>,
  /// Current playback position (sample index).
  position: AtomicUsize,
  /// Total number of samples.
  total_samples: AtomicUsize,
  /// Whether playback is paused.
  paused: AtomicBool,
  /// Whether any audio is loaded and playing/paused.
  active: AtomicBool,
  /// Volume multiplier stored as bits (f32 → u32 bits).
  volume_bits: AtomicU32,
  /// When true, do not auto-stop when reaching the end (more samples may arrive).
  streaming: AtomicBool,
  /// Real-time effect chain applied during playback.
  effect_chain: Mutex<EffectChain>,
  /// Sample rate for effect processing.
  sample_rate: u32,
  /// Channel count for effect processing.
  channels: u16,
}

use std::sync::atomic::AtomicU32;

impl SharedState {
  fn new(sample_rate: u32, channels: u16) -> Self {
    Self {
      samples: Mutex::new(Vec::new()),
      position: AtomicUsize::new(0),
      total_samples: AtomicUsize::new(0),
      paused: AtomicBool::new(false),
      active: AtomicBool::new(false),
      volume_bits: AtomicU32::new(f32::to_bits(1.0)),
      streaming: AtomicBool::new(false),
      effect_chain: Mutex::new(EffectChain::new()),
      sample_rate,
      channels,
    }
  }

  fn volume(&self) -> f32 {
    f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
  }

  fn set_volume(&self, vol: f32) {
    self
      .volume_bits
      .store(vol.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
  }
}

/// Audio player with pause/resume/seek/volume controls.
///
/// Plays audio through the default output device.
pub struct AudioPlayer {
  _stream: cpal::Stream,
  state: Arc<SharedState>,
  device_sample_rate: u32,
  device_channels: u16,
}

impl AudioPlayer {
  /// Creates a new AudioPlayer using the default output device.
  #[instrument]
  pub fn new() -> Result<Self> {
    let host = cpal::default_host();
    let device = host
      .default_output_device()
      .ok_or_else(|| EngineError::Device("No output device available".to_string()))?;

    let config = device
      .default_output_config()
      .map_err(|e| EngineError::Config(e.to_string()))?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    info!(sample_rate, channels, "Initialized audio device");

    let state = Arc::new(SharedState::new(sample_rate, channels));
    let state_clone = state.clone();

    let err_fn = |err| error!("an error occurred on stream: {}", err);

    let stream = match config.sample_format() {
      cpal::SampleFormat::F32 => device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| write_data(data, &state_clone),
        err_fn,
        None,
      ),
      _ => {
        return Err(EngineError::Config(
          "Unsupported sample format (only F32 supported for now)".to_string(),
        ));
      }
    }
    .map_err(|e| EngineError::Device(e.to_string()))?;

    stream
      .play()
      .map_err(|e| EngineError::Play(e.to_string()))?;

    Ok(Self {
      _stream: stream,
      state,
      device_sample_rate: sample_rate,
      device_channels: channels,
    })
  }

  /// Loads and starts playing an `AudioBuffer`.
  ///
  /// If audio is currently playing, it is replaced.
  #[instrument(skip(self, buffer), fields(buffer_duration = buffer.duration_secs()))]
  pub fn play(&self, buffer: &AudioBuffer) -> Result<()> {
    if buffer.sample_rate != self.device_sample_rate {
      return Err(EngineError::Config(format!(
        "Sample rate mismatch: Device is {}Hz, Buffer is {}Hz",
        self.device_sample_rate, buffer.sample_rate
      )));
    }

    if buffer.channels != self.device_channels {
      return Err(EngineError::Config(format!(
        "Channel count mismatch: Device is {}, Buffer is {}",
        self.device_channels, buffer.channels
      )));
    }

    info!("Loading audio for playback");

    {
      let mut samples = self
        .state
        .samples
        .lock()
        .map_err(|e| EngineError::Play(format!("Lock poisoned: {e}")))?;
      *samples = buffer.samples.clone();
    }

    self
      .state
      .total_samples
      .store(buffer.samples.len(), Ordering::SeqCst);
    self.state.position.store(0, Ordering::SeqCst);
    self.state.paused.store(false, Ordering::SeqCst);
    self.state.active.store(true, Ordering::SeqCst);

    Ok(())
  }

  /// Pauses playback. Call `resume()` to continue.
  pub fn pause(&self) {
    self.state.paused.store(true, Ordering::SeqCst);
  }

  /// Resumes playback after a pause.
  pub fn resume(&self) {
    self.state.paused.store(false, Ordering::SeqCst);
  }

  /// Stops playback and resets position to the beginning.
  pub fn stop(&self) {
    self.state.active.store(false, Ordering::SeqCst);
    self.state.paused.store(false, Ordering::SeqCst);
    self.state.position.store(0, Ordering::SeqCst);
  }

  /// Seeks to a position in seconds from the start.
  pub fn seek(&self, position_secs: f64) -> Result<()> {
    let sample_idx =
      (position_secs * self.device_sample_rate as f64 * self.device_channels as f64) as usize;
    let total = self.state.total_samples.load(Ordering::Relaxed);
    let clamped = sample_idx.min(total);
    self.state.position.store(clamped, Ordering::SeqCst);
    Ok(())
  }

  /// Sets the playback volume (0.0 = mute, 1.0 = normal, 2.0 = 2× boost).
  pub fn set_volume(&self, volume: f32) {
    self.state.set_volume(volume);
  }

  /// Returns the current volume.
  pub fn volume(&self) -> f32 {
    self.state.volume()
  }

  /// Returns the current playback state.
  pub fn playback_state(&self) -> PlaybackState {
    if !self.state.active.load(Ordering::Relaxed) {
      return PlaybackState::Stopped;
    }
    if self.state.paused.load(Ordering::Relaxed) {
      PlaybackState::Paused
    } else {
      PlaybackState::Playing
    }
  }

  /// Returns the current playback position in seconds.
  pub fn position_secs(&self) -> f64 {
    let pos = self.state.position.load(Ordering::Relaxed);
    let divisor = self.device_sample_rate as f64 * self.device_channels as f64;
    if divisor > 0.0 {
      pos as f64 / divisor
    } else {
      0.0
    }
  }

  /// Returns the total duration of the loaded audio in seconds.
  pub fn duration_secs(&self) -> f64 {
    let total = self.state.total_samples.load(Ordering::Relaxed);
    let divisor = self.device_sample_rate as f64 * self.device_channels as f64;
    if divisor > 0.0 {
      total as f64 / divisor
    } else {
      0.0
    }
  }

  /// Returns the playback progress as a fraction (0.0 – 1.0).
  pub fn progress(&self) -> f64 {
    let total = self.state.total_samples.load(Ordering::Relaxed);
    if total == 0 {
      return 0.0;
    }
    let pos = self.state.position.load(Ordering::Relaxed);
    pos as f64 / total as f64
  }

  /// Returns `true` if playback has reached the end.
  pub fn is_finished(&self) -> bool {
    let total = self.state.total_samples.load(Ordering::Relaxed);
    let pos = self.state.position.load(Ordering::Relaxed);
    total > 0 && pos >= total
  }

  /// Appends new audio samples to the playback buffer.
  ///
  /// This enables streaming playback: start playing immediately and keep
  /// appending new chunks as they arrive from a TTS provider.
  ///
  /// If the player is not yet active, it will start playing automatically.
  pub fn append_samples(&self, new_samples: &[f32]) -> Result<()> {
    if new_samples.is_empty() {
      return Ok(());
    }

    {
      let mut samples = self
        .state
        .samples
        .lock()
        .map_err(|e| EngineError::Play(format!("Lock poisoned: {e}")))?;
      samples.extend_from_slice(new_samples);
      self
        .state
        .total_samples
        .store(samples.len(), Ordering::SeqCst);
    }

    // Enable streaming mode and auto-activate
    self.state.streaming.store(true, Ordering::SeqCst);
    if !self.state.active.load(Ordering::Relaxed) {
      self.state.paused.store(false, Ordering::SeqCst);
      self.state.active.store(true, Ordering::SeqCst);
    }

    Ok(())
  }

  /// Signals that no more samples will be appended.
  ///
  /// After calling this, the player will auto-stop when playback reaches
  /// the end of the buffer.
  pub fn finish_streaming(&self) {
    self.state.streaming.store(false, Ordering::SeqCst);
  }

  /// Clears the loaded audio buffer.
  pub fn clear(&self) {
    self.stop();
    if let Ok(mut samples) = self.state.samples.lock() {
      samples.clear();
    }
    self.state.total_samples.store(0, Ordering::SeqCst);
  }

  /// Adds a real-time audio effect to the playback chain.
  pub fn add_effect(&self, effect: Box<dyn AudioEffect>) {
    if let Ok(mut chain) = self.state.effect_chain.lock() {
      chain.push(effect);
    }
  }

  /// Adds a [`phonon_core::DspPlugin`] to the playback effect chain.
  pub fn add_plugin(&self, plugin: Box<dyn phonon_core::DspPlugin>) {
    if let Ok(mut chain) = self.state.effect_chain.lock() {
      chain.push_plugin(plugin);
    }
  }

  /// Removes all effects from the playback chain.
  pub fn clear_effects(&self) {
    if let Ok(mut chain) = self.state.effect_chain.lock() {
      chain.clear();
    }
  }

  /// Returns the number of active effects.
  pub fn effect_count(&self) -> usize {
    self.state.effect_chain.lock().map(|c| c.len()).unwrap_or(0)
  }

  pub fn device_sample_rate(&self) -> u32 {
    self.device_sample_rate
  }

  pub fn device_channels(&self) -> u16 {
    self.device_channels
  }
}

/// Audio callback: reads from shared state into the output buffer.
fn write_data(output: &mut [f32], state: &SharedState) {
  // If not active or paused, fill with silence.
  if !state.active.load(Ordering::Relaxed) || state.paused.load(Ordering::Relaxed) {
    for frame in output.iter_mut() {
      *frame = 0.0;
    }
    return;
  }

  let volume = state.volume();
  let samples = match state.samples.lock() {
    Ok(s) => s,
    Err(_) => {
      // Poisoned lock — fill silence
      for frame in output.iter_mut() {
        *frame = 0.0;
      }
      return;
    }
  };

  let total = samples.len();
  let mut pos = state.position.load(Ordering::Relaxed);

  for frame in output.iter_mut() {
    if pos < total {
      *frame = samples[pos] * volume;
      pos += 1;
    } else {
      *frame = 0.0;
    }
  }

  state.position.store(pos, Ordering::Relaxed);

  // Apply real-time effects
  if let Ok(mut chain) = state.effect_chain.lock() {
    if !chain.is_empty() {
      chain.process(output, state.sample_rate, state.channels);
    }
  }

  // Auto-stop when finished (unless streaming — more data may arrive)
  if pos >= total && !state.streaming.load(Ordering::Relaxed) {
    state.active.store(false, Ordering::Relaxed);
  }
}
