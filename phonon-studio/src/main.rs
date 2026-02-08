#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_logger::tracing;
use phonon_core::{algorithms, AudioBuffer, PhononError, Result};
use phonon_engine::AudioPlayer;
use std::path::PathBuf;

fn main() {
    // Init logger
    if let Err(e) = dioxus_logger::init(tracing::Level::INFO) {
        eprintln!("Failed to init logger: {}", e);
    }

    launch(App);
}

#[derive(Clone, Debug, PartialEq)]
struct LoadedAudio {
    path: String,
    sample_rate: u32,
    channels: u16,
    duration: f64,
    sample_count: usize,
    samples: Vec<f32>,
}

/// Downsamples audio data for visualization
fn generate_waveform_path(samples: &[f32], channels: u16, width: f32, height: f32) -> String {
    if samples.is_empty() {
        return String::new();
    }

    let samples_per_pixel = (samples.len() as f32 / width).ceil() as usize;
    let step = samples_per_pixel.max(1);
    let mid_y = height / 2.0;
    let scale_y = height / 2.0;

    let mut path = String::new();
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

fn App() -> Element {
    let mut audio_state = use_signal(|| None::<LoadedAudio>);
    let mut status_msg = use_signal(|| "Ready".to_string());
    let mut player_state = use_signal(|| None::<AudioPlayer>);

    // Configure Tailwind via Dioxus.toml and input.css, so we don't need inline styles here.
    
    rsx! {
        div {
            class: "flex flex-col h-screen bg-neutral-900 text-gray-200 font-sans overflow-hidden",
            
            // --- Header ---
            header {
                class: "bg-neutral-800 p-4 flex items-center justify-between border-b border-neutral-700",
                div { class: "font-bold text-xl text-gray-100 flex items-center gap-2", "🎛️ Phonon Studio" }
                div {
                    class: "flex gap-2",
                    button {
                        class: "px-4 py-2 rounded shadow transition hover:opacity-90 bg-blue-600 text-white font-medium",
                        onclick: move |_| {
                            async move {
                                status_msg.set("Opening file dialog...".to_string());
                                if let Some(path) = rfd::AsyncFileDialog::new()
                                    .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"])
                                    .pick_file()
                                    .await 
                                {
                                    let path_buf = path.path().to_path_buf();
                                    status_msg.set(format!("Loading {}...", path_buf.display()));
                                    let path_str = path_buf.to_string_lossy().to_string();
                                    
                                    match phonon_io::read_audio(&path_buf) {
                                        Ok(buffer) => {
                                            audio_state.set(Some(LoadedAudio {
                                                path: path_str,
                                                sample_rate: buffer.sample_rate,
                                                channels: buffer.channels,
                                                duration: buffer.duration_secs(),
                                                sample_count: buffer.samples.len(),
                                                samples: buffer.samples,
                                            }));
                                            status_msg.set("File loaded successfully".to_string());
                                        }
                                        Err(e) => status_msg.set(format!("Error: {}", e)),
                                    }
                                } else {
                                    status_msg.set("Cancelled".to_string());
                                }
                            }
                        },
                        "Open Audio File"
                    }
                }
            }

            // --- Main Workspace ---
            main {
                class: "flex-1 flex flex-col items-center justify-center p-6 overflow-y-auto",
                
                if let Some(info) = audio_state() {
                    // Waveform View
                    div {
                        class: "w-full max-w-5xl bg-neutral-800 border border-neutral-700 rounded-lg p-5 mb-5 shadow-lg",
                        svg {
                            width: "100%",
                            height: "200",
                            view_box: "0 0 1000 200",
                            preserve_aspect_ratio: "none",
                            path {
                                d: "{generate_waveform_path(&info.samples, info.channels, 1000.0, 200.0)}",
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

            // --- Footer / Transport ---
            footer {
                class: "bg-neutral-800 p-4 border-t border-neutral-700 flex items-center justify-between",
                
                div {
                    class: "flex gap-4",
                    button {
                        class: "px-3 py-2 rounded shadow transition hover:opacity-90 bg-neutral-700 text-white text-xl",
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

                            if let Some(player) = player_state.read().as_ref() {
                                if let Some(info) = audio_state() {
                                    let mut buffer = AudioBuffer {
                                        samples: info.samples.clone(),
                                        sample_rate: info.sample_rate,
                                        channels: info.channels,
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
                                }
                            }
                        },
                        "▶"
                    }
                    button {
                        class: "px-3 py-2 rounded shadow transition hover:opacity-90 bg-red-600 text-white text-xl",
                        title: "Stop",
                        onclick: move |_| {
                            if let Some(player) = player_state.read().as_ref() {
                                player.clear();
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
