#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_logger::tracing;
use phonon_core::{algorithms, AudioBuffer};
use phonon_engine::AudioPlayer;
use std::sync::Arc;

fn main() {
    // Init logger
    if let Err(e) = dioxus_logger::init(tracing::Level::INFO) {
        eprintln!("Failed to init logger: {}", e);
    }

    launch(App);
}

/// Lightweight display metadata (no raw samples — safe to clone on every render)
#[derive(Clone, Debug, PartialEq)]
struct AudioDisplayInfo {
    path: String,
    sample_rate: u32,
    channels: u16,
    duration: f64,
    sample_count: usize,
    waveform_path: String,
    rms: f32,
    peak: f32,
    peak_dbfs: f32,
}

/// Downsamples audio data for visualization (called once at load time)
fn generate_waveform_path(samples: &[f32], width: f32, height: f32) -> String {
    if samples.is_empty() {
        return String::new();
    }

    let samples_per_pixel = (samples.len() as f32 / width).ceil() as usize;
    let step = samples_per_pixel.max(1);
    let mid_y = height / 2.0;
    let scale_y = height / 2.0;

    // Pre-allocate a reasonable capacity to avoid repeated allocations
    let mut path = String::with_capacity(width as usize * 20);
    path.push_str(&format!("M 0 {}", mid_y));

    // Simple peak finding for each pixel bucket
    for x in 0..width as usize {
        let start = x * step;
        let end = (start + step).min(samples.len());

        if start >= samples.len() {
            break;
        }

        let mut max_val = 0.0f32;
        for i in start..end {
            if let Some(s) = samples.get(i) {
                let abs = s.abs();
                if abs > max_val {
                    max_val = abs;
                }
            }
        }

        let y = mid_y - (max_val * scale_y);
        path.push_str(&format!(" L {} {:.2}", x, y));
    }

    // Mirror for the bottom half
    for x in (0..width as usize).rev() {
        let start = x * step;
        let end = (start + step).min(samples.len());

        if start >= samples.len() {
            continue;
        }

        let mut max_val = 0.0f32;
        for i in start..end {
            if let Some(s) = samples.get(i) {
                let abs = s.abs();
                if abs > max_val {
                    max_val = abs;
                }
            }
        }
        let y = mid_y + (max_val * scale_y);
        path.push_str(&format!(" L {} {:.2}", x, y));
    }

    path.push_str(" Z");
    path
}

/// Tailwind CSS asset for desktop and web
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// Effect parameters controlled by the user.
#[derive(Clone, Debug, PartialEq)]
struct EffectParams {
    normalize_target: f32,
    fade_in_secs: f64,
    fade_out_secs: f64,
    speed_factor: f64,
    compress_preset: String,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            normalize_target: 0.9,
            fade_in_secs: 0.0,
            fade_out_secs: 0.0,
            speed_factor: 1.0,
            compress_preset: "none".to_string(),
        }
    }
}

fn App() -> Element {
    let mut display_info = use_signal(|| None::<AudioDisplayInfo>);
    let mut audio_buffer = use_signal(|| None::<Arc<AudioBuffer>>);
    let mut status_msg = use_signal(|| "Ready".to_string());
    let mut player_state = use_signal(|| None::<AudioPlayer>);
    let mut effects = use_signal(EffectParams::default);
    let mut show_effects = use_signal(|| false);
    let mut show_export = use_signal(|| false);
    let mut export_bitrate = use_signal(|| 192_u32);

    rsx! {
        document::Stylesheet { href: TAILWIND_CSS }
        div {
            class: "flex flex-col h-dvh bg-neutral-900 text-gray-200 font-sans overflow-hidden",

            // --- Header ---
            header {
                class: "bg-neutral-800 p-4 flex items-center justify-between border-b border-neutral-700",
                div { class: "font-bold text-xl text-gray-100 flex items-center gap-2", "🎛️ Phonon Studio" }
                div {
                    class: "flex gap-2",
                    button {
                        class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium",
                        onclick: move |_| {
                            show_effects.set(!show_effects());
                        },
                        "Effects"
                    }
                    button {
                        class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium",
                        onclick: move |_| {
                            show_export.set(!show_export());
                        },
                        "Export"
                    }
                    button {
                        class: "px-4 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-blue-600 text-white font-medium",
                        onclick: move |_| {
                            async move {
                                status_msg.set("Opening file dialog...".to_string());
                                let file_handle = rfd::AsyncFileDialog::new()
                                    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"])
                                    .pick_file()
                                    .await;

                                let Some(path) = file_handle else {
                                    status_msg.set("Cancelled".to_string());
                                    return;
                                };

                                let path_buf = path.path().to_path_buf();
                                let path_str = path_buf.to_string_lossy().to_string();
                                status_msg.set(format!("Loading {}...", path_buf.display()));

                                // Decode audio on a background thread to keep UI responsive
                                let load_result = tokio::task::spawn_blocking(move || {
                                    phonon_io::read_audio(&path_buf)
                                })
                                .await;

                                let buffer = match load_result {
                                    Ok(Ok(buf)) => buf,
                                    Ok(Err(e)) => {
                                        status_msg.set(format!("Error: {}", e));
                                        return;
                                    }
                                    Err(e) => {
                                        status_msg.set(format!("Task error: {}", e));
                                        return;
                                    }
                                };

                                status_msg.set("Generating waveform...".to_string());

                                // Pre-compute waveform path (once, not every render)
                                let waveform = generate_waveform_path(&buffer.samples, 1000.0, 200.0);

                display_info.set(Some(AudioDisplayInfo {
                                    path: path_str,
                                    sample_rate: buffer.sample_rate,
                                    channels: buffer.channels,
                                    duration: buffer.duration_secs(),
                                    sample_count: buffer.samples.len(),
                                    waveform_path: waveform,
                                    rms: algorithms::rms(&buffer),
                                    peak: algorithms::peak(&buffer),
                                    peak_dbfs: algorithms::peak_dbfs(&buffer),
                                }));
                                audio_buffer.set(Some(Arc::new(buffer)));
                                status_msg.set("File loaded successfully".to_string());
                            }
                        },
                        "Open Audio File"
                    }
                }
            }

            // --- Main Workspace ---
            div {
                class: "flex-1 flex overflow-hidden",

                // --- Left: main content ---
                main {
                    class: "flex-1 flex flex-col items-center justify-start p-6 overflow-y-auto",

                    if let Some(info) = display_info() {
                        // Waveform View
                        div {
                            class: "w-full max-w-5xl bg-neutral-800 border border-neutral-700 rounded-lg p-5 mb-5 shadow-lg",
                        svg {
                            width: "100%",
                            height: "200",
                            view_box: "0 0 1000 200",
                            preserve_aspect_ratio: "none",
                            path {
                                d: "{info.waveform_path}",
                                class: "stroke-emerald-400 fill-emerald-400",
                                stroke_width: "1",
                                vector_effect: "non-scaling-stroke"
                            }
                        }
                    }

                        // Info Panel
                        div {
                            class: "bg-neutral-800 p-4 rounded-lg border border-neutral-700 w-full max-w-5xl mt-5",
                            div { class: "grid grid-cols-2 md:grid-cols-4 gap-4",
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "File Path" } span { class: "font-medium truncate", "{info.path}" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Sample Rate" } span { class: "font-medium", "{info.sample_rate} Hz" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Channels" } span { class: "font-medium", "{info.channels}" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Duration" } span { class: "font-medium", "{info.duration:.2}s" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Samples" } span { class: "font-medium", "{info.sample_count}" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "RMS" } span { class: "font-medium", "{info.rms:.4}" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Peak" } span { class: "font-medium", "{info.peak:.4}" } }
                                div { class: "flex flex-col", label { class: "text-xs text-gray-500 mb-1", "Peak dBFS" } span { class: "font-medium", "{info.peak_dbfs:.1} dB" } }
                            }
                        }
                    } else {
                        // Empty State
                        div {
                            class: "text-center text-gray-500",
                            h2 { class: "text-2xl font-bold mb-2", "No Audio Loaded" }
                            p { "Click 'Open Audio File' to start editing." }
                        }
                    }
                }

                // --- Right sidebar: Effects & Export ---
                if show_effects() || show_export() {
                    aside {
                        class: "w-72 bg-neutral-800 border-l border-neutral-700 p-4 overflow-y-auto flex flex-col gap-4",

                        // Effects Panel
                        if show_effects() {
                            div {
                                h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3", "Effects" }

                                // Normalize
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "Normalize Peak" }
                                    input {
                                        r#type: "range",
                                        min: "0.1",
                                        max: "1.0",
                                        step: "0.05",
                                        value: "{effects().normalize_target}",
                                        class: "w-full accent-emerald-500",
                                        oninput: move |evt| {
                                            if let Ok(v) = evt.value().parse::<f32>() {
                                                effects.write().normalize_target = v;
                                            }
                                        },
                                    }
                                    span { class: "text-xs text-gray-400", "{effects().normalize_target:.2}" }
                                }

                                // Fade In
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "Fade In (s)" }
                                    input {
                                        r#type: "range",
                                        min: "0",
                                        max: "5",
                                        step: "0.1",
                                        value: "{effects().fade_in_secs}",
                                        class: "w-full accent-emerald-500",
                                        oninput: move |evt| {
                                            if let Ok(v) = evt.value().parse::<f64>() {
                                                effects.write().fade_in_secs = v;
                                            }
                                        },
                                    }
                                    span { class: "text-xs text-gray-400", "{effects().fade_in_secs:.1}s" }
                                }

                                // Fade Out
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "Fade Out (s)" }
                                    input {
                                        r#type: "range",
                                        min: "0",
                                        max: "5",
                                        step: "0.1",
                                        value: "{effects().fade_out_secs}",
                                        class: "w-full accent-emerald-500",
                                        oninput: move |evt| {
                                            if let Ok(v) = evt.value().parse::<f64>() {
                                                effects.write().fade_out_secs = v;
                                            }
                                        },
                                    }
                                    span { class: "text-xs text-gray-400", "{effects().fade_out_secs:.1}s" }
                                }

                                // Speed
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "Speed" }
                                    input {
                                        r#type: "range",
                                        min: "0.25",
                                        max: "4.0",
                                        step: "0.05",
                                        value: "{effects().speed_factor}",
                                        class: "w-full accent-emerald-500",
                                        oninput: move |evt| {
                                            if let Ok(v) = evt.value().parse::<f64>() {
                                                effects.write().speed_factor = v;
                                            }
                                        },
                                    }
                                    span { class: "text-xs text-gray-400", "×{effects().speed_factor:.2}" }
                                }

                                // Compressor preset
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "Compressor" }
                                    select {
                                        class: "w-full bg-neutral-700 text-gray-200 rounded px-2 py-1 text-sm",
                                        value: "{effects().compress_preset}",
                                        onchange: move |evt| {
                                            effects.write().compress_preset = evt.value();
                                        },
                                        option { value: "none", "None" }
                                        option { value: "voice", "Voice" }
                                        option { value: "gentle", "Gentle" }
                                        option { value: "limiter", "Limiter" }
                                    }
                                }

                                // Apply button
                                button {
                                    class: "w-full px-3 py-2 rounded-sm bg-emerald-600 text-white font-medium hover:opacity-90 transition mt-2",
                                    onclick: move |_| {
                                        let reader = audio_buffer.read();
                                        let Some(ref buf_arc) = *reader else { return };
                                        let eff = effects();

                                        let mut buf = AudioBuffer {
                                            samples: buf_arc.samples.clone(),
                                            sample_rate: buf_arc.sample_rate,
                                            channels: buf_arc.channels,
                                        };

                                        // Apply effects
                                        algorithms::normalize(&mut buf, eff.normalize_target);

                                        if eff.fade_in_secs > 0.0 {
                                            algorithms::fade_in(&mut buf, eff.fade_in_secs);
                                        }
                                        if eff.fade_out_secs > 0.0 {
                                            algorithms::fade_out(&mut buf, eff.fade_out_secs);
                                        }

                                        if (eff.speed_factor - 1.0).abs() > 0.01 {
                                            if let Ok(b) = algorithms::change_speed(&buf, eff.speed_factor) {
                                                buf = b;
                                            }
                                        }

                                        match eff.compress_preset.as_str() {
                                            "voice" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::voice()),
                                            "gentle" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::gentle()),
                                            "limiter" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::limiter()),
                                            _ => {}
                                        }

                                        // Update waveform and info
                                        let waveform = generate_waveform_path(&buf.samples, 1000.0, 200.0);
                                        let path_str = display_info().map(|i| i.path.clone()).unwrap_or_default();
                                        display_info.set(Some(AudioDisplayInfo {
                                            path: path_str,
                                            sample_rate: buf.sample_rate,
                                            channels: buf.channels,
                                            duration: buf.duration_secs(),
                                            sample_count: buf.samples.len(),
                                            waveform_path: waveform,
                                            rms: algorithms::rms(&buf),
                                            peak: algorithms::peak(&buf),
                                            peak_dbfs: algorithms::peak_dbfs(&buf),
                                        }));

                                        drop(reader);
                                        audio_buffer.set(Some(Arc::new(buf)));
                                        status_msg.set("Effects applied".to_string());
                                    },
                                    "Apply Effects"
                                }
                            }
                        }

                        // Export Panel
                        if show_export() {
                            div {
                                h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3", "Export" }

                                // Bitrate (for MP3)
                                div { class: "mb-3",
                                    label { class: "text-xs text-gray-500 block mb-1", "MP3 Bitrate (kbps)" }
                                    select {
                                        class: "w-full bg-neutral-700 text-gray-200 rounded px-2 py-1 text-sm",
                                        value: "{export_bitrate}",
                                        onchange: move |evt| {
                                            if let Ok(v) = evt.value().parse::<u32>() {
                                                export_bitrate.set(v);
                                            }
                                        },
                                        option { value: "128", "128" }
                                        option { value: "192", selected: true, "192" }
                                        option { value: "256", "256" }
                                        option { value: "320", "320" }
                                    }
                                }

                                // Export WAV
                                button {
                                    class: "w-full px-3 py-2 rounded-sm bg-blue-600 text-white font-medium hover:opacity-90 transition mb-2",
                                    onclick: move |_| {
                                        async move {
                                            let reader = audio_buffer.read();
                                            let Some(ref buf_arc) = *reader else {
                                                status_msg.set("No audio loaded".to_string());
                                                return;
                                            };
                                            let buf = buf_arc.clone();
                                            drop(reader);

                                            let file_handle = rfd::AsyncFileDialog::new()
                                                .add_filter("WAV", &["wav"])
                                                .set_file_name("output.wav")
                                                .save_file()
                                                .await;

                                            let Some(path) = file_handle else { return };
                                            let path_buf = path.path().to_path_buf();

                                            status_msg.set("Exporting WAV...".to_string());
                                            let result = tokio::task::spawn_blocking(move || {
                                                phonon_wav::write_wav(&path_buf, &buf)
                                            }).await;

                                            match result {
                                                Ok(Ok(())) => status_msg.set("WAV exported successfully".to_string()),
                                                Ok(Err(e)) => status_msg.set(format!("Export error: {}", e)),
                                                Err(e) => status_msg.set(format!("Task error: {}", e)),
                                            }
                                        }
                                    },
                                    "Export as WAV"
                                }

                                // Export MP3
                                button {
                                    class: "w-full px-3 py-2 rounded-sm bg-purple-600 text-white font-medium hover:opacity-90 transition",
                                    onclick: move |_| {
                                        async move {
                                            let reader = audio_buffer.read();
                                            let Some(ref buf_arc) = *reader else {
                                                status_msg.set("No audio loaded".to_string());
                                                return;
                                            };
                                            let buf = buf_arc.clone();
                                            let bitrate = export_bitrate();
                                            drop(reader);

                                            let file_handle = rfd::AsyncFileDialog::new()
                                                .add_filter("MP3", &["mp3"])
                                                .set_file_name("output.mp3")
                                                .save_file()
                                                .await;

                                            let Some(path) = file_handle else { return };
                                            let path_buf = path.path().to_path_buf();

                                            status_msg.set("Exporting MP3...".to_string());
                                            let result = tokio::task::spawn_blocking(move || {
                                                let config = phonon_io::Mp3Config { bitrate_kbps: bitrate };
                                                phonon_io::write_mp3(&path_buf, &buf, &config)
                                            }).await;

                                            match result {
                                                Ok(Ok(())) => status_msg.set("MP3 exported successfully".to_string()),
                                                Ok(Err(e)) => status_msg.set(format!("Export error: {}", e)),
                                                Err(e) => status_msg.set(format!("Task error: {}", e)),
                                            }
                                        }
                                    },
                                    "Export as MP3"
                                }
                            }
                        }
                    }
                }
            }

            // --- Footer / Transport ---
            footer {
                class: "bg-neutral-800 p-4 border-t border-neutral-700 flex items-center justify-between",

                div {
                    class: "flex gap-4",
                    button {
                        class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white text-xl",
                        title: "Play",
                        onclick: move |_| {
                            // Init player if needed
                            if player_state.read().is_none() {
                                match AudioPlayer::new() {
                                    Ok(p) => player_state.set(Some(p)),
                                    Err(e) => {
                                        status_msg.set(format!("Audio Init Failed: {}", e));
                                        return;
                                    }
                                }
                            }

                            let reader = audio_buffer.read();
                            let Some(ref buf_arc) = *reader else { return };
                            let player_reader = player_state.read();
                            let Some(ref player) = *player_reader else { return };

                            // If paused, just resume
                            if player.playback_state() == phonon_engine::PlaybackState::Paused {
                                player.resume();
                                status_msg.set("Resumed".to_string());
                                return;
                            }

                            // Clone via Arc (cheap ref-count bump) then build a mutable buffer for resampling
                            let mut buffer = AudioBuffer {
                                samples: buf_arc.samples.clone(),
                                sample_rate: buf_arc.sample_rate,
                                channels: buf_arc.channels,
                            };

                            // Resample logic
                            let device_rate = player.device_sample_rate();
                            if buffer.sample_rate != device_rate {
                                status_msg.set(format!("Resampling {}->{}Hz...", buffer.sample_rate, device_rate));
                                if let Ok(b) = algorithms::resample(&buffer, device_rate) { buffer = b; }
                            }

                            // Channel conversion logic
                            let device_channels = player.device_channels();
                            if buffer.channels != device_channels {
                                status_msg.set(format!("Converting Channels {}->{}...", buffer.channels, device_channels));
                                if let Ok(b) = algorithms::ensure_channels(&buffer, device_channels) { buffer = b; }
                            }

                            match player.play(&buffer) {
                                Ok(_) => status_msg.set("Playing...".to_string()),
                                Err(e) => status_msg.set(format!("Playback Error: {}", e)),
                            }
                        },
                        "▶"
                    }
                    button {
                        class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-yellow-600 text-white text-xl",
                        title: "Pause",
                        onclick: move |_| {
                            if let Some(player) = player_state.read().as_ref() {
                                player.pause();
                                status_msg.set("Paused".to_string());
                            }
                        },
                        "⏸"
                    }
                    button {
                        class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-red-600 text-white text-xl",
                        title: "Stop",
                        onclick: move |_| {
                            if let Some(player) = player_state.read().as_ref() {
                                player.stop();
                                status_msg.set("Stopped".to_string());
                            }
                        },
                        "⏹"
                    }
                }

                div {
                    class: "text-sm text-gray-500",
                    "{status_msg}"
                }
            }
        }
    }
}
