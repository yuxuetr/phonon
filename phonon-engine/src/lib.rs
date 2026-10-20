use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use phonon_core::AudioBuffer;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{error, info, instrument};

pub mod effects;
pub mod stream;
pub use effects::*;
pub use stream::{convert_for_device, play_stream};

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
///
/// The audio callback only ever `try_lock`s the mutexes, so it never waits
/// on the control thread; control methods keep their critical sections to
/// a swap or push and do allocation/deallocation outside the lock.
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

    // Copy outside the lock; swap inside; free the old track after unlocking.
    let new_samples = buffer.samples.clone();
    let _old_samples = {
      let mut samples = self
        .state
        .samples
        .lock()
        .map_err(|e| EngineError::Play(format!("Lock poisoned: {e}")))?;
      std::mem::replace(&mut *samples, new_samples)
    };

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
    let total = self.state.total_samples.load(Ordering::Relaxed);
    let idx = seek_index(
      position_secs,
      self.device_sample_rate,
      self.device_channels,
      total,
    );
    self.state.position.store(idx, Ordering::SeqCst);
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
    // Take the Vec out so it is freed after the lock is released.
    let _old_samples = self
      .state
      .samples
      .lock()
      .map(|mut s| std::mem::take(&mut *s));
    self.state.total_samples.store(0, Ordering::SeqCst);
  }

  /// Adds a [`phonon_core::DspPlugin`] to the playback effect chain.
  pub fn add_plugin(&self, plugin: Box<dyn phonon_core::DspPlugin>) {
    if let Ok(mut chain) = self.state.effect_chain.lock() {
      chain.push(plugin);
    }
  }

  /// Removes all effects from the playback chain.
  pub fn clear_effects(&self) {
    // Drop the effects after the lock is released.
    let _old_chain = self
      .state
      .effect_chain
      .lock()
      .map(|mut chain| std::mem::take(&mut *chain));
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

/// Interleaved sample index for `secs`, clamped to `total` and rounded down
/// to a frame boundary (a mid-frame position would swap channels).
fn seek_index(secs: f64, sample_rate: u32, channels: u16, total: usize) -> usize {
  let ch = (channels as usize).max(1);
  let idx = ((secs.max(0.0) * sample_rate as f64) as usize).saturating_mul(ch);
  let clamped = idx.min(total);
  clamped - clamped % ch
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
  // Never wait on the control thread: if it holds the lock (or it is
  // poisoned), output silence for this block without advancing.
  let Ok(samples) = state.samples.try_lock() else {
    output.fill(0.0);
    return;
  };

  let total = samples.len();
  let start_pos = state.position.load(Ordering::Relaxed);
  let mut pos = start_pos;

  for frame in output.iter_mut() {
    if pos < total {
      *frame = samples[pos] * volume;
      pos += 1;
    } else {
      *frame = 0.0;
    }
  }

  drop(samples);

  // Only advance if no seek/stop/play changed the position meanwhile.
  let _ = state
    .position
    .compare_exchange(start_pos, pos, Ordering::AcqRel, Ordering::Relaxed);

  // Apply real-time effects; skip this block if the chain is being edited.
  if let Ok(mut chain) = state.effect_chain.try_lock()
    && !chain.is_empty()
  {
    chain.process(output, state.sample_rate, state.channels);
  }

  // Auto-stop when finished (unless streaming — more data may arrive)
  if pos >= total && !state.streaming.load(Ordering::Relaxed) {
    state.active.store(false, Ordering::Relaxed);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn loaded_state(samples: Vec<f32>) -> SharedState {
    let state = SharedState::new(48_000, 2);
    state.total_samples.store(samples.len(), Ordering::SeqCst);
    if let Ok(mut s) = state.samples.lock() {
      *s = samples;
    }
    state.active.store(true, Ordering::SeqCst);
    state
  }

  /// Runs `f` on another thread and fails if it doesn't finish in time,
  /// so a callback that blocks fails the test instead of hanging it.
  fn finishes_within<F: FnOnce() + Send + 'static>(f: F) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      f();
      let _ = tx.send(());
    });
    rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok()
  }

  #[test]
  fn test_callback_does_not_block_on_held_sample_lock() {
    let state = Arc::new(loaded_state(vec![0.5; 64]));
    let guard_state = state.clone();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    // Control thread holds the sample lock until told to release it.
    let holder = std::thread::spawn(move || {
      let _guard = guard_state.samples.lock();
      let _ = locked_tx.send(());
      let _ = release_rx.recv();
    });
    locked_rx.recv().unwrap();

    let cb_state = state.clone();
    let done = finishes_within(move || {
      let mut out = [1.0f32; 8];
      write_data(&mut out, &cb_state);
      assert!(out.iter().all(|&s| s == 0.0), "contended block is silence");
    });
    let _ = release_tx.send(());
    holder.join().unwrap();
    assert!(done, "audio callback blocked on the sample lock");
    // Position did not advance during the skipped block.
    assert_eq!(state.position.load(Ordering::SeqCst), 0);
  }

  #[test]
  fn test_callback_skips_effects_when_chain_lock_held() {
    let state = Arc::new(loaded_state(vec![0.5; 64]));
    let _chain = state.effect_chain.lock().unwrap();
    let cb_state = state.clone();
    let done = finishes_within(move || {
      let mut out = [0.0f32; 8];
      write_data(&mut out, &cb_state);
      assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    });
    assert!(done, "audio callback blocked on the effect-chain lock");
  }

  #[test]
  fn test_seek_index_is_frame_aligned() {
    // 48 kHz stereo: 0.5 / 48000 s lands mid-frame (sample 1).
    assert_eq!(seek_index(0.5 / 48_000.0 + 1e-12, 48_000, 2, 1_000), 0);
    assert_eq!(seek_index(1.5 / 48_000.0, 48_000, 2, 1_000), 2);
    // Clamped to the end, still aligned.
    assert_eq!(seek_index(10.0, 48_000, 2, 1_001), 1_000);
  }
}
