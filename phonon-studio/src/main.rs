#![allow(non_snake_case)]

mod components;
mod state;

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use dioxus_logger::tracing;

use components::{
  effects_panel::EffectsPanel, eq_panel::EqPanel, export_panel::ExportPanel, header::Header,
  info_panel::InfoPanel, plugin_browser::PluginBrowser, transport::Transport,
  waveform::WaveformView,
};
use state::{AppState, SidePanel, load_audio};

/// Tailwind CSS asset for desktop and web
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
  if let Err(e) = dioxus_logger::init(tracing::Level::INFO) {
    eprintln!("Failed to init logger: {}", e);
  }
  launch(App);
}

fn App() -> Element {
  // Provide global state via context
  let state = use_hook(AppState::init);
  use_context_provider(|| state);

  rsx! {
      document::Stylesheet { href: TAILWIND_CSS }
      div {
          class: "flex flex-col h-dvh bg-neutral-900 text-gray-200 font-sans overflow-hidden",

          // Drag-and-drop overlay
          ondragover: move |evt| { evt.prevent_default(); },
          ondrop: move |evt| {
              evt.prevent_default();
              let mut s = state;
              async move {
                  handle_drop(&mut s, evt).await;
              }
          },

          // --- Header ---
          Header {}

          // --- Main workspace ---
          div {
              class: "flex-1 flex overflow-hidden",

              // Main content
              main {
                  class: "flex-1 flex flex-col items-center justify-start p-6 overflow-y-auto",

                  if (state.display_info)().is_some() {
                      WaveformView {}
                      InfoPanel {}
                  } else {
                      div {
                          class: "text-center text-gray-500 mt-16",
                          h2 { class: "text-2xl font-bold mb-2", "No Audio Loaded" }
                          p { "Click 'Open Audio File' or drag & drop an audio file to start." }
                      }
                  }
              }

              // Side panel
              if (state.side_panel)() != SidePanel::None {
                  aside {
                      class: "w-80 bg-neutral-800 border-l border-neutral-700 p-4 overflow-y-auto flex flex-col gap-4",

                      match (state.side_panel)() {
                          SidePanel::Effects => rsx! { EffectsPanel {} },
                          SidePanel::Eq => rsx! { EqPanel {} },
                          SidePanel::Export => rsx! { ExportPanel {} },
                          SidePanel::Plugins => rsx! { PluginBrowser {} },
                          SidePanel::None => rsx! {},
                      }
                  }
              }
          }

          // --- Transport ---
          Transport {}
      }
  }
}

/// Handle drag-and-drop file loading.
async fn handle_drop(state: &mut AppState, evt: Event<DragData>) {
  let file_list = evt.data().files();
  let Some(first_file) = file_list.first() else {
    return;
  };
  let first_name = first_file.name();

  let path_lower = first_name.to_lowercase();
  let supported = ["wav", "mp3", "flac", "ogg", "aac", "m4a", "mp4"];
  let is_audio = supported.iter().any(|ext| path_lower.ends_with(ext));
  if !is_audio {
    state.status_msg.set("Unsupported file format".to_string());
    return;
  }

  let path_buf = std::path::PathBuf::from(&first_name);
  load_audio(state, path_buf, "File loaded (drag & drop)").await;
}
