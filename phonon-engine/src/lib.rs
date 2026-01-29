use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_queue::SegQueue;
use phonon_core::AudioBuffer;
use std::sync::Arc;
use thiserror::Error;

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

pub struct AudioPlayer {
    _stream: cpal::Stream,
    queue: Arc<SegQueue<f32>>,
    device_sample_rate: u32,
    device_channels: u16,
}

impl AudioPlayer {
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

        let queue = Arc::new(SegQueue::new());
        let queue_clone = queue.clone();

        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    write_data(data, &queue_clone)
                },
                err_fn,
                None,
            ),
            _ => return Err(EngineError::Config("Unsupported sample format (only F32 supported for now)".to_string())),
        }
        .map_err(|e| EngineError::Device(e.to_string()))?;

        stream.play().map_err(|e| EngineError::Play(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            queue,
            device_sample_rate: sample_rate,
            device_channels: channels,
        })
    }

    /// Appends audio data to the playback queue.
    ///
    /// Note: Currently requires the input buffer to match the device's sample rate and channel count.
    /// If they don't match, this function returns an error.
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

        for &sample in &buffer.samples {
            self.queue.push(sample);
        }

        Ok(())
    }
    
    /// Clears the playback queue (Stop/Skip).
    pub fn clear(&self) {
        // Pop everything until empty
        while self.queue.pop().is_some() {}
    }

    pub fn device_sample_rate(&self) -> u32 {
        self.device_sample_rate
    }

    pub fn device_channels(&self) -> u16 {
        self.device_channels
    }
}

fn write_data(output: &mut [f32], queue: &SegQueue<f32>) {
    for frame in output.iter_mut() {
        if let Some(sample) = queue.pop() {
            *frame = sample;
        } else {
            *frame = 0.0; // Silence
        }
    }
}
