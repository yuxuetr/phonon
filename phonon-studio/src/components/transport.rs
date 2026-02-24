#![allow(non_snake_case)]

use dioxus::prelude::*;
use phonon_core::{algorithms, AudioBuffer};
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
                    onclick: move |_| { play_audio(&mut state); },
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

fn play_audio(state: &mut AppState) {
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

    let buf_reader = state.audio_buffer.read();
    let Some(ref buf_arc) = *buf_reader else { return };
    let player_reader = state.player_state.read();
    let Some(ref player) = *player_reader else { return };

    // If paused, just resume
    if player.playback_state() == phonon_engine::PlaybackState::Paused {
        player.resume();
        state.status_msg.set("Resumed".to_string());
        return;
    }

    let mut buffer = AudioBuffer {
        samples: buf_arc.samples.clone(),
        sample_rate: buf_arc.sample_rate,
        channels: buf_arc.channels,
    };

    // Apply volume
    let vol = (state.volume)();
    if (vol - 1.0).abs() > 0.01 {
        algorithms::adjust_volume(&mut buffer, vol);
    }

    // Resample if needed
    let device_rate = player.device_sample_rate();
    if buffer.sample_rate != device_rate {
        state.status_msg.set(format!(
            "Resampling {}->{}Hz...",
            buffer.sample_rate, device_rate
        ));
        if let Ok(b) = algorithms::resample(&buffer, device_rate) {
            buffer = b;
        }
    }

    // Channel conversion
    let device_channels = player.device_channels();
    if buffer.channels != device_channels {
        state.status_msg.set(format!(
            "Converting Channels {}->{}...",
            buffer.channels, device_channels
        ));
        if let Ok(b) = algorithms::ensure_channels(&buffer, device_channels) {
            buffer = b;
        }
    }

    match player.play(&buffer) {
        Ok(_) => state.status_msg.set("Playing...".to_string()),
        Err(e) => state.status_msg.set(format!("Playback Error: {}", e)),
    }
}
