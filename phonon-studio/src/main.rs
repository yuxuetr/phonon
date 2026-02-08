#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_logger::tracing;
use phonon_core::{algorithms, AudioBuffer, PhononError, Result};
use phonon_engine::AudioPlayer;
use std::path::PathBuf;

// --- CSS Styles ---
const STYLE: &str = r#"
    :root {
        --bg-color: #1e1e1e;
        --panel-bg: #252526;
        --header-bg: #333333;
        --text-primary: #e0e0e0;
        --text-secondary: #aaaaaa;
        --accent-color: #00ff88;
        --button-primary-bg: #007acc;
        --button-danger-bg: #d94040;
        --border-color: #3e3e42;
    }

    body {
        margin: 0;
        padding: 0;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
        background-color: var(--bg-color);
        color: var(--text-primary);
        height: 100vh;
        overflow: hidden;
    }

    .app-container {
        display: flex;
        flex-direction: column;
        height: 100vh;
    }

    /* Header */
    .header {
        background-color: var(--header-bg);
        padding: 10px 20px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        border-bottom: 1px solid var(--border-color);
    }

    .logo {
        font-weight: bold;
        font-size: 1.2rem;
        color: var(--text-primary);
        display: flex;
        align-items: center;
        gap: 10px;
    }

    .toolbar {
        display: flex;
        gap: 10px;
    }

    /* Main Workspace */
    .workspace {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 20px;
        overflow-y: auto;
    }

    .empty-state {
        text-align: center;
        color: var(--text-secondary);
    }

    .waveform-container {
        width: 100%;
        max-width: 1000px;
        background-color: var(--panel-bg);
        border: 1px solid var(--border-color);
        border-radius: 6px;
        padding: 20px;
        margin-bottom: 20px;
        box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
    }

    .info-panel {
        background-color: var(--panel-bg);
        padding: 15px;
        border-radius: 6px;
        border: 1px solid var(--border-color);
        width: 100%;
        max-width: 1000px;
        margin-top: 20px;
    }

    .info-grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
        gap: 15px;
    }

    .info-item label {
        display: block;
        font-size: 0.8rem;
        color: var(--text-secondary);
        margin-bottom: 4px;
    }

    .info-item span {
        font-weight: 500;
    }

    /* Footer / Transport */
    .footer {
        background-color: var(--panel-bg);
        padding: 10px 20px;
        border-top: 1px solid var(--border-color);
        display: flex;
        align-items: center;
        justify-content: space-between;
    }

    .transport-controls {
        display: flex;
        gap: 15px;
    }

    .status-bar {
        font-size: 0.9rem;
        color: var(--text-secondary);
    }

    /* Buttons */
    .btn {
        padding: 8px 16px;
        border: none;
        border-radius: 4px;
        cursor: pointer;
        font-weight: 500;
        transition: background-color 0.2s;
        color: white;
        background-color: #444;
    }

    .btn:hover {
        opacity: 0.9;
    }

    .btn-primary {
        background-color: var(--button-primary-bg);
    }

    .btn-danger {
        background-color: var(--button-danger-bg);
    }
    
    .btn-icon {
        padding: 8px 12px;
        font-size: 1.2rem;
    }
"#;

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

    rsx! {
        style { "{STYLE}" }
        div {
            class: "app-container",
            
            // --- Header ---
            header {
                class: "header",
                div { class: "logo", "🎛️ Phonon Studio" }
                div {
                    class: "toolbar",
                    button {
                        class: "btn btn-primary",
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
                class: "workspace",
                
                if let Some(info) = audio_state() {
                    // Waveform View
                    div {
                        class: "waveform-container",
                        svg {
                            width: "100%",
                            height: "200",
                            view_box: "0 0 1000 200",
                            preserve_aspect_ratio: "none",
                            path {
                                d: "{generate_waveform_path(&info.samples, info.channels, 1000.0, 200.0)}",
                                stroke: "var(--accent-color)",
                                fill: "var(--accent-color)",
                                stroke_width: "1",
                                vector_effect: "non-scaling-stroke"
                            }
                        }
                    }

                    // Info Panel
                    div {
                        class: "info-panel",
                        div { class: "info-grid",
                            div { class: "info-item", label { "File Path" } span { "{info.path}" } }
                            div { class: "info-item", label { "Sample Rate" } span { "{info.sample_rate} Hz" } }
                            div { class: "info-item", label { "Channels" } span { "{info.channels}" } }
                            div { class: "info-item", label { "Duration" } span { "{info.duration:.2}s" } }
                            div { class: "info-item", label { "Samples" } span { "{info.sample_count}" } }
                        }
                    }
                } else {
                    // Empty State
                    div {
                        class: "empty-state",
                        h2 { "No Audio Loaded" }
                        p { "Click 'Open Audio File' to start editing." }
                    }
                }
            }

            // --- Footer / Transport ---
            footer {
                class: "footer",
                
                div {
                    class: "transport-controls",
                    button {
                        class: "btn btn-icon",
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
                        class: "btn btn-icon btn-danger",
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
                    class: "status-bar",
                    "{status_msg}"
                }
            }
        }
    }
}
