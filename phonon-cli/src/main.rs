use clap::{Parser, Subcommand, ValueEnum};
use phonon_ai::{OpenAiTts, SttProvider, TranscribeOptions, TtsProvider, VoiceConfig, WhisperStt};
use phonon_core::{FilterType, algorithms};
use phonon_engine::AudioPlayer;
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
