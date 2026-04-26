#![allow(non_snake_case)]

use dioxus::prelude::*;
use phonon_core::{AudioBuffer, EqBand, FilterType, algorithms};
use std::sync::Arc;

use crate::state::{AppState, build_display_info};

/// 5-band EQ frequencies and labels.
const BAND_INFO: [(f64, &str); 5] = [
  (60.0, "Sub"),
  (250.0, "Low"),
  (1000.0, "Mid"),
  (4000.0, "Hi-Mid"),
  (12000.0, "High"),
];

#[component]
pub fn EqPanel() -> Element {
  let mut state = use_context::<AppState>();

  rsx! {
      div {
          h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3", "Parametric EQ" }

          // Preset selector
          div { class: "mb-4",
              label { class: "text-xs text-gray-500 block mb-1", "Preset" }
              select {
                  class: "w-full bg-neutral-700 text-gray-200 rounded px-2 py-1 text-sm",
                  value: "{(state.effects)().eq_preset}",
                  onchange: move |evt| {
                      let preset = evt.value();
                      // Apply preset gains
                      let gains = match preset.as_str() {
                          "voice" => [0.0, 0.0, 3.0, 2.0, -3.0],
                          "bass_heavy" => [8.0, 5.0, 0.0, -1.0, -3.0],
                          "bright" => [-2.0, -1.0, 0.0, 3.0, 5.0],
                          _ => [0.0; 5], // "none" or "custom"
                      };
                      let mut eff = state.effects.write();
                      eff.eq_preset = preset;
                      eff.eq_gains = gains;
                  },
                  option { value: "none", "None (Flat)" }
                  option { value: "voice", "Voice" }
                  option { value: "bass_heavy", "Bass Heavy" }
                  option { value: "bright", "Bright" }
                  option { value: "custom", "Custom" }
              }
          }

          // 5 band sliders (vertical-style, displayed as horizontal sliders)
          div { class: "space-y-2 mb-4",
              for (idx, (freq, label)) in BAND_INFO.iter().enumerate() {
                  {
                      let gain = (state.effects)().eq_gains[idx];
                      let freq_label = format!("{label} ({freq:.0} Hz)");
                      rsx! {
                          div { class: "flex items-center gap-2",
                              span { class: "text-xs text-gray-400 w-24 shrink-0", "{freq_label}" }
                              input {
                                  r#type: "range",
                                  min: "-12",
                                  max: "12",
                                  step: "0.5",
                                  value: "{gain}",
                                  class: "flex-1 accent-violet-500",
                                  oninput: move |evt| {
                                      if let Ok(v) = evt.value().parse::<f64>() {
                                          let mut eff = state.effects.write();
                                          eff.eq_gains[idx] = v;
                                          eff.eq_preset = "custom".to_string();
                                      }
                                  },
                              }
                              span { class: "text-xs text-gray-400 w-12 text-right",
                                  "{gain:+.1} dB"
                              }
                          }
                      }
                  }
              }
          }

          // Reset
          button {
              class: "w-full px-3 py-1.5 rounded-sm bg-neutral-700 text-gray-300 text-sm hover:opacity-90 transition mb-2",
              onclick: move |_| {
                  let mut eff = state.effects.write();
                  eff.eq_gains = [0.0; 5];
                  eff.eq_preset = "none".to_string();
              },
              "Reset EQ"
          }

          // Apply
          button {
              class: "w-full px-3 py-2 rounded-sm bg-violet-600 text-white font-medium hover:opacity-90 transition",
              onclick: move |_| {
                  apply_eq(&mut state);
              },
              "Apply EQ"
          }
      }
  }
}

fn apply_eq(state: &mut AppState) {
  let reader = state.audio_buffer.read();
  let Some(ref buf_arc) = *reader else { return };
  let eff = (state.effects)();

  // Build EQ bands from the 5-band gains
  let bands: Vec<EqBand> = BAND_INFO
    .iter()
    .zip(eff.eq_gains.iter())
    .filter(|(_, gain)| gain.abs() > 0.1)
    .map(|((freq, _), gain)| {
      // Use LowShelf for sub, HighShelf for high, Peaking for mid bands
      let filter_type = if *freq <= 80.0 {
        FilterType::LowShelf
      } else if *freq >= 10000.0 {
        FilterType::HighShelf
      } else {
        FilterType::Peaking
      };
      EqBand::new(filter_type, *freq, 1.0, *gain)
    })
    .collect();

  if bands.is_empty() {
    state
      .status_msg
      .set("EQ: all bands flat, nothing to apply".to_string());
    return;
  }

  let mut buf = AudioBuffer {
    samples: buf_arc.samples.clone(),
    sample_rate: buf_arc.sample_rate,
    channels: buf_arc.channels,
  };
  drop(reader);

  algorithms::apply_eq(&mut buf, &bands);

  let path_str = (state.display_info)()
    .map(|i| i.path.clone())
    .unwrap_or_default();
  let info = build_display_info(&buf, &path_str);
  state.display_info.set(Some(info));
  state.audio_buffer.set(Some(Arc::new(buf)));
  state
    .status_msg
    .set(format!("EQ applied ({} bands)", bands.len()));
}
