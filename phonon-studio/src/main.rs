#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_logger::tracing;
use phonon_core::{AudioBuffer, PhononError, Result};
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

        // Find min and max in this chunk to draw a vertical line or envelope
        // For simplicity in this version, we'll just take the max absolute value (envelope)
        // and draw a line to it and back to negative.
        // Actually, let's draw a single line from -peak to +peak for each pixel x.
        // But SVG path "L" connects points.
        // A common simple waveform viz is just connecting the peaks.
        
        let mut max_val = 0.0f32;
        for i in start..end {
             if let Some(s) = samples.get(i) {
                 let abs = s.abs();
                 if abs > max_val {
                     max_val = abs;
                 }
             }
        }

        // We can draw a line from mid_y - (max * scale) to mid_y + (max * scale) at x?
        // SVG paths are usually continuous.
        // Let's draw the upper envelope first, then we could close it.
        // For a simple "line" look:
        let y = mid_y - (max_val * scale_y);
        path.push_str(&format!(" L {} {:.2}", x, y));
    }
    
    // Mirror for the bottom half to make it look like a waveform
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

    path.push_str(" Z"); // Close path
    path
}

fn App() -> Element {
    let mut audio_state = use_signal(|| None::<LoadedAudio>);
    let mut status_msg = use_signal(|| "Ready".to_string());
    let mut player_state = use_signal(|| None::<AudioPlayer>);

    rsx! {
        div {
            style: "width: 100%; height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; background-color: #f0f0f0; font-family: sans-serif;",
            h1 { "Phonon Studio" }
            p { "Audio Processing Workbench" }
            
            div {
                style: "margin-top: 20px; padding: 20px; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); width: 800px; text-align: center;",
                
                button {
                    style: "padding: 10px 20px; background-color: #007bff; color: white; border: none; border-radius: 4px; cursor: pointer; font-size: 16px;",
                    onclick: move |_| {
                        async move {
                            status_msg.set("Opening file dialog...".to_string());
                            if let Some(path) = rfd::AsyncFileDialog::new()
                                .add_filter("WAV Audio", &["wav"])
                                .pick_file()
                                .await 
                            {
                                let path_buf = path.path().to_path_buf();
                                status_msg.set(format!("Loading {}...", path_buf.display()));
                                
                                let path_str = path_buf.to_string_lossy().to_string();
                                
                                match phonon_wav::read_wav(&path_buf) {
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
                                    Err(e) => {
                                        status_msg.set(format!("Error loading file: {}", e));
                                    }
                                }
                            } else {
                                status_msg.set("Cancelled".to_string());
                            }
                        }
                    },
                    "Open WAV File"
                }

                div {
                    style: "margin-top: 20px; text-align: left;",
                    p { "Status: {status_msg}" }
                }

                if let Some(info) = audio_state() {
                    div {
                        style: "margin-top: 20px; width: 100%;",
                        
                        // Waveform Visualization
                        div {
                            style: "background: #333; border-radius: 4px; padding: 10px; margin-bottom: 20px;",
                            svg {
                                width: "600",
                                height: "150",
                                view_box: "0 0 600 150",
                                path {
                                    d: "{generate_waveform_path(&info.samples, info.channels, 600.0, 150.0)}",
                                    stroke: "#00ff88",
                                    fill: "#00ff88",
                                    stroke_width: "1"
                                }
                            }
                        }

                        // Playback Controls
                        div {
                            style: "display: flex; gap: 10px; margin-bottom: 20px; justify-content: center;",
                            button {
                                style: "padding: 8px 16px; background-color: #28a745; color: white; border: none; border-radius: 4px; cursor: pointer;",
                                onclick: move |_| {
                                    // Check if player exists, if not create it
                                    if player_state.read().is_none() {
                                        match AudioPlayer::new() {
                                            Ok(p) => player_state.set(Some(p)),
                                            Err(e) => {
                                                status_msg.set(format!("Failed to init audio: {}", e));
                                                return;
                                            }
                                        }
                                    }

                                    // Play audio
                                    if let Some(player) = player_state.read().as_ref() {
                                        // Reconstruct AudioBuffer from state
                                        let buffer = AudioBuffer {
                                            samples: info.samples.clone(),
                                            sample_rate: info.sample_rate,
                                            channels: info.channels,
                                        };
                                        
                                        match player.play(&buffer) {
                                            Ok(_) => status_msg.set("Playing...".to_string()),
                                            Err(e) => status_msg.set(format!("Playback error: {}", e)),
                                        }
                                    }
                                },
                                "Play"
                            }
                            
                            button {
                                style: "padding: 8px 16px; background-color: #dc3545; color: white; border: none; border-radius: 4px; cursor: pointer;",
                                onclick: move |_| {
                                    if let Some(player) = player_state.read().as_ref() {
                                        player.clear();
                                        status_msg.set("Stopped".to_string());
                                    }
                                },
                                "Stop"
                            }
                        }

                        div {
                            style: "padding-top: 20px; border-top: 1px solid #eee; text-align: left;",
                            h3 { "File Info" }
                            p { strong { "Path: " } "{info.path}" }
                            p { strong { "Sample Rate: " } "{info.sample_rate} Hz" }
                            p { strong { "Channels: " } "{info.channels}" }
                            p { strong { "Duration: " } "{info.duration:.2} seconds" }
                            p { strong { "Samples: " } "{info.sample_count}" }
                        }
                    }
                }
            }
        }
    }
}
