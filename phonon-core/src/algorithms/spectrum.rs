use crate::{AudioBuffer, Result};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Computes the Fast Fourier Transform (FFT) of the audio buffer.
///
/// Returns the magnitude spectrum (amplitude vs frequency).
///
/// # Arguments
///
/// * `buffer` - The input audio buffer.
///
/// # Returns
///
/// A vector of vectors, where each inner vector corresponds to a channel,
/// and contains the magnitude of each frequency bin.
/// The size of the FFT is the next power of two of the buffer length.
pub fn compute_spectrum(buffer: &AudioBuffer) -> Result<Vec<Vec<f32>>> {
    if buffer.samples.is_empty() {
        return Ok(vec![]);
    }

    let channels = buffer.channels as usize;
    let frames = buffer.samples.len() / channels;

    // Use next power of 2 for efficiency, or just use length if we don't care about zero padding.
    // Usually visualization uses fixed window size (e.g. 2048).
    // Here we analyze the *whole* buffer for simplicity, assuming short clips.
    // For long clips, this function should take a window size.
    // Let's implement full buffer FFT for now, but padded to power of 2.
    let fft_len = frames.next_power_of_two();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_len);

    let mut spectra = Vec::with_capacity(channels);

    for ch in 0..channels {
        // Prepare input buffer (padded with zeros)
        let mut input: Vec<Complex<f32>> = Vec::with_capacity(fft_len);
        
        for i in 0..frames {
            let val = buffer.samples[i * channels + ch];
            input.push(Complex { re: val, im: 0.0 });
        }
        // Zero padding
        for _ in frames..fft_len {
            input.push(Complex { re: 0.0, im: 0.0 });
        }

        // Run FFT
        fft.process(&mut input);

        // Compute magnitude (first half + DC)
        // Nyquist is at fft_len / 2
        let output_len = fft_len / 2 + 1;
        let mut magnitudes = Vec::with_capacity(output_len);

        for item in input.iter().take(output_len) {
            // Norm: Divide by N to get true amplitude? 
            // Standard definition usually doesn't, but for "magnitude", we normally take abs / N.
            // Let's return raw magnitude (norm) for now.
            let mag = item.norm() / fft_len as f32; 
            magnitudes.push(mag);
        }

        spectra.push(magnitudes);
    }

    Ok(spectra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spectrum_pure_tone() {
        // Use power of 2 to avoid spectral leakage from zero padding in this simple test
        let sample_rate = 1024; 
        let freq = 128.0; // 128 Hz tone (exactly bin 128)
        let len = 1024; // 1 second
        
        let mut samples = Vec::new();
        for i in 0..len {
            let t = i as f32 / sample_rate as f32;
            let s = (t * freq * 2.0 * std::f32::consts::PI).sin();
            samples.push(s);
        }
        
        let buffer = AudioBuffer {
            samples,
            sample_rate,
            channels: 1,
        };
        
        let spectra = compute_spectrum(&buffer).unwrap();
        let mags = &spectra[0];
        
        // FFT len is 1024.
        // Bin 128 should be the peak.
        
        let mut max_mag = 0.0;
        let mut max_idx = 0;
        for (i, &mag) in mags.iter().enumerate() {
            if mag > max_mag {
                max_mag = mag;
                max_idx = i;
            }
        }
        
        assert_eq!(max_idx, 128, "Peak should be exactly at bin 128");
        // Magnitude of sin wave 1.0 amplitude is 0.5 in spectrum (split between pos and neg freq)
        assert!((max_mag - 0.5).abs() < 0.001, "Magnitude should be approx 0.5, got {}", max_mag);
    }
}
