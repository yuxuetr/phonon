#![allow(non_snake_case)]

use dioxus::prelude::*;
use phonon_core::{AudioBuffer, algorithms};
use phonon_engine::AudioPlayer;

use crate::state::AppState;

#[component]
pub fn Transport() -> Element {
  let mut state = use_context::<AppState>();

  // Format duration for display
  let time_display = match (state.display_info)() {
    Some(info) => {
      let mins = (info.duration / 60.0) as u32;
      let secs = info.duration % 60.0;
      format!("{}:{:05.2}", mins, secs)
    }
    None => "0:00.00".to_string(),
  };

  rsx! {
      footer {
          class: "bg-neutral-800 p-4 border-t border-neutral-700 flex items-center justify-between",

          // Left: playback controls
          div {
              class: "flex gap-3 items-center",
              button {
                  class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-neutral-700 text-white text-xl",
                  title: "Play",
                  onclick: move |_| async move { play_audio(&mut state).await; },
                  "▶"
              }
              button {
                  class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-yellow-600 text-white text-xl",
                  title: "Pause",
                  onclick: move |_| {
                      if let Some(player) = state.player_state.read().as_ref() {
                          player.pause();
                          state.status_msg.set("Paused".to_string());
                      }
                  },
                  "⏸"
              }
              button {
                  class: "px-3 py-2 rounded-sm shadow-sm transition hover:opacity-90 bg-red-600 text-white text-xl",
                  title: "Stop",
                  onclick: move |_| {
                      if let Some(player) = state.player_state.read().as_ref() {
                          player.stop();
                          state.status_msg.set("Stopped".to_string());
                      }
                  },
                  "⏹"
              }
          }

          // Center: time + volume
          div {
              class: "flex items-center gap-4",

              // Time display
              span { class: "text-sm font-mono text-gray-400", "{time_display}" }

              // Volume slider
              div { class: "flex items-center gap-2",
                  span { class: "text-xs text-gray-500", "🔊" }
                  input {
                      r#type: "range",
                      min: "0",
                      max: "1.5",
                      step: "0.01",
                      value: "{(state.volume)()}",
                      class: "w-24 accent-emerald-500",
                      oninput: move |evt| {
                          if let Ok(v) = evt.value().parse::<f32>() {
                              state.volume.set(v);
                          }
                      },
                  }
                  {
                      let vol_pct = format!("{:.0}%", (state.volume)() * 100.0);
                      rsx! { span { class: "text-xs text-gray-400 w-10", "{vol_pct}" } }
                  }
              }
          }

          // Right: status
          div {
              class: "text-sm text-gray-500",
              "{(state.status_msg)()}"
          }
      }
  }
}

async fn play_audio(state: &mut AppState) {
  // Init player if needed
  if state.player_state.read().is_none() {
    match AudioPlayer::new() {
      Ok(p) => state.player_state.set(Some(p)),
      Err(e) => {
        state.status_msg.set(format!("Audio Init Failed: {}", e));
        return;
      }
    }
  }

  let Some(source) = state.audio_buffer.read().clone() else {
    return;
  };
  // Signal guards must not live across the await below.
  let device = {
    let player_reader = state.player_state.read();
    let Some(ref player) = *player_reader else {
      return;
    };
    // If paused, just resume
    if player.playback_state() == phonon_engine::PlaybackState::Paused {
      player.resume();
      None
    } else {
      Some((player.device_sample_rate(), player.device_channels()))
    }
  };
  let Some((device_rate, device_channels)) = device else {
    state.status_msg.set("Resumed".to_string());
    return;
  };

  // Volume, resampling and channel conversion run on the blocking pool.
  let vol = (state.volume)();
  state.status_msg.set(format!(
    "Preparing {}Hz/{}ch -> {}Hz/{}ch...",
    source.sample_rate, source.channels, device_rate, device_channels
  ));
  let prepared = tokio::task::spawn_blocking(move || {
    let mut buffer = AudioBuffer::clone(&source);
    if (vol - 1.0).abs() > 0.01 {
      algorithms::adjust_volume(&mut buffer, vol);
    }
    if buffer.sample_rate != device_rate {
      buffer = algorithms::resample(&buffer, device_rate)?;
    }
    if buffer.channels != device_channels {
      buffer = algorithms::ensure_channels(&buffer, device_channels)?;
    }
    Ok::<_, phonon_core::PhononError>(buffer)
  })
  .await;

  let buffer = match prepared {
    Ok(Ok(buffer)) => buffer,
    Ok(Err(e)) => {
      state.status_msg.set(format!("Playback Error: {}", e));
      return;
    }
    Err(e) => {
      state.status_msg.set(format!("Task error: {}", e));
      return;
    }
  };

  let result = match state.player_state.read().as_ref() {
    Some(player) => player.play(&buffer),
    None => return,
  };
  match result {
    Ok(_) => state.status_msg.set("Playing...".to_string()),
    Err(e) => state.status_msg.set(format!("Playback Error: {}", e)),
  }
}
