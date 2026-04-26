#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::AppState;

#[component]
pub fn InfoPanel() -> Element {
  let state = use_context::<AppState>();
  let info = (state.display_info)();

  let Some(info) = info else {
    return rsx! {};
  };

  rsx! {
      div {
          class: "bg-neutral-800 p-4 rounded-lg border border-neutral-700 w-full max-w-5xl mt-5",
          div {
              class: "grid grid-cols-2 md:grid-cols-4 gap-4",
              InfoItem { label: "File Path", value: info.path.clone() }
              InfoItem { label: "Sample Rate", value: format!("{} Hz", info.sample_rate) }
              InfoItem { label: "Channels", value: format!("{}", info.channels) }
              InfoItem { label: "Duration", value: format!("{:.2}s", info.duration) }
              InfoItem { label: "Samples", value: format!("{}", info.sample_count) }
              InfoItem { label: "RMS", value: format!("{:.4}", info.rms) }
              InfoItem { label: "Peak", value: format!("{:.4}", info.peak) }
              InfoItem { label: "Peak dBFS", value: format!("{:.1} dB", info.peak_dbfs) }
          }
      }
  }
}

#[component]
fn InfoItem(label: String, value: String) -> Element {
  rsx! {
      div {
          class: "flex flex-col",
          label { class: "text-xs text-gray-500 mb-1", "{label}" }
          span { class: "font-medium truncate", "{value}" }
      }
  }
}
