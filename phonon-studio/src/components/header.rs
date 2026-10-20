#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{AppState, SidePanel, load_audio};

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
              button {
                  class: if (state.side_panel)() == SidePanel::Plugins {
                      "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-amber-600 text-white font-medium"
                  } else {
                      "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white font-medium"
                  },
                  onclick: move |_| {
                      let current = (state.side_panel)();
                      state.side_panel.set(if current == SidePanel::Plugins { SidePanel::None } else { SidePanel::Plugins });
                  },
                  "Plugins"
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

  load_audio(state, path.path().to_path_buf(), "File loaded successfully").await;
}
