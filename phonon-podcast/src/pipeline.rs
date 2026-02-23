use crate::config::PipelineConfig;
use crate::script::PodcastScript;
use crate::PodcastError;
use phonon_ai::TtsProvider;
use phonon_core::{self, AudioBuffer};
use std::path::Path;
use tracing::{info, instrument};

/// The podcast generation pipeline.
///
/// Orchestrates TTS synthesis, audio processing, and assembly
/// to produce a complete podcast episode from a script.
pub struct PodcastPipeline<T: TtsProvider> {
    tts: T,
}

impl<T: TtsProvider> PodcastPipeline<T> {
    /// Creates a new pipeline with the given TTS provider.
    pub fn new(tts: T) -> Self {
        Self { tts }
    }

    /// Generates a complete podcast episode as an `AudioBuffer`.
    ///
    /// # Pipeline Steps
    ///
    /// 1. Synthesize each segment via TTS
    /// 2. Post-process each segment (normalize)
    /// 3. Assemble segments with pauses/crossfades
    /// 4. Add intro/outro if configured
    /// 5. Mix with BGM if configured
    /// 6. Final normalize + fade in/out
    #[instrument(skip(self, script), fields(title = %script.title, segments = script.segment_count()))]
    pub async fn generate(&self, script: &PodcastScript) -> Result<AudioBuffer, PodcastError> {
        let config = &script.config;
        let target_rate = config.output_sample_rate;
        let target_channels = config.output_channels;

        info!("Starting podcast generation");

        // Step 1: Synthesize all segments
        let mut segment_buffers = Vec::with_capacity(script.segments.len());
        for (i, segment) in script.segments.iter().enumerate() {
            let voice = script
                .speakers
                .get(&segment.speaker)
                .ok_or_else(|| {
                    PodcastError::Script(format!("Unknown speaker: {}", segment.speaker))
                })?;

            info!(segment = i, speaker = %segment.speaker, "Synthesizing segment");

            let buffer = self
                .tts
                .synthesize(&segment.text, voice)
                .await
                .map_err(|e| PodcastError::Tts(e.to_string()))?;

            segment_buffers.push(buffer);
        }

        info!(count = segment_buffers.len(), "All segments synthesized");

        // Step 2: Post-process each segment
        let segment_buffers = self.post_process_segments(segment_buffers, target_rate, target_channels)?;

        // Step 3: Assemble segments with pauses/crossfades
        let mut assembled = self.assemble_segments(segment_buffers, config)?;

        // Step 4: Add intro/outro
        assembled = self.add_intro_outro(assembled, config, target_rate, target_channels)?;

        // Step 5: Mix with BGM
        assembled = self.mix_bgm(assembled, config, target_rate, target_channels)?;

        // Step 6: Final normalize + fade
        phonon_core::normalize(&mut assembled, config.normalize_target);

        if config.fade_in_secs > 0.0 {
            phonon_core::fade_in(&mut assembled, config.fade_in_secs);
        }
        if config.fade_out_secs > 0.0 {
            phonon_core::fade_out(&mut assembled, config.fade_out_secs);
        }

        info!(
            duration = assembled.duration_secs(),
            samples = assembled.samples.len(),
            "Podcast generation complete"
        );

        Ok(assembled)
    }

    /// Generates a podcast and writes it to a WAV file.
    #[instrument(skip(self, script, output), fields(output = %output.as_ref().display()))]
    pub async fn generate_to_file(
        &self,
        script: &PodcastScript,
        output: impl AsRef<Path>,
    ) -> Result<(), PodcastError> {
        let buffer = self.generate(script).await?;
        phonon_wav::write_wav(output, &buffer)
            .map_err(|e| PodcastError::Io(e.to_string()))?;
        Ok(())
    }

    /// Generates a single segment preview.
    #[instrument(skip(self, script), fields(segment_index))]
    pub async fn preview_segment(
        &self,
        script: &PodcastScript,
        segment_index: usize,
    ) -> Result<AudioBuffer, PodcastError> {
        let segment = script.segments.get(segment_index).ok_or_else(|| {
            PodcastError::Script(format!(
                "Segment index {} out of range (total: {})",
                segment_index,
                script.segments.len()
            ))
        })?;

        let voice = script.speakers.get(&segment.speaker).ok_or_else(|| {
            PodcastError::Script(format!("Unknown speaker: {}", segment.speaker))
        })?;

        let buffer = self
            .tts
            .synthesize(&segment.text, voice)
            .await
            .map_err(|e| PodcastError::Tts(e.to_string()))?;

        Ok(buffer)
    }

    /// Post-process segments: resample and channel-convert to target format.
    fn post_process_segments(
        &self,
        buffers: Vec<AudioBuffer>,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<Vec<AudioBuffer>, PodcastError> {
        let mut processed = Vec::with_capacity(buffers.len());

        for (i, mut buf) in buffers.into_iter().enumerate() {
            // Resample if needed
            if buf.sample_rate != target_rate {
                buf = phonon_core::resample(&buf, target_rate)
                    .map_err(|e| PodcastError::Processing(format!("Resample segment {}: {}", i, e)))?;
            }

            // Channel conversion if needed
            if buf.channels != target_channels {
                buf = phonon_core::ensure_channels(&buf, target_channels)
                    .map_err(|e| PodcastError::Processing(format!("Channel convert segment {}: {}", i, e)))?;
            }

            // Per-segment normalize to consistent volume
            phonon_core::normalize(&mut buf, 0.9);

            processed.push(buf);
        }

        Ok(processed)
    }

    /// Assemble segments with silence pauses or crossfades between them.
    fn assemble_segments(
        &self,
        segments: Vec<AudioBuffer>,
        config: &PipelineConfig,
    ) -> Result<AudioBuffer, PodcastError> {
        if segments.is_empty() {
            return Err(PodcastError::Processing("No segments to assemble".to_string()));
        }

        let target_rate = config.output_sample_rate;
        let target_channels = config.output_channels;
        let pause_secs = config.pause_between_segments_secs;
        let crossfade_secs = config.crossfade_secs;

        let mut iter = segments.into_iter();
        let mut result = iter.next()
            .ok_or_else(|| PodcastError::Processing("No segments".to_string()))?;

        for next_segment in iter {
            // Insert pause between segments (if configured)
            if pause_secs > 0.0 {
                let pause = phonon_core::silence(pause_secs, target_rate, target_channels)
                    .map_err(|e| PodcastError::Processing(format!("Generate pause: {}", e)))?;
                result = phonon_core::merge(&[result, pause])
                    .map_err(|e| PodcastError::Processing(format!("Merge pause: {}", e)))?;
            }

            // Append the next segment (with optional crossfade)
            if crossfade_secs > 0.0 {
                result = phonon_core::crossfade(&result, &next_segment, crossfade_secs)
                    .map_err(|e| PodcastError::Processing(format!("Crossfade: {}", e)))?;
            } else {
                result = phonon_core::merge(&[result, next_segment])
                    .map_err(|e| PodcastError::Processing(format!("Merge segment: {}", e)))?;
            }
        }

        Ok(result)
    }

    /// Load an audio file, resampling/channel-converting to target format.
    fn load_and_convert(
        &self,
        path: &str,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<AudioBuffer, PodcastError> {
        let mut buf = phonon_io::read_audio(path)
            .map_err(|e| PodcastError::Io(format!("Failed to load '{}': {}", path, e)))?;

        if buf.sample_rate != target_rate {
            buf = phonon_core::resample(&buf, target_rate)
                .map_err(|e| PodcastError::Processing(e.to_string()))?;
        }
        if buf.channels != target_channels {
            buf = phonon_core::ensure_channels(&buf, target_channels)
                .map_err(|e| PodcastError::Processing(e.to_string()))?;
        }

        Ok(buf)
    }

    /// Add intro/outro audio to the assembled track.
    fn add_intro_outro(
        &self,
        mut main: AudioBuffer,
        config: &PipelineConfig,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<AudioBuffer, PodcastError> {
        if let Some(ref intro_path) = config.intro_audio {
            let intro = self.load_and_convert(intro_path, target_rate, target_channels)?;
            main = phonon_core::merge(&[intro, main])
                .map_err(|e| PodcastError::Processing(format!("Merge intro: {}", e)))?;
        }

        if let Some(ref outro_path) = config.outro_audio {
            let outro = self.load_and_convert(outro_path, target_rate, target_channels)?;
            main = phonon_core::merge(&[main, outro])
                .map_err(|e| PodcastError::Processing(format!("Merge outro: {}", e)))?;
        }

        Ok(main)
    }

    /// Mix background music with the main track.
    fn mix_bgm(
        &self,
        main: AudioBuffer,
        config: &PipelineConfig,
        target_rate: u32,
        target_channels: u16,
    ) -> Result<AudioBuffer, PodcastError> {
        let Some(ref bgm_path) = config.bgm_audio else {
            return Ok(main);
        };

        let mut bgm = self.load_and_convert(bgm_path, target_rate, target_channels)?;

        // Lower BGM volume
        phonon_core::adjust_volume(&mut bgm, config.bgm_volume);

        // Loop BGM to match main track length if needed
        if bgm.samples.len() < main.samples.len() {
            let mut looped = Vec::with_capacity(main.samples.len());
            while looped.len() < main.samples.len() {
                let remaining = main.samples.len() - looped.len();
                let take = remaining.min(bgm.samples.len());
                looped.extend_from_slice(&bgm.samples[..take]);
            }
            bgm.samples = looped;
        } else {
            bgm.samples.truncate(main.samples.len());
        }

        phonon_core::mix(&main, &bgm)
            .map_err(|e| PodcastError::Processing(format!("Mix BGM: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonon_ai::{AiError, VoiceConfig};

    /// A mock TTS provider that generates a sine wave for any text.
    /// The duration scales with text length to simulate real TTS.
    struct MockTts;

    impl TtsProvider for MockTts {
        async fn synthesize(
            &self,
            text: &str,
            _voice: &VoiceConfig,
        ) -> Result<AudioBuffer, AiError> {
            // Generate a short sine tone, duration proportional to text length
            let sample_rate = 24000_u32; // Common TTS output rate
            let duration_secs = 0.5 + (text.len() as f64 * 0.02); // ~20ms per char
            let total_samples = (duration_secs * sample_rate as f64) as usize;

            let mut samples = Vec::with_capacity(total_samples);
            for i in 0..total_samples {
                let t = i as f64 / sample_rate as f64;
                let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
                samples.push(sample);
            }

            Ok(AudioBuffer {
                samples,
                sample_rate,
                channels: 1,
            })
        }
    }

    #[tokio::test]
    async fn test_pipeline_basic() {
        let json = r#"{
            "title": "Test Episode",
            "speakers": {
                "host": { "voice_id": "alloy" },
                "guest": { "voice_id": "echo" }
            },
            "segments": [
                { "speaker": "host", "text": "Welcome to the show!" },
                { "speaker": "guest", "text": "Thanks for having me." },
                { "speaker": "host", "text": "Let us dive in." }
            ]
        }"#;

        let script = PodcastScript::from_json(json).unwrap();
        let pipeline = PodcastPipeline::new(MockTts);

        let result = pipeline.generate(&script).await.unwrap();

        // Should have produced non-empty audio
        assert!(!result.samples.is_empty());
        assert_eq!(result.sample_rate, 44100); // Default output rate
        assert_eq!(result.channels, 1);
        // Duration should be > 0
        assert!(result.duration_secs() > 0.0);
    }

    #[tokio::test]
    async fn test_pipeline_with_config() {
        let json = r#"{
            "title": "Configured Episode",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "Hello world" },
                { "speaker": "host", "text": "Goodbye world" }
            ],
            "config": {
                "pause_between_segments_secs": 1.0,
                "normalize_target": 0.8,
                "fade_in_secs": 0.5,
                "fade_out_secs": 0.5
            }
        }"#;

        let script = PodcastScript::from_json(json).unwrap();
        let pipeline = PodcastPipeline::new(MockTts);

        let result = pipeline.generate(&script).await.unwrap();

        assert!(!result.samples.is_empty());
        // With 1s pause between 2 segments, total should be > 1s
        assert!(result.duration_secs() > 1.0);
    }

    #[tokio::test]
    async fn test_pipeline_preview_segment() {
        let json = r#"{
            "title": "Preview Test",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "First segment" },
                { "speaker": "host", "text": "Second segment" }
            ]
        }"#;

        let script = PodcastScript::from_json(json).unwrap();
        let pipeline = PodcastPipeline::new(MockTts);

        let seg0 = pipeline.preview_segment(&script, 0).await.unwrap();
        let seg1 = pipeline.preview_segment(&script, 1).await.unwrap();

        assert!(!seg0.samples.is_empty());
        assert!(!seg1.samples.is_empty());

        // Segment 2 has more chars → should be slightly longer
        // ("Second segment" vs "First segment")
        assert!(seg1.samples.len() >= seg0.samples.len());

        // Out of range should error
        assert!(pipeline.preview_segment(&script, 5).await.is_err());
    }

    #[tokio::test]
    async fn test_pipeline_generate_to_file() {
        let json = r#"{
            "title": "File Test",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "Hello world" }
            ]
        }"#;

        let script = PodcastScript::from_json(json).unwrap();
        let pipeline = PodcastPipeline::new(MockTts);

        let tmp = std::env::temp_dir().join("phonon_test_podcast.wav");
        pipeline.generate_to_file(&script, &tmp).await.unwrap();

        // File should exist and be non-empty
        let metadata = std::fs::metadata(&tmp).unwrap();
        assert!(metadata.len() > 0);

        // Clean up
        let _ = std::fs::remove_file(&tmp);
    }
}
