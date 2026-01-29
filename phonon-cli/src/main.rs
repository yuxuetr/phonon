use phonon_core::{algorithms, AudioBuffer, FilterType};
use phonon_engine::AudioPlayer;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use clap::{Parser, Subcommand, ValueEnum};

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
#[command(about = "Phonon Audio Engine CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect a WAV file
    Info {
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Play a WAV file
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
    /// Mix two files
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
    /// Generate a test tone
    GenTone {
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short, long)]
        duration: f64,
        #[arg(short, long)]
        freq: f64,
    },
    /// Apply a Biquad filter
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
    /// Change speed (Vinyl effect)
    Speed {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short, long)]
        factor: f64,
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Info { file } => {
            let buffer = phonon_wav::read_wav(file)?;
            println!("File: {:?}", file);
            println!("Duration: {:.2}s", buffer.duration_secs());
            println!("Sample Rate: {} Hz", buffer.sample_rate);
            println!("Channels: {}", buffer.channels);
            println!("Samples: {}", buffer.samples.len());
        }
        Commands::Play { file } => {
            let buffer = phonon_wav::read_wav(file)?;
            println!("Playing {:?} ({:.2}s)...", file, buffer.duration_secs());
            
            let player = AudioPlayer::new()?;
            println!("Device: {} Hz, {} channels", player.device_sample_rate(), 2); // Assuming stereo for now if we can't get channels easily
            
            // Check compatibility before playing
            let device_rate = player.device_sample_rate();
            let mut play_buffer = if buffer.sample_rate != device_rate {
                println!("Resampling from {}Hz to {}Hz...", buffer.sample_rate, device_rate);
                algorithms::resample(&buffer, device_rate)?
            } else {
                buffer
            };

            let device_channels = player.device_channels();
            if play_buffer.channels != device_channels {
                println!("Converting channels from {} to {}...", play_buffer.channels, device_channels);
                play_buffer = algorithms::ensure_channels(&play_buffer, device_channels)?;
            }
            
            player.play(&play_buffer)?;
            
            // Wait for playback to finish
            thread::sleep(Duration::from_secs_f64(play_buffer.duration_secs() + 0.5));
            println!("Done.");
            
            // Wait for playback to finish
            thread::sleep(Duration::from_secs_f64(play_buffer.duration_secs() + 0.5));
            println!("Done.");
        }
        Commands::Trim { input, output, start, end } => {
            let buffer = phonon_wav::read_wav(input)?;
            let trimmed = algorithms::trim(&buffer, *start, *end)?;
            phonon_wav::write_wav(output, &trimmed)?;
            println!("Trimmed audio saved to {:?}", output);
        }
        Commands::Mix { input1, input2, output } => {
            let b1 = phonon_wav::read_wav(input1)?;
            let b2 = phonon_wav::read_wav(input2)?;
            let mixed = algorithms::mix(&b1, &b2)?;
            phonon_wav::write_wav(output, &mixed)?;
            println!("Mixed audio saved to {:?}", output);
        }
        Commands::Normalize { input, output, target } => {
            let mut buffer = phonon_wav::read_wav(input)?;
            algorithms::normalize(&mut buffer, *target);
            phonon_wav::write_wav(output, &buffer)?;
            println!("Normalized audio saved to {:?}", output);
        }
        Commands::GenTone { output, duration, freq } => {
            let sample_rate = 44100;
            let channels = 1;
            let samples_count = (duration * sample_rate as f64) as usize;
            let mut samples = Vec::with_capacity(samples_count);
            
            for i in 0..samples_count {
                let t = i as f64 / sample_rate as f64;
                let sample = (t * freq * 2.0 * std::f64::consts::PI).sin() as f32;
                samples.push(sample);
            }
            
            let buffer = AudioBuffer {
                samples,
                sample_rate,
                channels,
            };
            
            phonon_wav::write_wav(output, &buffer)?;
            println!("Generated {}s sine wave at {}Hz to {:?}", duration, freq, output);
        }
        Commands::Filter { input, output, type_, cutoff, q_value, gain } => {
            let mut buffer = phonon_wav::read_wav(input)?;
            algorithms::filter(&mut buffer, (*type_).into(), *cutoff, *q_value, *gain)?;
            phonon_wav::write_wav(output, &buffer)?;
            println!("Applied {:?} filter at {}Hz to {:?}", type_, cutoff, output);
        }
        Commands::Speed { input, output, factor } => {
            let buffer = phonon_wav::read_wav(input)?;
            let new_buffer = algorithms::change_speed(&buffer, *factor)?;
            phonon_wav::write_wav(output, &new_buffer)?;
            println!("Changed speed by factor {} to {:?}", factor, output);
        }
    }

    Ok(())
}
