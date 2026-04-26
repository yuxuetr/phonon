#![allow(non_snake_case)]

use dioxus::prelude::*;
use phonon_core::{AudioBuffer, algorithms};
use std::sync::Arc;

use crate::state::{AppState, build_display_info};

#[component]
pub fn EffectsPanel() -> Element {
  let mut state = use_context::<AppState>();

  rsx! {
      div {
          h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3", "Effects" }

          // Normalize
          SliderControl {
              label: "Normalize Peak",
              min: "0.1",
              max: "1.0",
              step: "0.05",
              value: format!("{}", (state.effects)().normalize_target),
              display: format!("{:.2}", (state.effects)().normalize_target),
              onchange: move |v: f32| { state.effects.write().normalize_target = v; },
          }

          // Fade In
          SliderControl {
              label: "Fade In (s)",
              min: "0",
              max: "5",
              step: "0.1",
              value: format!("{}", (state.effects)().fade_in_secs),
              display: format!("{:.1}s", (state.effects)().fade_in_secs),
              onchange: move |v: f32| { state.effects.write().fade_in_secs = v as f64; },
          }

          // Fade Out
          SliderControl {
              label: "Fade Out (s)",
              min: "0",
              max: "5",
              step: "0.1",
              value: format!("{}", (state.effects)().fade_out_secs),
              display: format!("{:.1}s", (state.effects)().fade_out_secs),
              onchange: move |v: f32| { state.effects.write().fade_out_secs = v as f64; },
          }

          // Speed
          SliderControl {
              label: "Speed",
              min: "0.25",
              max: "4.0",
              step: "0.05",
              value: format!("{}", (state.effects)().speed_factor),
              display: format!("×{:.2}", (state.effects)().speed_factor),
              onchange: move |v: f32| { state.effects.write().speed_factor = v as f64; },
          }

          // Compressor preset
          div { class: "mb-3",
              label { class: "text-xs text-gray-500 block mb-1", "Compressor" }
              select {
                  class: "w-full bg-neutral-700 text-gray-200 rounded px-2 py-1 text-sm",
                  value: "{(state.effects)().compress_preset}",
                  onchange: move |evt| {
                      state.effects.write().compress_preset = evt.value();
                  },
                  option { value: "none", "None" }
                  option { value: "voice", "Voice" }
                  option { value: "gentle", "Gentle" }
                  option { value: "limiter", "Limiter" }
              }
          }

          // Apply button
          button {
              class: "w-full px-3 py-2 rounded-sm bg-emerald-600 text-white font-medium hover:opacity-90 transition mt-2",
              onclick: move |_| {
                  apply_effects(&mut state);
              },
              "Apply Effects"
          }
      }
  }
}

fn apply_effects(state: &mut AppState) {
  let reader = state.audio_buffer.read();
  let Some(ref buf_arc) = *reader else { return };
  let eff = (state.effects)();

  let mut buf = AudioBuffer {
    samples: buf_arc.samples.clone(),
    sample_rate: buf_arc.sample_rate,
    channels: buf_arc.channels,
  };
  drop(reader);

  algorithms::normalize(&mut buf, eff.normalize_target);

  if eff.fade_in_secs > 0.0 {
    algorithms::fade_in(&mut buf, eff.fade_in_secs);
  }
  if eff.fade_out_secs > 0.0 {
    algorithms::fade_out(&mut buf, eff.fade_out_secs);
  }
  if (eff.speed_factor - 1.0).abs() > 0.01 {
    if let Ok(b) = algorithms::change_speed(&buf, eff.speed_factor) {
      buf = b;
    }
  }
  match eff.compress_preset.as_str() {
    "voice" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::voice()),
    "gentle" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::gentle()),
    "limiter" => buf = algorithms::with_compression(&buf, &algorithms::CompressorConfig::limiter()),
    _ => {}
  }

  let path_str = (state.display_info)()
    .map(|i| i.path.clone())
    .unwrap_or_default();
  let info = build_display_info(&buf, &path_str);
  state.display_info.set(Some(info));
  state.audio_buffer.set(Some(Arc::new(buf)));
  state.status_msg.set("Effects applied".to_string());
}

#[component]
fn SliderControl(
  label: String,
  min: String,
  max: String,
  step: String,
  value: String,
  display: String,
  onchange: EventHandler<f32>,
) -> Element {
  rsx! {
      div { class: "mb-3",
          label { class: "text-xs text-gray-500 block mb-1", "{label}" }
          input {
              r#type: "range",
              min: "{min}",
              max: "{max}",
              step: "{step}",
              value: "{value}",
              class: "w-full accent-emerald-500",
              oninput: move |evt| {
                  if let Ok(v) = evt.value().parse::<f32>() {
                      onchange.call(v);
                  }
              },
          }
          span { class: "text-xs text-gray-400", "{display}" }
      }
  }
}
