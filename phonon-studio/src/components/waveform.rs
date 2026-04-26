#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{AppState, VizMode};

#[component]
pub fn WaveformView() -> Element {
  let state = use_context::<AppState>();
  let info = (state.display_info)();

  let Some(info) = info else {
    return rsx! {};
  };

  rsx! {
      div {
          class: "w-full max-w-5xl bg-neutral-800 border border-neutral-700 rounded-lg p-5 mb-5 shadow-lg",

          // Viz mode toggle tabs
          div {
              class: "flex gap-2 mb-3",
              button {
                  class: if (state.viz_mode)() == VizMode::Waveform {
                      "px-3 py-1 rounded text-xs font-medium bg-emerald-600 text-white"
                  } else {
                      "px-3 py-1 rounded text-xs font-medium bg-neutral-700 text-gray-400 hover:text-white"
                  },
                  onclick: {
                      let mut viz = state.viz_mode;
                      move |_| viz.set(VizMode::Waveform)
                  },
                  "Waveform"
              }
              button {
                  class: if (state.viz_mode)() == VizMode::Spectrum {
                      "px-3 py-1 rounded text-xs font-medium bg-violet-600 text-white"
                  } else {
                      "px-3 py-1 rounded text-xs font-medium bg-neutral-700 text-gray-400 hover:text-white"
                  },
                  onclick: {
                      let mut viz = state.viz_mode;
                      move |_| viz.set(VizMode::Spectrum)
                  },
                  "Spectrum"
              }
          }

          match (state.viz_mode)() {
              VizMode::Waveform => rsx! {
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
              },
              VizMode::Spectrum => rsx! {
                  super::spectrum::SpectrumView { bars: info.spectrum_bars.clone() }
              },
          }
      }
  }
}
