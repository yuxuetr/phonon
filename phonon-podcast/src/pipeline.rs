use crate::PodcastError;
use crate::config::{AssetSource, PipelineConfig};
use crate::export::{Episode, build_episode_metadata};
use crate::script::PodcastScript;
use phonon_ai::{AiError, SoundGenProvider, SoundPrompt, TtsProvider};
use phonon_core::{self, AudioBuffer};
use std::path::Path;
use tokio::sync::Semaphore;
use tracing::{info, instrument};

/// Sentinel "no sound generator" type — the default `S` parameter for
/// [`PodcastPipeline`]. Calling [`SoundGenProvider::generate`] on this
/// returns an error so configs that ask for `AssetSource::Generated`
/// fail loudly rather than silently dropping the asset.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSoundGen;

impl SoundGenProvider for NoSoundGen {
  async fn generate(&self, _prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
    Err(AiError::Config(
      "AssetSource::Generated requires a SoundGenProvider — call \
       PodcastPipeline::with_sound_gen() first"
        .to_string(),
    ))
  }
}

/// The podcast generation pipeline.
///
/// Orchestrates TTS synthesis, audio processing, and assembly
/// to produce a complete podcast episode from a script.
///
/// The optional second type parameter `S` is a [`SoundGenProvider`].
/// By default it's [`NoSoundGen`] — set it via
/// [`PodcastPipeline::with_sound_gen`] when the script uses
/// `AssetSource::Generated` for BGM, intro, or outro.
pub struct PodcastPipeline<T: TtsProvider, S: SoundGenProvider = NoSoundGen> {
  tts: T,
  sound_gen: S,
}

impl<T: TtsProvider> PodcastPipeline<T, NoSoundGen> {
  /// Creates a new pipeline with the given TTS provider and no
  /// sound generator. Use [`Self::with_sound_gen`] to upgrade.
  pub fn new(tts: T) -> Self {
    Self {
      tts,
      sound_gen: NoSoundGen,
    }
  }
}

impl<T: TtsProvider, S: SoundGenProvider> PodcastPipeline<T, S> {
  /// Returns a new pipeline with the supplied sound generator,
  /// preserving the existing TTS provider.
  pub fn with_sound_gen<S2: SoundGenProvider>(self, sound_gen: S2) -> PodcastPipeline<T, S2> {
    PodcastPipeline {
      tts: self.tts,
      sound_gen,
    }
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
  ///
  /// Steps 2–6 are CPU-bound and run on tokio's blocking pool.
  #[instrument(skip(self, script), fields(title = %script.title, segments = script.segment_count()))]
  pub async fn generate(&self, script: &PodcastScript) -> Result<AudioBuffer, PodcastError> {
    let config = &script.config;

    info!("Starting podcast generation");

    let segment_buffers = self.synthesize_segments(script).await?;
    let assets = self.resolve_assets(config).await?;
    let (assembled, _spans) = render_off_runtime(segment_buffers, assets, config.clone()).await?;

    info!(
      duration = assembled.duration_secs(),
      samples = assembled.samples.len(),
      "Podcast generation complete"
    );

    Ok(assembled)
  }

  /// Generate a complete [`Episode`] — audio plus chapters and SRT
  /// reconstructed from per-segment durations. The returned episode has
  /// empty `tags`; populate them via [`Episode::with_tags`] before export.
  #[instrument(skip(self, script), fields(title = %script.title))]
  pub async fn generate_episode(&self, script: &PodcastScript) -> Result<Episode, PodcastError> {
    // Segment order is kept, so the spans measured during assembly line
    // up with the script even though synthesis runs concurrently.
    let raw_buffers = self.synthesize_segments(script).await?;

    let assets = self.resolve_assets(&script.config).await?;
    let (assembled, spans) = render_off_runtime(raw_buffers, assets, script.config.clone()).await?;

    let (chapters, srt) = build_episode_metadata(script, &spans);
    let episode = Episode::new(assembled)
      .with_chapters(chapters)
      .with_srt(srt);

    info!("Episode generation complete");
    Ok(episode)
  }

  /// Generates a podcast and writes it to a WAV file.
  #[instrument(skip(self, script, output), fields(output = %output.as_ref().display()))]
  pub async fn generate_to_file(
    &self,
    script: &PodcastScript,
    output: impl AsRef<Path>,
  ) -> Result<(), PodcastError> {
    let buffer = self.generate(script).await?;
    let output = output.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || phonon_wav::write_wav(output, &buffer))
      .await
      .map_err(|e| PodcastError::Processing(format!("WAV writer task failed: {e}")))?
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

    let voice = script
      .speakers
      .get(&segment.speaker)
      .ok_or_else(|| PodcastError::Script(format!("Unknown speaker: {}", segment.speaker)))?;

    let buffer = self
      .tts
      .synthesize(&segment.text, voice)
      .await
      .map_err(|e| PodcastError::Tts(e.to_string()))?;

    Ok(buffer)
  }

  /// Synthesize every segment, at most `config.tts_concurrency` at a
  /// time, returning the buffers in script order.
  async fn synthesize_segments(
    &self,
    script: &PodcastScript,
  ) -> Result<Vec<AudioBuffer>, PodcastError> {
    // Resolve every voice (with segment-level language overrides) before
    // starting any synthesis, so an unknown speaker fails fast.
    let mut items = Vec::with_capacity(script.segments.len());
    for (i, segment) in script.segments.iter().enumerate() {
      let mut voice = script
        .speakers
        .get(&segment.speaker)
        .ok_or_else(|| PodcastError::Script(format!("Unknown speaker: {}", segment.speaker)))?
        .clone();
      if let Some(ref lang) = segment.language {
        voice.language = Some(lang.clone());
      }
      items.push((i, segment, voice));
    }

    let concurrency = script.config.tts_concurrency.max(1);
    info!(
      segments = items.len(),
      concurrency, "Starting concurrent TTS synthesis"
    );

    let semaphore = Semaphore::new(concurrency);
    let semaphore = &semaphore;
    let tts = &self.tts;
    let futures = items.into_iter().map(|(i, segment, voice)| async move {
      let _permit = semaphore
        .acquire()
        .await
        .map_err(|e| PodcastError::Processing(format!("Semaphore error: {e}")))?;
      info!(segment = i, speaker = %segment.speaker, "Synthesizing segment");
      tts
        .synthesize(&segment.text, &voice)
        .await
        .map_err(|e| PodcastError::Tts(e.to_string()))
    });

    let buffers = futures::future::try_join_all(futures).await?;
    info!(count = buffers.len(), "All segments synthesized");
    Ok(buffers)
  }

  /// Load or generate the configured intro / outro / BGM, still in their
  /// source format (conversion happens in [`render`]).
  async fn resolve_assets(&self, config: &PipelineConfig) -> Result<Assets, PodcastError> {
    let mut assets = Assets::default();
    if let Some(source) = config.effective_intro() {
      assets.intro = Some(self.resolve_asset(&source).await?);
    }
    if let Some(source) = config.effective_outro() {
      assets.outro = Some(self.resolve_asset(&source).await?);
    }
    if let Some(source) = config.effective_bgm() {
      assets.bgm = Some(self.resolve_asset(&source).await?);
    }
    Ok(assets)
  }

  async fn resolve_asset(&self, source: &AssetSource) -> Result<AudioBuffer, PodcastError> {
    match source {
      AssetSource::File { path } => {
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
          phonon_io::read_audio(&path)
            .map_err(|e| PodcastError::Io(format!("Failed to load '{}': {}", path, e)))
        })
        .await
        .map_err(|e| PodcastError::Processing(format!("Asset loader task failed: {e}")))?
      }
      AssetSource::Generated { prompt } => self
        .sound_gen
        .generate(prompt)
        .await
        .map_err(|e| PodcastError::Processing(format!("Sound generation failed: {}", e))),
    }
  }
}

/// Intro / outro / BGM buffers resolved before rendering.
#[derive(Default)]
struct Assets {
  intro: Option<AudioBuffer>,
  outro: Option<AudioBuffer>,
  bgm: Option<AudioBuffer>,
}

/// Run [`render`] on tokio's blocking pool so resampling, assembly and
/// mixing don't stall the async worker that awaits the pipeline.
async fn render_off_runtime(
  segments: Vec<AudioBuffer>,
  assets: Assets,
  config: PipelineConfig,
) -> Result<(AudioBuffer, Vec<(f64, f64)>), PodcastError> {
  tokio::task::spawn_blocking(move || render(segments, assets, &config))
    .await
    .map_err(|e| PodcastError::Processing(format!("Render task failed: {e}")))?
}

/// Everything after TTS: convert, assemble, add intro/outro, mix BGM,
/// normalize and fade. Returns the finished track and each segment's
/// `(start, end)` in seconds within it.
fn render(
  segments: Vec<AudioBuffer>,
  assets: Assets,
  config: &PipelineConfig,
) -> Result<(AudioBuffer, Vec<(f64, f64)>), PodcastError> {
  let target_rate = config.output_sample_rate;
  let target_channels = config.output_channels;

  let segments = post_process_segments(segments, target_rate, target_channels)?;

  // Segment spans are measured during assembly (so pauses and crossfade
  // overlaps are exact), then shifted by the intro that precedes them.
  let (mut assembled, spans) = assemble_segments(segments, config)?;

  let mut intro_secs = 0.0;
  if let Some(intro) = assets.intro {
    let intro = convert_to_target(intro, target_rate, target_channels)?;
    intro_secs = intro.duration_secs();
    assembled = phonon_core::merge(&[intro, assembled])
      .map_err(|e| PodcastError::Processing(format!("Merge intro: {}", e)))?;
  }
  if let Some(outro) = assets.outro {
    let outro = convert_to_target(outro, target_rate, target_channels)?;
    assembled = phonon_core::merge(&[assembled, outro])
      .map_err(|e| PodcastError::Processing(format!("Merge outro: {}", e)))?;
  }
  let spans = spans
    .into_iter()
    .map(|(start, end)| (start + intro_secs, end + intro_secs))
    .collect();

  if let Some(bgm) = assets.bgm {
    let bgm = convert_to_target(bgm, target_rate, target_channels)?;
    assembled = mix_bgm(assembled, bgm, config.bgm_volume)?;
  }

  phonon_core::normalize(&mut assembled, config.normalize_target);
  if config.fade_in_secs > 0.0 {
    phonon_core::fade_in(&mut assembled, config.fade_in_secs);
  }
  if config.fade_out_secs > 0.0 {
    phonon_core::fade_out(&mut assembled, config.fade_out_secs);
  }

  Ok((assembled, spans))
}

/// True-peak ceiling for the loudest-peaking segment after leveling.
const SEGMENT_TRUE_PEAK_DBTP: f32 = -1.0;

/// Post-process segments: resample, channel-convert to target format and
/// level them to one loudness (see [`level_segments`]).
fn post_process_segments(
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

    processed.push(buf);
  }

  level_segments(&mut processed);

  Ok(processed)
}

/// Give every segment the same integrated loudness: the highest at which
/// no segment's true peak exceeds [`SEGMENT_TRUE_PEAK_DBTP`]. Matching
/// peaks instead leaves TTS voices up to 6 dB apart; a fixed loudness
/// target would either clip peaky voices or move speech against BGM, which
/// is scaled independently. Segments too short to measure (< 400 ms) are
/// peak-normalized as before.
fn level_segments(segments: &mut [AudioBuffer]) {
  let measured: Vec<Option<(f32, f32)>> = segments
    .iter()
    .map(|buf| {
      let lufs = phonon_core::loudness_integrated(buf);
      lufs
        .is_finite()
        .then(|| (lufs, phonon_core::true_peak_dbtp(buf)))
    })
    .collect();
  let target = measured
    .iter()
    .flatten()
    .map(|(lufs, peak)| lufs + SEGMENT_TRUE_PEAK_DBTP - peak)
    .fold(f32::INFINITY, f32::min);
  for (buf, level) in segments.iter_mut().zip(measured) {
    match level {
      Some((lufs, _)) => phonon_core::adjust_volume_db(buf, target - lufs),
      None => phonon_core::normalize(buf, 0.9),
    }
  }
}

/// Assemble segments with silence pauses or crossfades between them.
///
/// Also returns each segment's `(start, end)` in seconds within the
/// assembled track. Everything is appended into one pre-sized buffer, so
/// assembly is linear in the output length.
fn assemble_segments(
  segments: Vec<AudioBuffer>,
  config: &PipelineConfig,
) -> Result<(AudioBuffer, Vec<(f64, f64)>), PodcastError> {
  if segments.is_empty() {
    return Err(PodcastError::Processing(
      "No segments to assemble".to_string(),
    ));
  }

  let target_rate = config.output_sample_rate;
  let target_channels = config.output_channels;
  let samples_per_sec = target_rate as f64 * target_channels as f64;

  let pause = if config.pause_between_segments_secs > 0.0 {
    phonon_core::silence(
      config.pause_between_segments_secs,
      target_rate,
      target_channels,
    )
    .map_err(|e| PodcastError::Processing(format!("Generate pause: {}", e)))?
    .samples
  } else {
    Vec::new()
  };
  // Same length rule as `phonon_core::crossfade`: whole frames only.
  let ch = target_channels.max(1) as usize;
  let fade_len = if config.crossfade_secs > 0.0 {
    let len = (config.crossfade_secs * samples_per_sec) as usize;
    len - len % ch
  } else {
    0
  };

  let capacity =
    segments.iter().map(|s| s.samples.len()).sum::<usize>() + pause.len() * (segments.len() - 1);
  let mut out: Vec<f32> = Vec::with_capacity(capacity);
  let mut spans = Vec::with_capacity(segments.len());

  for (i, segment) in segments.iter().enumerate() {
    let next = &segment.samples;
    let mut overlap = 0;
    if i > 0 {
      out.extend_from_slice(&pause);
      // Linear crossfade of the track's tail (the pause, if any) into the
      // head of the next segment, as in `phonon_core::crossfade`.
      overlap = fade_len.min(out.len()).min(next.len());
      let start = out.len() - overlap;
      for (j, (o, &n)) in out[start..].iter_mut().zip(next.iter()).enumerate() {
        let t = j as f32 / overlap as f32;
        *o = (*o * (1.0 - t) + n * t).clamp(-1.0, 1.0);
      }
    }
    let start = out.len() - overlap;
    out.extend_from_slice(&next[overlap..]);
    spans.push((
      start as f64 / samples_per_sec,
      out.len() as f64 / samples_per_sec,
    ));
  }

  let assembled = AudioBuffer {
    samples: out,
    sample_rate: target_rate,
    channels: target_channels,
  };
  Ok((assembled, spans))
}

/// Resample + channel-convert an already-loaded buffer to the target.
fn convert_to_target(
  mut buf: AudioBuffer,
  target_rate: u32,
  target_channels: u16,
) -> Result<AudioBuffer, PodcastError> {
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

/// Mix background music under the main track, looping or trimming it to
/// the main track's length.
fn mix_bgm(
  main: AudioBuffer,
  mut bgm: AudioBuffer,
  volume: f32,
) -> Result<AudioBuffer, PodcastError> {
  if bgm.samples.is_empty() {
    return Err(PodcastError::Processing(
      "BGM asset is empty — nothing to loop under the main track".to_string(),
    ));
  }

  phonon_core::adjust_volume(&mut bgm, volume);

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

  phonon_core::mix(&main, &bgm).map_err(|e| PodcastError::Processing(format!("Mix BGM: {}", e)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_ai::{AiError, VoiceConfig};
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  fn sine(secs: f32, amp: f32) -> Vec<f32> {
    (0..(secs * 16_000.0) as usize)
      .map(|i| amp * (std::f32::consts::TAU * 440.0 * i as f32 / 16_000.0).sin())
      .collect()
  }

  #[test]
  fn test_segments_match_loudness_not_peak() {
    // Same peak, very different loudness: a steady tone vs a quiet tone
    // with one loud burst. Peak normalization leaves them ~20 dB apart.
    let steady = AudioBuffer {
      samples: sine(3.0, 0.5),
      sample_rate: 16_000,
      channels: 1,
    };
    let mut bursty = sine(3.0, 0.05);
    bursty[..800].copy_from_slice(&sine(0.05, 0.5));
    let bursty = AudioBuffer {
      samples: bursty,
      sample_rate: 16_000,
      channels: 1,
    };
    let short = AudioBuffer {
      samples: sine(0.2, 0.1),
      sample_rate: 16_000,
      channels: 1,
    };
    let out = post_process_segments(vec![steady, bursty, short], 16_000, 1).expect("process");
    let a = phonon_core::loudness_integrated(&out[0]);
    let b = phonon_core::loudness_integrated(&out[1]);
    assert!((a - b).abs() <= 0.1, "segments {a:.2} vs {b:.2} LUFS");
    // The peakier segment sets the level: its true peak sits at the ceiling.
    let peaks = [&out[0], &out[1]].map(phonon_core::true_peak_dbtp);
    assert!(
      (peaks[1] - SEGMENT_TRUE_PEAK_DBTP).abs() < 0.05,
      "{peaks:?}"
    );
    assert!(peaks[0] < peaks[1], "{peaks:?}");
    // Too short to measure: falls back to peak normalization.
    let peak = out[2].samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!((peak - 0.9).abs() < 1e-3, "short segment peak {peak}");
  }

  /// Mock SoundGenProvider that records prompt count and returns a quiet
  /// 1 s sine at 44.1 kHz mono — long enough to be looped/trimmed by the
  /// pipeline against a typical few-segment script output.
  struct MockSoundGen {
    calls: Arc<AtomicUsize>,
  }

  impl SoundGenProvider for MockSoundGen {
    fn generate(
      &self,
      _prompt: &SoundPrompt,
    ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send {
      let calls = self.calls.clone();
      async move {
        calls.fetch_add(1, Ordering::SeqCst);
        let sr = 44100_u32;
        let samples: Vec<f32> = (0..sr as usize)
          .map(|i| {
            let t = i as f64 / sr as f64;
            (t * 220.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.3
          })
          .collect();
        Ok(AudioBuffer {
          samples,
          sample_rate: sr,
          channels: 1,
        })
      }
    }
  }

  /// A mock TTS provider that generates a sine wave for any text.
  /// The duration scales with text length to simulate real TTS.
  struct MockTts;

  impl TtsProvider for MockTts {
    async fn synthesize(&self, text: &str, _voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
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

  /// The quadratic merge/crossfade assembly this module used to do;
  /// `assemble_segments` must produce the same samples and spans.
  fn reference_assembly(
    segments: &[AudioBuffer],
    config: &PipelineConfig,
  ) -> (Vec<f32>, Vec<(f64, f64)>) {
    let sps = config.output_sample_rate as f64 * config.output_channels as f64;
    let mut result = segments[0].clone();
    let mut spans = vec![(0.0, result.samples.len() as f64 / sps)];
    for next in &segments[1..] {
      if config.pause_between_segments_secs > 0.0 {
        let pause = phonon_core::silence(
          config.pause_between_segments_secs,
          config.output_sample_rate,
          config.output_channels,
        )
        .unwrap();
        result = phonon_core::merge(&[result, pause]).unwrap();
      }
      result = if config.crossfade_secs > 0.0 {
        phonon_core::crossfade(&result, next, config.crossfade_secs).unwrap()
      } else {
        phonon_core::merge(&[result, next.clone()]).unwrap()
      };
      let end = result.samples.len();
      spans.push(((end - next.samples.len()) as f64 / sps, end as f64 / sps));
    }
    (result.samples, spans)
  }

  #[test]
  fn test_assembly_matches_merge_and_crossfade_reference() {
    let segments: Vec<AudioBuffer> = [900usize, 40, 1_500, 700]
      .iter()
      .enumerate()
      .map(|(k, &frames)| AudioBuffer {
        samples: (0..frames * 2)
          .map(|i| ((i + k * 7) as f32 * 0.013).sin() * 0.8)
          .collect(),
        sample_rate: 8_000,
        channels: 2,
      })
      .collect();

    for (pause, fade) in [(0.0, 0.0), (0.05, 0.0), (0.0, 0.03), (0.02, 0.03)] {
      let config = PipelineConfig {
        output_sample_rate: 8_000,
        output_channels: 2,
        pause_between_segments_secs: pause,
        crossfade_secs: fade,
        ..Default::default()
      };

      let (expected, expected_spans) = reference_assembly(&segments, &config);
      let (got, spans) = assemble_segments(segments.clone(), &config).unwrap();
      assert_eq!(
        got.samples.len(),
        expected.len(),
        "pause={pause} fade={fade}"
      );
      for (a, b) in got.samples.iter().zip(&expected) {
        assert!(
          (a - b).abs() < 1e-6,
          "pause={pause} fade={fade}: {a} vs {b}"
        );
      }
      assert_eq!(spans, expected_spans, "pause={pause} fade={fade}");
    }
  }

  /// Records the peak number of overlapping `synthesize` calls.
  struct ConcurrencyProbeTts {
    in_flight: AtomicUsize,
    peak: AtomicUsize,
  }

  impl TtsProvider for ConcurrencyProbeTts {
    async fn synthesize(&self, _text: &str, _voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
      let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
      self.peak.fetch_max(now, Ordering::SeqCst);
      tokio::time::sleep(std::time::Duration::from_millis(20)).await;
      self.in_flight.fetch_sub(1, Ordering::SeqCst);
      Ok(AudioBuffer {
        samples: vec![0.1; 4_410],
        sample_rate: 44_100,
        channels: 1,
      })
    }
  }

  #[tokio::test]
  async fn test_generate_episode_synthesizes_concurrently() {
    let json = r#"{
      "title": "Concurrent",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "one" },
        { "speaker": "host", "text": "two" },
        { "speaker": "host", "text": "three" },
        { "speaker": "host", "text": "four" }
      ],
      "config": { "tts_concurrency": 3 }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(ConcurrencyProbeTts {
      in_flight: AtomicUsize::new(0),
      peak: AtomicUsize::new(0),
    });
    let episode = pipeline.generate_episode(&script).await.unwrap();
    assert!(!episode.audio.samples.is_empty());
    let peak = pipeline.tts.peak.load(Ordering::SeqCst);
    assert_eq!(
      peak, 3,
      "expected tts_concurrency-bounded overlap, got {peak}"
    );
  }

  /// Sound generator that returns a zero-length buffer.
  struct EmptySoundGen;

  impl SoundGenProvider for EmptySoundGen {
    async fn generate(&self, _prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
      Ok(AudioBuffer {
        samples: vec![],
        sample_rate: 44100,
        channels: 1,
      })
    }
  }

  #[test]
  fn test_pipeline_empty_bgm_errors_instead_of_hanging() {
    let json = r#"{
      "title": "Empty BGM",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [{ "speaker": "host", "text": "Hello there" }],
      "config": { "bgm": { "kind": "generated", "prompt": { "prompt": "silence" } } }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();

    // Run on a separate thread so a regression (infinite loop) fails the
    // test via the timeout instead of hanging the whole test binary.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
      let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
      let pipeline = PodcastPipeline::new(MockTts).with_sound_gen(EmptySoundGen);
      let _ = tx.send(rt.block_on(pipeline.generate(&script)).is_err());
    });
    let errored = rx
      .recv_timeout(std::time::Duration::from_secs(10))
      .expect("pipeline hung on an empty BGM buffer");
    assert!(errored, "empty BGM should be reported as an error");
  }

  #[tokio::test]
  async fn test_episode_timings_account_for_intro_and_crossfade() {
    let json = r#"{
      "title": "Timing",
      "speakers": { "host": { "voice_id": "alloy" }, "guest": { "voice_id": "echo" } },
      "segments": [
        { "speaker": "host", "text": "Welcome to the show" },
        { "speaker": "guest", "text": "Glad to be here" }
      ],
      "config": {
        "pause_between_segments_secs": 0.5,
        "crossfade_secs": 0.2,
        "intro": { "kind": "generated", "prompt": { "prompt": "jingle" } }
      }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts).with_sound_gen(MockSoundGen {
      calls: Arc::new(AtomicUsize::new(0)),
    });
    let episode = pipeline.generate_episode(&script).await.unwrap();

    // MockSoundGen's intro is exactly 1 s, so speech starts at 1 s.
    let ch = &episode.chapters;
    assert_eq!(ch.len(), 2);
    assert!(
      (ch[0].start - 1.0).abs() < 1e-3,
      "first start {}",
      ch[0].start
    );

    // MockTts: 0.5 s + 20 ms per char. Crossfade overlaps 0.2 s of the pause.
    let seg0 = 0.5 + "Welcome to the show".len() as f64 * 0.02;
    let expected_second = 1.0 + seg0 + 0.5 - 0.2;
    assert!(
      (ch[1].start - expected_second).abs() < 5e-3,
      "second start {} expected {}",
      ch[1].start,
      expected_second
    );

    // The last chapter ends where the audio ends (no outro configured).
    let total = episode.audio.duration_secs();
    let last_end = ch[1].end.unwrap_or(f64::NAN);
    assert!((last_end - total).abs() < 5e-3, "end {last_end} vs {total}");

    let srt = episode.srt.as_deref().unwrap_or("");
    assert!(srt.contains("00:00:01,000 -->"), "srt:\n{srt}");
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
  async fn test_pipeline_generated_bgm_invokes_sound_gen() {
    let json = r#"{
      "title": "Generated BGM",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Welcome to the show" },
        { "speaker": "host", "text": "And here is more content" }
      ],
      "config": {
        "bgm_volume": 0.1,
        "bgm": {
          "kind": "generated",
          "prompt": { "prompt": "ambient pad" }
        }
      }
    }"#;

    let script = PodcastScript::from_json(json).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = PodcastPipeline::new(MockTts).with_sound_gen(MockSoundGen {
      calls: calls.clone(),
    });

    let result = pipeline.generate(&script).await.unwrap();
    assert_eq!(
      calls.load(Ordering::SeqCst),
      1,
      "BGM gen should be invoked once"
    );
    assert!(!result.samples.is_empty());
  }

  #[tokio::test]
  async fn test_pipeline_generated_intro_without_sound_gen_errors() {
    let json = r#"{
      "title": "No Generator",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Hello" }
      ],
      "config": {
        "intro": {
          "kind": "generated",
          "prompt": { "prompt": "jingle" }
        }
      }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let err = pipeline.generate(&script).await.expect_err("should error");
    let s = err.to_string();
    assert!(
      s.contains("SoundGenProvider") || s.contains("sound generator"),
      "unexpected error: {}",
      s
    );
  }

  #[tokio::test]
  async fn test_pipeline_legacy_bgm_audio_still_works() {
    // bgm_audio (legacy path) still loads without a sound generator.
    // Use a non-existent path to verify we *try* to load the file
    // (and thus emit an Io error, not a "no sound gen" error).
    let json = r#"{
      "title": "Legacy",
      "speakers": { "host": { "voice_id": "alloy" } },
      "segments": [
        { "speaker": "host", "text": "Hello" }
      ],
      "config": {
        "bgm_audio": "/nonexistent/path/bgm.wav"
      }
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let err = pipeline.generate(&script).await.expect_err("should error");
    let s = err.to_string();
    assert!(s.contains("Failed to load"), "unexpected error: {}", s);
  }

  #[tokio::test]
  async fn test_generate_episode_produces_chapters_and_srt() {
    let json = r#"{
      "title": "Episode 1",
      "speakers": {
        "alice": { "voice_id": "alloy" },
        "bob":   { "voice_id": "echo" }
      },
      "segments": [
        { "speaker": "alice", "text": "First half" },
        { "speaker": "bob",   "text": "Second half" }
      ]
    }"#;
    let script = PodcastScript::from_json(json).unwrap();
    let pipeline = PodcastPipeline::new(MockTts);
    let episode = pipeline.generate_episode(&script).await.unwrap();

    assert!(!episode.audio.samples.is_empty());
    assert_eq!(
      episode.chapters.len(),
      2,
      "two distinct speakers → two chapters"
    );
    assert_eq!(episode.chapters[0].title, "alice");
    assert_eq!(episode.chapters[1].title, "bob");
    let srt = episode.srt.as_deref().unwrap_or("");
    assert!(srt.contains("First half"));
    assert!(srt.contains("Second half"));
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
