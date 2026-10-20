use clap::{Parser, Subcommand, ValueEnum};
use phonon_ai::{
  EdgeTts, OpenAiTts, SttProvider, TranscribeOptions, TtsProvider, VoiceConfig, WhisperStt,
};
use phonon_core::{FilterType, algorithms};
use phonon_engine::AudioPlayer;
use phonon_io::{AudioChapter, CoverArt, CoverMime, Mp3Config, PodcastTags, attach_tag};
use phonon_podcast::chapters::{SilenceDetectConfig, chapters_from_silence, format_chapters_text};
use phonon_podcast::subtitle::segments_to_srt_file;
use phonon_podcast::{PodcastPipeline, PodcastScript};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy, ValueEnum, Debug)]
enum CliFilterType {
  LowPass,
  HighPass,
  BandPass,
  Notch,
  Peaking,
  LowShelf,
  HighShelf,
}

impl From<CliFilterType> for FilterType {
  fn from(val: CliFilterType) -> Self {
    match val {
      CliFilterType::LowPass => FilterType::LowPass,
      CliFilterType::HighPass => FilterType::HighPass,
      CliFilterType::BandPass => FilterType::BandPass,
      CliFilterType::Notch => FilterType::Notch,
      CliFilterType::Peaking => FilterType::Peaking,
      CliFilterType::LowShelf => FilterType::LowShelf,
      CliFilterType::HighShelf => FilterType::HighShelf,
    }
  }
}

#[derive(Parser)]
#[command(name = "phonon")]
#[command(about = "Phonon Audio Engine CLI — audio processing, TTS, podcast generation, and more")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Inspect an audio file
  Info {
    #[arg(short, long)]
    file: PathBuf,
  },
  /// Play an audio file
  Play {
    #[arg(short, long)]
    file: PathBuf,
  },
  /// Trim audio and save to new file
  Trim {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long)]
    start: f64,
    #[arg(short, long)]
    end: f64,
  },
  /// Mix two audio files together
  Mix {
    #[arg(long)]
    input1: PathBuf,
    #[arg(long)]
    input2: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
  },
  /// Normalize audio volume
  Normalize {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long)]
    target: f32,
  },
  /// Generate a test tone (sine wave)
  GenTone {
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long)]
    duration: f64,
    #[arg(short, long)]
    freq: f64,
  },
  /// Apply a biquad filter
  Filter {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long, value_enum)]
    type_: CliFilterType,
    #[arg(short, long)]
    cutoff: f64,
    #[arg(short, long, default_value_t = 0.707)]
    q_value: f64,
    #[arg(short, long, default_value_t = 0.0)]
    gain: f64,
  },
  /// Change playback speed
  Speed {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long)]
    factor: f64,
  },
  /// Convert audio format (WAV, MP3, FLAC, OGG)
  Convert {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    /// MP3 bitrate in kbps (default: 192)
    #[arg(short, long, default_value_t = 192)]
    bitrate: u32,
  },
  /// Text-to-Speech: synthesize text to audio file
  Tts {
    /// Text to synthesize
    #[arg(short, long)]
    text: String,
    /// Voice ID (e.g. alloy, echo, nova, shimmer, fable, onyx)
    #[arg(short, long, default_value = "alloy")]
    voice: String,
    /// TTS model (tts-1 or tts-1-hd)
    #[arg(short, long, default_value = "tts-1")]
    model: String,
    /// Output file path
    #[arg(short, long)]
    output: PathBuf,
  },
  /// Speech-to-Text: transcribe audio using Whisper
  Stt {
    /// Audio file to transcribe
    #[arg(short, long)]
    input: PathBuf,
    /// Language hint (e.g. "en", "zh")
    #[arg(short, long)]
    language: Option<String>,
    /// Output JSON with timestamps instead of plain text
    #[arg(long, default_value_t = false)]
    json: bool,
  },
  /// Generate SRT subtitles from audio
  Subtitle {
    /// Audio file to transcribe
    #[arg(short, long)]
    input: PathBuf,
    /// Output SRT file path
    #[arg(short, long)]
    output: PathBuf,
    /// Language hint (e.g. "en", "zh")
    #[arg(short, long)]
    language: Option<String>,
  },
  /// Detect chapters in audio (via silence detection)
  #[command(allow_negative_numbers = true)]
  Chapters {
    /// Audio file to analyze
    #[arg(short, long)]
    input: PathBuf,
    /// Silence threshold (dB, default: -40.0)
    #[arg(long, default_value_t = -40.0)]
    threshold_db: f32,
    /// Minimum silence duration in seconds (default: 1.0)
    #[arg(long, default_value_t = 1.0)]
    min_silence: f64,
  },
  /// Video audio operations
  Video {
    #[command(subcommand)]
    action: VideoAction,
  },
  /// Podcast generation from a JSON script
  Podcast {
    #[command(subcommand)]
    action: PodcastAction,
  },
  /// Measure or normalize loudness (ITU-R BS.1770-4 / EBU R128)
  #[command(allow_negative_numbers = true)]
  Lufs {
    /// Audio file to measure
    #[arg(short, long)]
    input: PathBuf,
    /// Output file. If set together with `--target`, normalize and write here.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Target integrated loudness in LUFS (e.g. -16 for podcasts).
    /// Required when `--output` is given.
    #[arg(long)]
    target: Option<f32>,
    /// True-peak ceiling in dBTP. Default: -1.0 (only used when normalizing).
    #[arg(long, default_value_t = -1.0)]
    true_peak: f32,
  },
  /// Write ID3v2.4 tags + chapters into an existing MP3 (in place)
  TagMp3 {
    /// MP3 file to tag (modified in place)
    #[arg(short, long)]
    input: PathBuf,
    /// Track title
    #[arg(long)]
    title: Option<String>,
    /// Artist / author
    #[arg(long)]
    artist: Option<String>,
    /// Album / show name
    #[arg(long)]
    album: Option<String>,
    /// Year (e.g. 2026)
    #[arg(long)]
    year: Option<i32>,
    /// Genre (e.g. Podcast)
    #[arg(long)]
    genre: Option<String>,
    /// Free-form comment
    #[arg(long)]
    comment: Option<String>,
    /// ISO-639-2 language code (e.g. eng, zho)
    #[arg(long)]
    language: Option<String>,
    /// Track number
    #[arg(long)]
    track: Option<u32>,
    /// Path to a JPEG or PNG cover image (extension determines MIME)
    #[arg(long)]
    cover: Option<PathBuf>,
    /// Path to a JSON file with chapter markers in the form
    /// `[{"title":"Intro","start_secs":0.0,"end_secs":15.0}, ...]`
    #[arg(long)]
    chapters: Option<PathBuf>,
  },
  /// One-shot: script -> tagged MP3 + sidecar SRT, with LUFS normalization
  #[command(allow_negative_numbers = true)]
  EpisodeExport {
    /// PodcastScript JSON path
    #[arg(short, long)]
    script: PathBuf,
    /// Output MP3 path (must end in .mp3)
    #[arg(short, long)]
    output: PathBuf,
    /// TTS provider
    #[arg(long, value_enum, default_value_t = TtsBackend::Edge)]
    provider: TtsBackend,
    /// MP3 bitrate in kbps
    #[arg(long, default_value_t = 192)]
    bitrate: u32,
    /// Target loudness in LUFS. Set to a non-finite value (e.g. NaN) to skip.
    #[arg(long, default_value_t = -16.0)]
    lufs: f32,
    /// True-peak ceiling in dBTP (used together with `--lufs`)
    #[arg(long, default_value_t = -1.0)]
    true_peak: f32,
    /// Sidecar SRT path. Defaults to the MP3 path with `.srt` extension.
    #[arg(long)]
    srt: Option<PathBuf>,
    /// Tags (override what's in the script title). Cover art uses `--cover`.
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    artist: Option<String>,
    #[arg(long)]
    album: Option<String>,
    #[arg(long)]
    year: Option<i32>,
    #[arg(long)]
    genre: Option<String>,
    #[arg(long)]
    language: Option<String>,
    #[arg(long)]
    track: Option<u32>,
    /// Path to a JPEG or PNG cover image
    #[arg(long)]
    cover: Option<PathBuf>,
  },
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum TtsBackend {
  /// Microsoft Edge TTS — free, no API key required.
  Edge,
  /// OpenAI TTS — needs `OPENAI_API_KEY`.
  Openai,
}

#[derive(Subcommand)]
enum VideoAction {
  /// Extract audio track from a video file
  Extract {
    /// Input video file
    #[arg(short, long)]
    input: PathBuf,
    /// Output audio file
    #[arg(short, long)]
    output: PathBuf,
    /// Target sample rate (optional)
    #[arg(long)]
    sample_rate: Option<u32>,
  },
  /// Replace audio track in a video file
  Replace {
    /// Input video file
    #[arg(short = 'v', long)]
    video: PathBuf,
    /// New audio file
    #[arg(short, long)]
    audio: PathBuf,
    /// Output video file
    #[arg(short, long)]
    output: PathBuf,
  },
  /// Probe streams in a media file
  Probe {
    /// Input media file
    #[arg(short, long)]
    input: PathBuf,
  },
}

#[derive(Subcommand)]
enum PodcastAction {
  /// Generate a complete podcast from a script file
  Generate {
    /// Path to the podcast script JSON file
    #[arg(short, long)]
    script: PathBuf,
    /// Output audio file path
    #[arg(short, long)]
    output: PathBuf,
  },
  /// Preview a single segment from a script
  Preview {
    /// Path to the podcast script JSON file
    #[arg(short, long)]
    script: PathBuf,
    /// Segment index to preview (0-based)
    #[arg(long)]
    segment: usize,
    /// Output audio file path
    #[arg(short, long)]
    output: PathBuf,
  },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt::init();

  let cli = Cli::parse();

  match &cli.command {
    Commands::Info { file } => {
      let buffer = phonon_io::read_audio(file)?;
      println!("File: {:?}", file);
      println!("Duration: {:.2}s", buffer.duration_secs());
      println!("Sample Rate: {} Hz", buffer.sample_rate);
      println!("Channels: {}", buffer.channels);
      println!("Samples: {}", buffer.samples.len());
      println!("Peak: {:.4}", algorithms::peak(&buffer));
      println!("RMS: {:.4}", algorithms::rms(&buffer));
      println!("Peak dBFS: {:.2} dB", algorithms::peak_dbfs(&buffer));
    }
    Commands::Play { file } => {
      let buffer = phonon_io::read_audio(file)?;
      println!("Playing {:?} ({:.2}s)...", file, buffer.duration_secs());

      let player = AudioPlayer::new()?;

      let device_rate = player.device_sample_rate();
      let mut play_buffer = if buffer.sample_rate != device_rate {
        println!("Resampling {}→{}Hz...", buffer.sample_rate, device_rate);
        algorithms::resample(&buffer, device_rate)?
      } else {
        buffer
      };

      let device_channels = player.device_channels();
      if play_buffer.channels != device_channels {
        println!(
          "Converting channels {}→{}...",
          play_buffer.channels, device_channels
        );
        play_buffer = algorithms::ensure_channels(&play_buffer, device_channels)?;
      }

      player.play(&play_buffer)?;
      thread::sleep(Duration::from_secs_f64(play_buffer.duration_secs() + 0.5));
      println!("Done.");
    }
    Commands::Trim {
      input,
      output,
      start,
      end,
    } => {
      let buffer = phonon_io::read_audio(input)?;
      let trimmed = algorithms::trim(&buffer, *start, *end)?;
      phonon_io::write_audio(output, &trimmed)?;
      println!("Trimmed {:.2}s→{:.2}s saved to {:?}", start, end, output);
    }
    Commands::Mix {
      input1,
      input2,
      output,
    } => {
      let b1 = phonon_io::read_audio(input1)?;
      let b2 = phonon_io::read_audio(input2)?;
      let mixed = algorithms::mix(&b1, &b2)?;
      phonon_io::write_audio(output, &mixed)?;
      println!("Mixed audio saved to {:?}", output);
    }
    Commands::Normalize {
      input,
      output,
      target,
    } => {
      let mut buffer = phonon_io::read_audio(input)?;
      algorithms::normalize(&mut buffer, *target);
      phonon_io::write_audio(output, &buffer)?;
      println!("Normalized audio saved to {:?}", output);
    }
    Commands::GenTone {
      output,
      duration,
      freq,
    } => {
      let buffer = algorithms::generate_tone(*freq, *duration, 44100)?;
      phonon_io::write_audio(output, &buffer)?;
      println!(
        "Generated {:.1}s sine wave at {:.0}Hz → {:?}",
        duration, freq, output
      );
    }
    Commands::Filter {
      input,
      output,
      type_,
      cutoff,
      q_value,
      gain,
    } => {
      let mut buffer = phonon_io::read_audio(input)?;
      algorithms::filter(&mut buffer, (*type_).into(), *cutoff, *q_value, *gain)?;
      phonon_io::write_audio(output, &buffer)?;
      println!("Applied {:?} filter at {}Hz → {:?}", type_, cutoff, output);
    }
    Commands::Speed {
      input,
      output,
      factor,
    } => {
      let buffer = phonon_io::read_audio(input)?;
      let result = algorithms::change_speed(&buffer, *factor)?;
      phonon_io::write_audio(output, &result)?;
      println!("Changed speed ×{} → {:?}", factor, output);
    }
    Commands::Convert {
      input,
      output,
      bitrate,
    } => {
      let buffer = phonon_io::read_audio(input)?;
      let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("wav");

      match ext.to_lowercase().as_str() {
        "mp3" => {
          let config = phonon_io::Mp3Config {
            bitrate_kbps: *bitrate,
          };
          phonon_io::write_mp3(output, &buffer, &config)?;
        }
        _ => {
          phonon_io::write_audio(output, &buffer)?;
        }
      }
      println!(
        "Converted {:?} → {:?} ({:.2}s)",
        input,
        output,
        buffer.duration_secs()
      );
    }
    Commands::Tts {
      text,
      voice,
      model,
      output,
    } => {
      println!("Synthesizing with voice '{}' (model: {})...", voice, model);
      let tts = OpenAiTts::new()?;
      let voice_config = VoiceConfig::new(voice.clone()).with_model(model.clone());
      let buffer = tts.synthesize(text, &voice_config).await?;
      phonon_io::write_audio(output, &buffer)?;
      println!(
        "Saved {:.2}s of audio → {:?}",
        buffer.duration_secs(),
        output
      );
    }
    Commands::Stt {
      input,
      language,
      json,
    } => {
      let stt = WhisperStt::new()?;
      let mut opts = TranscribeOptions::new();
      if let Some(lang) = language {
        opts = opts.with_language(lang);
      }

      println!("Transcribing {:?}...", input);
      let input_str = input.to_string_lossy();
      let result = stt.transcribe_file(&input_str, Some(opts)).await?;

      if *json {
        let json_out = serde_json::to_string_pretty(&serde_json::json!({
            "text": result.text,
            "segments": result.segments.iter().map(|s| {
                serde_json::json!({
                    "start": s.start,
                    "end": s.end,
                    "text": s.text,
                })
            }).collect::<Vec<_>>(),
        }))?;
        println!("{}", json_out);
      } else {
        println!("{}", result.text);
      }
    }
    Commands::Subtitle {
      input,
      language,
      output,
    } => {
      let stt = WhisperStt::new()?;
      let mut opts = TranscribeOptions::new();
      if let Some(lang) = language {
        opts = opts.with_language(lang);
      }

      println!("Transcribing {:?}...", input);
      let input_str = input.to_string_lossy();
      let result = stt.transcribe_file(&input_str, Some(opts)).await?;

      segments_to_srt_file(&result.segments, output)?;
      println!(
        "Generated SRT with {} entries → {:?}",
        result.segments.len(),
        output
      );
    }
    Commands::Chapters {
      input,
      threshold_db,
      min_silence,
    } => {
      let buffer = phonon_io::read_audio(input)?;
      // Convert dB threshold to linear amplitude
      let threshold_linear = (10.0f32).powf(*threshold_db / 20.0);
      let config = SilenceDetectConfig {
        threshold: threshold_linear,
        min_silence_secs: *min_silence,
      };
      let chapters = chapters_from_silence(&buffer, &config);
      let text = format_chapters_text(&chapters);
      println!("{}", text);
    }
    Commands::Video { action } => match action {
      VideoAction::Extract {
        input,
        output,
        sample_rate,
      } => {
        println!("Extracting audio from {:?}...", input);
        let buffer = phonon_video::extract_audio(input, *sample_rate).await?;
        phonon_io::write_audio(output, &buffer)?;
        println!(
          "Extracted {:.2}s audio → {:?}",
          buffer.duration_secs(),
          output
        );
      }
      VideoAction::Replace {
        video,
        audio,
        output,
      } => {
        println!("Replacing audio in {:?}...", video);
        phonon_video::replace_audio(video, audio, output).await?;
        println!("Saved → {:?}", output);
      }
      VideoAction::Probe { input } => {
        let streams = phonon_video::probe_streams(input).await?;
        for s in &streams {
          println!(
            "Stream #{}: {} ({}){}",
            s.index,
            s.codec_type,
            s.codec_name,
            s.duration
              .as_deref()
              .map_or(String::new(), |d| format!(" duration={}s", d)),
          );
          if let Some(sr) = &s.sample_rate {
            println!("  sample_rate: {} Hz", sr);
          }
          if let Some(ch) = s.channels {
            println!("  channels: {}", ch);
          }
          if let Some(br) = &s.bit_rate {
            println!("  bitrate: {}", br);
          }
        }
      }
    },
    Commands::Lufs {
      input,
      output,
      target,
      true_peak,
    } => {
      let mut buffer = phonon_io::read_audio(input)?;
      let report = phonon_core::measure_loudness(&buffer);
      println!("File: {:?}", input);
      println!("Duration:           {:.2} s", buffer.duration_secs());
      println!("Integrated:         {:>7.2} LUFS", report.integrated_lufs);
      println!(
        "Short-term max:     {:>7.2} LUFS",
        report.short_term_max_lufs
      );
      println!(
        "Momentary max:      {:>7.2} LUFS",
        report.momentary_max_lufs
      );
      println!("Loudness range:     {:>7.2} LU", report.range_lu);
      println!("True peak:          {:>7.2} dBTP", report.true_peak_dbtp);
      println!("Sample peak:        {:>7.4}", phonon_core::peak(&buffer));
      println!("RMS:                {:>7.4}", phonon_core::rms(&buffer));

      if let Some(out_path) = output {
        let target_lufs = target
          .ok_or_else(|| anyhow::anyhow!("--target <LUFS> is required when --output is set"))?;
        phonon_core::normalize_lufs(&mut buffer, target_lufs, *true_peak);
        phonon_io::write_audio(out_path, &buffer)?;
        let after = phonon_core::measure_loudness(&buffer);
        println!(
          "Normalized to {:.2} LUFS / true peak {:.2} dBTP -> {:?}",
          after.integrated_lufs, after.true_peak_dbtp, out_path
        );
      }
    }
    Commands::TagMp3 {
      input,
      title,
      artist,
      album,
      year,
      genre,
      comment,
      language,
      track,
      cover,
      chapters,
    } => {
      let tags = PodcastTags {
        title: title.clone(),
        artist: artist.clone(),
        album: album.clone(),
        year: *year,
        genre: genre.clone(),
        comment: comment.clone(),
        language: language.clone(),
        track: *track,
        cover: load_cover(cover.as_deref())?,
      };
      let chapter_list = match chapters.as_deref() {
        Some(p) => load_chapters_json(p)?,
        None => Vec::new(),
      };
      attach_tag(input, &tags, &chapter_list)?;
      println!(
        "Tagged {:?} ({} chapters, cover: {})",
        input,
        chapter_list.len(),
        tags.cover.is_some(),
      );
    }
    Commands::EpisodeExport {
      script,
      output,
      provider,
      bitrate,
      lufs,
      true_peak,
      srt,
      title,
      artist,
      album,
      year,
      genre,
      language,
      track,
      cover,
    } => {
      if output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        != Some("mp3".to_string())
      {
        anyhow::bail!("--output must be a .mp3 path (got {:?})", output);
      }

      println!("Loading script from {:?}...", script);
      let podcast_script = PodcastScript::from_file(script)?;
      println!(
        "'{}' — {} segments, {} speakers",
        podcast_script.title,
        podcast_script.segment_count(),
        podcast_script.speaker_names().len()
      );

      // Generate the episode (audio + chapters + SRT) using the chosen TTS provider.
      // We branch here because `PodcastPipeline<T>` is generic over the provider.
      let mut episode = match provider {
        TtsBackend::Edge => {
          println!("Provider: Edge TTS (free, keyless)");
          let pipeline = PodcastPipeline::new(EdgeTts::new());
          pipeline.generate_episode(&podcast_script).await?
        }
        TtsBackend::Openai => {
          println!("Provider: OpenAI TTS");
          let tts = OpenAiTts::new()?;
          let pipeline = PodcastPipeline::new(tts);
          pipeline.generate_episode(&podcast_script).await?
        }
      };

      if lufs.is_finite() {
        println!(
          "Loudness-normalizing to {:.1} LUFS / {:.1} dBTP...",
          lufs, true_peak
        );
        phonon_core::normalize_lufs(&mut episode.audio, *lufs, *true_peak);
      }

      // Title in tags falls back to the script title.
      let resolved_title = title.clone().or_else(|| Some(podcast_script.title.clone()));

      let tags = PodcastTags {
        title: resolved_title,
        artist: artist.clone(),
        album: album.clone(),
        year: *year,
        genre: genre.clone(),
        comment: None,
        language: language.clone(),
        track: *track,
        cover: load_cover(cover.as_deref())?,
      };
      episode = episode.with_tags(tags);

      let mp3_cfg = Mp3Config {
        bitrate_kbps: *bitrate,
      };
      episode.export_mp3(output, &mp3_cfg)?;
      println!(
        "Exported tagged MP3 -> {:?} ({} chapters, {:.2} s)",
        output,
        episode.chapters.len(),
        episode.audio.duration_secs(),
      );

      let srt_path = srt.clone().unwrap_or_else(|| output.with_extension("srt"));
      if episode.srt.is_some() {
        episode.write_srt(&srt_path)?;
        println!("Wrote SRT -> {:?}", srt_path);
      }
    }
    Commands::Podcast { action } => match action {
      PodcastAction::Generate { script, output } => {
        println!("Loading script from {:?}...", script);
        let podcast_script = PodcastScript::from_file(script)?;
        println!(
          "Script '{}': {} segments, {} speakers",
          podcast_script.title,
          podcast_script.segment_count(),
          podcast_script.speaker_names().len()
        );

        let tts = OpenAiTts::new()?;
        let pipeline = PodcastPipeline::new(tts);

        println!("Generating podcast...");
        pipeline.generate_to_file(&podcast_script, output).await?;
        println!("Podcast saved to {:?}", output);
      }
      PodcastAction::Preview {
        script,
        segment,
        output,
      } => {
        let podcast_script = PodcastScript::from_file(script)?;
        let tts = OpenAiTts::new()?;
        let pipeline = PodcastPipeline::new(tts);

        println!("Previewing segment {}...", segment);
        let buffer = pipeline.preview_segment(&podcast_script, *segment).await?;
        phonon_io::write_audio(output, &buffer)?;
        println!(
          "Preview saved to {:?} ({:.2}s)",
          output,
          buffer.duration_secs()
        );
      }
    },
  }

  Ok(())
}

/// Load a cover image from disk for embedding into a tag.
///
/// MIME is inferred from the file extension. Returns `Ok(None)` when no path
/// is supplied so callers can pass the result straight into a `PodcastTags`.
fn load_cover(path: Option<&std::path::Path>) -> anyhow::Result<Option<CoverArt>> {
  let Some(p) = path else { return Ok(None) };
  let ext = p
    .extension()
    .and_then(|e| e.to_str())
    .map(|e| e.to_lowercase())
    .unwrap_or_default();
  let mime = match ext.as_str() {
    "jpg" | "jpeg" => CoverMime::Jpeg,
    "png" => CoverMime::Png,
    other => anyhow::bail!("unsupported cover image extension '.{other}': use .jpg or .png"),
  };
  let data = std::fs::read(p)?;
  Ok(Some(CoverArt {
    mime,
    data,
    description: None,
  }))
}

/// Parse a chapter list from JSON. Accepts the canonical
/// `[{"title":"…","start_secs":0.0,"end_secs":1.0}, …]` shape used by the
/// `tag-mp3` subcommand.
fn load_chapters_json(path: &std::path::Path) -> anyhow::Result<Vec<AudioChapter>> {
  let text = std::fs::read_to_string(path)?;
  let raw: serde_json::Value = serde_json::from_str(&text)?;
  let arr = raw
    .as_array()
    .ok_or_else(|| anyhow::anyhow!("chapters JSON must be a top-level array"))?;
  let mut chapters = Vec::with_capacity(arr.len());
  for (i, item) in arr.iter().enumerate() {
    let title = item
      .get("title")
      .and_then(|v| v.as_str())
      .ok_or_else(|| anyhow::anyhow!("chapter {i}: missing string field 'title'"))?;
    let start_secs = item
      .get("start_secs")
      .and_then(|v| v.as_f64())
      .ok_or_else(|| anyhow::anyhow!("chapter {i}: missing number field 'start_secs'"))?;
    let end_secs = item
      .get("end_secs")
      .and_then(|v| v.as_f64())
      .ok_or_else(|| anyhow::anyhow!("chapter {i}: missing number field 'end_secs'"))?;
    chapters.push(AudioChapter::new(title.to_string(), start_secs, end_secs));
  }
  Ok(chapters)
}

#[cfg(test)]
mod helper_tests {
  use super::*;
  use std::io::Write;

  #[test]
  fn test_load_cover_none() {
    let result = load_cover(None).unwrap();
    assert!(result.is_none());
  }

  #[test]
  fn test_load_cover_jpg() {
    let mut tmp = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    tmp.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let cover = load_cover(Some(tmp.path())).unwrap().unwrap();
    assert_eq!(cover.mime, CoverMime::Jpeg);
    assert_eq!(cover.data, vec![0xFF, 0xD8, 0xFF, 0xE0]);
  }

  #[test]
  fn test_load_cover_unknown_extension() {
    let tmp = tempfile::Builder::new().suffix(".gif").tempfile().unwrap();
    let err = load_cover(Some(tmp.path())).unwrap_err();
    assert!(
      err
        .to_string()
        .contains("unsupported cover image extension")
    );
  }

  #[test]
  fn test_load_chapters_json_roundtrip() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    let json = r#"[
      {"title":"Intro","start_secs":0.0,"end_secs":1.5},
      {"title":"Body","start_secs":1.5,"end_secs":4.0}
    ]"#;
    tmp.write_all(json.as_bytes()).unwrap();
    let chapters = load_chapters_json(tmp.path()).unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "Intro");
    assert!((chapters[0].end_secs - 1.5).abs() < 1e-9);
    assert!((chapters[1].start_secs - 1.5).abs() < 1e-9);
  }

  #[test]
  fn test_load_chapters_json_rejects_non_array() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(b"{\"foo\":1}").unwrap();
    assert!(load_chapters_json(tmp.path()).is_err());
  }

  #[test]
  fn test_load_chapters_json_missing_field() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp
      .write_all(b"[{\"title\":\"X\",\"start_secs\":0.0}]")
      .unwrap();
    let err = load_chapters_json(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("end_secs"));
  }
}
