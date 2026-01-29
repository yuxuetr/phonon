#![allow(non_snake_case)]

use dioxus::prelude::*;
use dioxus_logger::tracing;

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
}

fn App() -> Element {
    let mut audio_state = use_signal(|| None::<LoadedAudio>);
    let mut status_msg = use_signal(|| "Ready".to_string());

    rsx! {
        div {
            style: "width: 100%; height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; background-color: #f0f0f0; font-family: sans-serif;",
            h1 { "Phonon Studio" }
            p { "Audio Processing Workbench" }
            
            div {
                style: "margin-top: 20px; padding: 20px; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); width: 400px; text-align: center;",
                
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
                        style: "margin-top: 20px; padding-top: 20px; border-top: 1px solid #eee; text-align: left;",
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
