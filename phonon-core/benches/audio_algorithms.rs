use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use phonon_core::{
    compress, compute_spectrum, crossfade, generate_signal, mix, normalize, resample,
    AudioBuffer, CompressorConfig, Waveform,
};

/// Create a mono sine-wave buffer of given duration at 44100 Hz.
fn make_sine(duration_secs: f64, sample_rate: u32) -> AudioBuffer {
    let num_samples = (duration_secs * sample_rate as f64) as usize;
    let freq = 440.0_f32;
    let mut samples = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        samples.push((t * freq * 2.0 * std::f32::consts::PI).sin() * 0.8);
    }
    AudioBuffer {
        samples,
        sample_rate,
        channels: 1,
    }
}

fn bench_resample(c: &mut Criterion) {
    let mut group = c.benchmark_group("resample");

    for &duration in &[0.5, 1.0, 5.0] {
        let buf = make_sine(duration, 44100);
        group.bench_with_input(
            BenchmarkId::new("44100→48000", format!("{duration}s")),
            &buf,
            |b, buf| {
                b.iter(|| resample(buf, 48000));
            },
        );
    }

    // Downsample benchmark
    let buf = make_sine(1.0, 48000);
    group.bench_with_input(
        BenchmarkId::new("48000→16000", "1s"),
        &buf,
        |b, buf| {
            b.iter(|| resample(buf, 16000));
        },
    );

    group.finish();
}

fn bench_mix(c: &mut Criterion) {
    let mut group = c.benchmark_group("mix");

    for &duration in &[0.5, 1.0, 5.0] {
        let buf1 = make_sine(duration, 44100);
        let buf2 = make_sine(duration, 44100);
        group.bench_with_input(
            BenchmarkId::new("mono", format!("{duration}s")),
            &(buf1, buf2),
            |b, (b1, b2)| {
                b.iter(|| mix(b1, b2));
            },
        );
    }

    group.finish();
}

fn bench_normalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalize");

    for &duration in &[0.5, 1.0, 5.0] {
        let buf = make_sine(duration, 44100);
        group.bench_with_input(
            BenchmarkId::new("to_peak_1.0", format!("{duration}s")),
            &buf,
            |b, buf| {
                let mut clone = buf.clone();
                b.iter(|| {
                    clone.samples.copy_from_slice(&buf.samples);
                    normalize(&mut clone, 1.0);
                });
            },
        );
    }

    group.finish();
}

fn bench_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("compress");
    let configs = [
        ("voice", CompressorConfig::voice()),
        ("gentle", CompressorConfig::gentle()),
        ("limiter", CompressorConfig::limiter()),
    ];

    let buf = make_sine(1.0, 44100);

    for (name, config) in &configs {
        group.bench_with_input(BenchmarkId::new(*name, "1s"), &buf, |b, buf| {
            let mut clone = buf.clone();
            b.iter(|| {
                clone.samples.copy_from_slice(&buf.samples);
                compress(&mut clone, config);
            });
        });
    }

    // Longer buffer with voice preset
    let buf5 = make_sine(5.0, 44100);
    group.bench_with_input(BenchmarkId::new("voice", "5s"), &buf5, |b, buf| {
        let mut clone = buf.clone();
        b.iter(|| {
            clone.samples.copy_from_slice(&buf.samples);
            compress(&mut clone, &CompressorConfig::voice());
        });
    });

    group.finish();
}

fn bench_spectrum(c: &mut Criterion) {
    let mut group = c.benchmark_group("fft_spectrum");

    for &duration in &[0.1, 0.5, 1.0] {
        let buf = make_sine(duration, 44100);
        group.bench_with_input(
            BenchmarkId::new("mono", format!("{duration}s")),
            &buf,
            |b, buf| {
                b.iter(|| compute_spectrum(buf));
            },
        );
    }

    group.finish();
}

fn bench_crossfade(c: &mut Criterion) {
    let mut group = c.benchmark_group("crossfade");

    let buf1 = make_sine(2.0, 44100);
    let buf2 = make_sine(2.0, 44100);

    for &overlap_ms in &[100, 500, 1000] {
        let overlap = overlap_ms as f64 / 1000.0;
        group.bench_with_input(
            BenchmarkId::new("2s+2s", format!("{overlap_ms}ms")),
            &(buf1.clone(), buf2.clone()),
            |b, (b1, b2)| {
                b.iter(|| crossfade(b1, b2, overlap));
            },
        );
    }

    group.finish();
}

fn bench_generators(c: &mut Criterion) {
    let mut group = c.benchmark_group("generate_signal");

    for waveform in &[
        Waveform::Sine,
        Waveform::Square,
        Waveform::Sawtooth,
        Waveform::WhiteNoise,
    ] {
        group.bench_with_input(
            BenchmarkId::new(format!("{waveform:?}"), "1s@44100"),
            waveform,
            |b, wf| {
                b.iter(|| generate_signal(*wf, 440.0, 1.0, 44100, 0.8));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_resample,
    bench_mix,
    bench_normalize,
    bench_compress,
    bench_spectrum,
    bench_crossfade,
    bench_generators,
);
criterion_main!(benches);
