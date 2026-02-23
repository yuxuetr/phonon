//! Workspace-level integration tests.
//!
//! Verifies cross-crate end-to-end flows: signal generation → processing → I/O roundtrip.

#[cfg(test)]
mod tests {
    use phonon_core::{algorithms, AudioBuffer};
    use phonon_podcast::subtitle::{SubtitleEntry, generate_srt};
    use phonon_podcast::chapters::{chapters_from_silence, SilenceDetectConfig};
    use phonon_ai::TranscriptSegment;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn sine_wave(sample_rate: u32, duration_secs: f64, freq: f64) -> AudioBuffer {
        let total = (sample_rate as f64 * duration_secs) as usize;
        let mut samples = Vec::with_capacity(total);
        for i in 0..total {
            let t = i as f64 / sample_rate as f64;
            samples.push((t * freq * 2.0 * std::f64::consts::PI).sin() as f32 * 0.8);
        }
        AudioBuffer {
            samples,
            sample_rate,
            channels: 1,
        }
    }

    fn silence_buffer(sample_rate: u32, duration_secs: f64) -> AudioBuffer {
        let total = (sample_rate as f64 * duration_secs) as usize;
        AudioBuffer {
            samples: vec![0.0; total],
            sample_rate,
            channels: 1,
        }
    }

    // -----------------------------------------------------------------------
    // WAV roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_wav_roundtrip() {
        let original = sine_wave(44100, 1.0, 440.0);
        let tmp = std::env::temp_dir().join("phonon_integ_wav_rt.wav");

        phonon_wav::write_wav(&tmp, &original).unwrap();
        let loaded = phonon_io::read_audio(&tmp).unwrap();

        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.channels, 1);
        assert!(
            (loaded.samples.len() as i64 - original.samples.len() as i64).unsigned_abs() < 10,
            "sample count mismatch: {} vs {}",
            loaded.samples.len(),
            original.samples.len()
        );

        for (i, (&orig, &loaded_s)) in original.samples.iter().zip(loaded.samples.iter()).enumerate() {
            let diff = (orig - loaded_s).abs();
            assert!(diff < 0.001, "sample {} diverged: orig={}, loaded={}", i, orig, loaded_s);
        }

        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------------
    // MP3 roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_mp3_roundtrip() {
        let original = sine_wave(44100, 1.0, 440.0);
        let tmp = std::env::temp_dir().join("phonon_integ_mp3_rt.mp3");

        let config = phonon_io::Mp3Config { bitrate_kbps: 192 };
        phonon_io::write_mp3(&tmp, &original, &config).unwrap();

        let loaded = phonon_io::read_audio(&tmp).unwrap();
        assert_eq!(loaded.sample_rate, 44100);
        assert!(loaded.samples.len() > 40000, "too few samples: {}", loaded.samples.len());
        assert!(loaded.duration_secs() > 0.9, "duration too short: {}", loaded.duration_secs());

        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------------
    // DSP pipeline: generate → normalize → compress → fade → write
    // -----------------------------------------------------------------------

    #[test]
    fn test_dsp_pipeline() {
        let mut buffer = sine_wave(44100, 2.0, 440.0);
        assert!((buffer.duration_secs() - 2.0).abs() < 0.01);

        algorithms::normalize(&mut buffer, 0.9);
        let peak = algorithms::peak(&buffer);
        assert!((peak - 0.9).abs() < 0.01, "peak after normalize: {}", peak);

        let config = algorithms::CompressorConfig::voice();
        let compressed = algorithms::with_compression(&buffer, &config);
        assert_eq!(compressed.samples.len(), buffer.samples.len());

        let mut result = compressed;
        algorithms::fade_in(&mut result, 0.2);
        algorithms::fade_out(&mut result, 0.5);

        assert!(result.samples[0].abs() < 0.01, "first sample after fade_in: {}", result.samples[0]);
        let last = result.samples.last().copied().unwrap_or(1.0);
        assert!(last.abs() < 0.01, "last sample after fade_out: {}", last);

        let tmp = std::env::temp_dir().join("phonon_integ_dsp_pipeline.wav");
        phonon_wav::write_wav(&tmp, &result).unwrap();
        let reloaded = phonon_io::read_audio(&tmp).unwrap();
        assert_eq!(reloaded.sample_rate, result.sample_rate);
        assert!(!reloaded.samples.is_empty());

        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------------
    // Podcast script parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_podcast_script_parse() {
        let json = r#"{
            "title": "Test Episode",
            "speakers": {
                "Alice": { "voice_id": "alloy" },
                "Bob": { "voice_id": "echo" }
            },
            "segments": [
                { "speaker": "Alice", "text": "Hello everyone!" },
                { "speaker": "Bob", "text": "Thanks for having me." },
                { "speaker": "Alice", "text": "Let's get started." }
            ]
        }"#;

        let script: phonon_podcast::PodcastScript = serde_json::from_str(json).unwrap();
        assert_eq!(script.title, "Test Episode");
        assert_eq!(script.segment_count(), 3);
        assert_eq!(script.speaker_names().len(), 2);
        assert_eq!(script.segments[0].speaker, "Alice");
        assert_eq!(script.segments[1].speaker, "Bob");
    }

    // -----------------------------------------------------------------------
    // SRT generation
    // -----------------------------------------------------------------------

    #[test]
    fn test_srt_generation_from_entries() {
        let entries = vec![
            SubtitleEntry::new(0.0, 2.5, "Hello and welcome."),
            SubtitleEntry::new(2.5, 5.0, "Today we discuss audio."),
            SubtitleEntry::new(5.0, 8.0, "Let's begin!"),
        ];

        let srt = generate_srt(&entries);
        let lines: Vec<&str> = srt.lines().collect();

        assert_eq!(lines[0], "1");
        assert_eq!(lines[1], "00:00:00,000 --> 00:00:02,500");
        assert_eq!(lines[2], "Hello and welcome.");
        assert_eq!(lines[4], "2");
        assert_eq!(lines[5], "00:00:02,500 --> 00:00:05,000");
        assert_eq!(lines[8], "3");
    }

    #[test]
    fn test_srt_from_transcript_segments() {
        let segments = vec![
            TranscriptSegment { id: 0, start: 0.0, end: 1.5, text: " Hello ".to_string() },
            TranscriptSegment { id: 1, start: 1.5, end: 3.0, text: "World".to_string() },
        ];

        let entries = phonon_podcast::subtitle::from_segments(&segments);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Hello");
        assert_eq!(entries[1].start, 1.5);
    }

    // -----------------------------------------------------------------------
    // Chapter detection (silence-based)
    // -----------------------------------------------------------------------

    #[test]
    fn test_chapter_detection_with_silence() {
        let sr = 44100;
        let tone = sine_wave(sr, 1.0, 440.0);
        let sil = silence_buffer(sr, 2.0);

        let mut samples = Vec::new();
        samples.extend_from_slice(&tone.samples);
        samples.extend_from_slice(&sil.samples);
        samples.extend_from_slice(&tone.samples);
        samples.extend_from_slice(&sil.samples);
        samples.extend_from_slice(&tone.samples);

        let buffer = AudioBuffer { samples, sample_rate: sr, channels: 1 };
        assert!((buffer.duration_secs() - 7.0).abs() < 0.01);

        let config = SilenceDetectConfig { threshold: 0.01, min_silence_secs: 1.0 };
        let chapters = chapters_from_silence(&buffer, &config);

        assert_eq!(chapters.len(), 3, "expected 3 chapters, got {}: {:?}", chapters.len(), chapters);
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[1].title, "Chapter 2");
        assert_eq!(chapters[2].title, "Chapter 3");
    }

    #[test]
    fn test_chapter_detection_no_silence() {
        let buffer = sine_wave(44100, 3.0, 440.0);
        let config = SilenceDetectConfig { threshold: 0.01, min_silence_secs: 1.0 };
        let chapters = chapters_from_silence(&buffer, &config);

        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "Chapter 1");
        assert!(chapters[0].end.is_some());
    }

    // -----------------------------------------------------------------------
    // Cross-crate: generate → resample → channel convert → write → read
    // -----------------------------------------------------------------------

    #[test]
    fn test_resample_and_channel_convert_pipeline() {
        let mono = sine_wave(22050, 1.0, 440.0);
        assert_eq!(mono.sample_rate, 22050);
        assert_eq!(mono.channels, 1);

        let resampled = algorithms::resample(&mono, 44100).unwrap();
        assert_eq!(resampled.sample_rate, 44100);
        assert!((resampled.duration_secs() - 1.0).abs() < 0.05);

        let stereo = algorithms::ensure_channels(&resampled, 2).unwrap();
        assert_eq!(stereo.channels, 2);
        assert!((stereo.duration_secs() - 1.0).abs() < 0.05);

        let tmp = std::env::temp_dir().join("phonon_integ_resample_ch.wav");
        phonon_wav::write_wav(&tmp, &stereo).unwrap();
        let loaded = phonon_io::read_audio(&tmp).unwrap();
        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.channels, 2);

        let _ = std::fs::remove_file(&tmp);
    }

    // -----------------------------------------------------------------------
    // Signal generators end-to-end
    // -----------------------------------------------------------------------

    #[test]
    fn test_generator_to_wav_roundtrip() {
        let tone = algorithms::generate_tone(880.0, 0.5, 44100).unwrap();
        assert_eq!(tone.sample_rate, 44100);
        assert!((tone.duration_secs() - 0.5).abs() < 0.01);

        let tmp = std::env::temp_dir().join("phonon_integ_gen_tone.wav");
        phonon_io::write_audio(&tmp, &tone).unwrap();
        let loaded = phonon_io::read_audio(&tmp).unwrap();
        assert!(loaded.samples.len() > 20000);

        let _ = std::fs::remove_file(&tmp);
    }
}
