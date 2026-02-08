use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::{info, instrument};

/// Reads an audio file and decodes it into an AudioBuffer.
///
/// Supports all formats supported by Symphonia (MP3, WAV, FLAC, OGG, AAC, etc.).
#[instrument(skip(path), fields(path = %path.as_ref().display()))]
pub fn read_audio<P: AsRef<Path>>(path: P) -> Result<AudioBuffer> {
    let path = path.as_ref();
    let src = File::open(path).map_err(|e| PhononError::Io(e))?;
    
    // Create the media source stream.
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    // Create a probe hint using the file's extension. [Optional]
    let mut hint = Hint::new();
    if let Some(extension) = path.extension() {
        if let Some(extension_str) = extension.to_str() {
            hint.with_extension(extension_str);
        }
    }

    // Use the default options for metadata and format readers.
    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();

    // Probe the media source.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &fmt_opts, &meta_opts)
        .map_err(|e| PhononError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    // Get the instantiated format reader.
    let mut format = probed.format;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| PhononError::Wav("No supported audio track found".to_string()))?;

    // Use the default options for the decoder.
    let dec_opts: DecoderOptions = Default::default();

    // Create a decoder for the track.
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| PhononError::Wav(format!("Unsupported codec: {}", e)))?;

    // Store the track identifier, it will be used to filter packets.
    let track_id = track.id;

    let mut samples: Vec<f32> = Vec::new();
    let mut sample_rate = 0;
    let mut channels = 0;

    // The decode loop.
    loop {
        // Get the next packet from the media format.
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break; // End of stream
            }
            Err(e) => return Err(PhononError::Wav(format!("Failed to read packet: {}", e))),
        };

        // Consume any new metadata that has been read since the last packet.
        while !format.metadata().is_latest() {
            format.metadata().pop();
        }

        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet into audio samples.
        match decoder.decode(&packet) {
            Ok(decoded) => {
                // Get the audio buffer specification.
                let spec = *decoded.spec();
                
                // Initialize metadata on first packet
                if sample_rate == 0 {
                    sample_rate = spec.rate;
                    channels = spec.channels.count() as u16;
                }

                // Copy the decoded audio samples into a SampleBuffer for easy conversion.
                let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(decoded);

                // The samples are now floating point numbers.
                samples.extend_from_slice(sample_buf.samples());
            }
            Err(Error::IoError(e)) => return Err(PhononError::Io(e)),
            Err(Error::DecodeError(e)) => return Err(PhononError::Wav(format!("Decode error: {}", e))),
            Err(e) => return Err(PhononError::Wav(format!("Unhandled decode error: {}", e))),
        }
    }

    info!(samples = samples.len(), sample_rate, channels, "Audio loaded");

    Ok(AudioBuffer {
        samples,
        sample_rate,
        channels,
    })
}
