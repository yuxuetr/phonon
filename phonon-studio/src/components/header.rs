#![allow(non_snake_case)]

use dioxus::prelude::*;
use std::sync::Arc;

use crate::state::{build_display_info, AppState, SidePanel};

#[component]
pub fn Header() -> Element {
    let mut state = use_context::<AppState>();

    rsx! {
        header {
            class: "bg-neutral-800 p-4 flex items-center justify-between border-b border-neutral-700",
            div { class: "font-bold text-xl text-gray-100 flex items-center gap-2", "🎛️ Phonon Studio" }
            div {
                class: "flex gap-2",

                // Toggle buttons for side panels
                button {
                    class: if (state.side_panel)() == SidePanel::Effects {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-emerald-600 text-white font-medium"
                    } else {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium"
                    },
                    onclick: move |_| {
                        let current = (state.side_panel)();
                        state.side_panel.set(if current == SidePanel::Effects { SidePanel::None } else { SidePanel::Effects });
                    },
                    "Effects"
                }
                button {
                    class: if (state.side_panel)() == SidePanel::Eq {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-violet-600 text-white font-medium"
                    } else {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium"
                    },
                    onclick: move |_| {
                        let current = (state.side_panel)();
                        state.side_panel.set(if current == SidePanel::Eq { SidePanel::None } else { SidePanel::Eq });
                    },
                    "EQ"
                }
                button {
                    class: if (state.side_panel)() == SidePanel::Export {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-blue-600 text-white font-medium"
                    } else {
                        "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium"
                    },
                    onclick: move |_| {
                        let current = (state.side_panel)();
                        state.side_panel.set(if current == SidePanel::Export { SidePanel::None } else { SidePanel::Export });
                    },
                    "Export"
                }

                // Open file
                button {
                    class: "px-4 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-blue-600 text-white font-medium",
                    onclick: move |_| {
                        async move {
                            open_audio_file(&mut state).await;
                        }
                    },
                    "Open Audio File"
                }
            }
        }
    }
}

async fn open_audio_file(state: &mut AppState) {
    state.status_msg.set("Opening file dialog...".to_string());
    let file_handle = rfd::AsyncFileDialog::new()
        .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"])
        .pick_file()
        .await;

    let Some(path) = file_handle else {
        state.status_msg.set("Cancelled".to_string());
        return;
    };

    let path_buf = path.path().to_path_buf();
    let path_str = path_buf.to_string_lossy().to_string();
    state
        .status_msg
        .set(format!("Loading {}...", path_buf.display()));

    let load_result = tokio::task::spawn_blocking(move || phonon_io::read_audio(&path_buf)).await;

    let buffer = match load_result {
        Ok(Ok(buf)) => buf,
        Ok(Err(e)) => {
            state.status_msg.set(format!("Error: {}", e));
            return;
        }
        Err(e) => {
            state.status_msg.set(format!("Task error: {}", e));
            return;
        }
    };

    state
        .status_msg
        .set("Generating waveform...".to_string());

    let info = build_display_info(&buffer, &path_str);
    state.display_info.set(Some(info));
    state.audio_buffer.set(Some(Arc::new(buffer)));
    state
        .status_msg
        .set("File loaded successfully".to_string());
}
