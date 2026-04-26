#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::AppState;

#[component]
pub fn ExportPanel() -> Element {
  let mut state = use_context::<AppState>();

  rsx! {
      div {
          h3 { class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3", "Export" }

          // MP3 Bitrate
          div { class: "mb-3",
              label { class: "text-xs text-gray-500 block mb-1", "MP3 Bitrate (kbps)" }
              select {
                  class: "w-full bg-neutral-700 text-gray-200 rounded px-2 py-1 text-sm",
                  value: "{(state.export_bitrate)()}",
                  onchange: move |evt| {
                      if let Ok(v) = evt.value().parse::<u32>() {
                          state.export_bitrate.set(v);
                      }
                  },
                  option { value: "128", "128" }
                  option { value: "192", "192" }
                  option { value: "256", "256" }
                  option { value: "320", "320" }
              }
          }

          // Format buttons
          ExportButton { format: "WAV", ext: "wav", color: "bg-blue-600" }
          ExportButton { format: "MP3", ext: "mp3", color: "bg-purple-600" }
          ExportButton { format: "FLAC", ext: "flac", color: "bg-teal-600" }
          ExportButton { format: "OGG", ext: "ogg", color: "bg-orange-600" }
          ExportButton { format: "AAC", ext: "aac", color: "bg-rose-600" }
      }
  }
}

#[component]
fn ExportButton(format: String, ext: String, color: String) -> Element {
  let mut state = use_context::<AppState>();
  let format_clone = format.clone();
  let ext_clone = ext.clone();

  rsx! {
      button {
          class: "w-full px-3 py-2 rounded-sm {color} text-white font-medium hover:opacity-90 transition mb-2",
          onclick: move |_| {
              let fmt = format_clone.clone();
              let ext = ext_clone.clone();
              async move {
                  export_audio(&mut state, &fmt, &ext).await;
              }
          },
          "Export as {format}"
      }
  }
}

async fn export_audio(state: &mut AppState, format: &str, ext: &str) {
  let reader = state.audio_buffer.read();
  let Some(ref buf_arc) = *reader else {
    state.status_msg.set("No audio loaded".to_string());
    return;
  };
  let buf = buf_arc.clone();
  let bitrate = (state.export_bitrate)();
  drop(reader);

  let file_handle = rfd::AsyncFileDialog::new()
    .add_filter(format, &[ext])
    .set_file_name(format!("output.{}", ext))
    .save_file()
    .await;

  let Some(path) = file_handle else { return };
  let path_buf = path.path().to_path_buf();

  state.status_msg.set(format!("Exporting {}...", format));

  let ext_owned = ext.to_string();
  let result = tokio::task::spawn_blocking(move || match ext_owned.as_str() {
    "wav" => phonon_wav::write_wav(&path_buf, &buf),
    "mp3" => {
      let config = phonon_io::Mp3Config {
        bitrate_kbps: bitrate,
      };
      phonon_io::write_mp3(&path_buf, &buf, &config)
    }
    "flac" => phonon_io::write_flac(&path_buf, &buf, &phonon_io::FlacConfig::default()),
    "ogg" => phonon_io::write_ogg(&path_buf, &buf, &phonon_io::OggConfig::default()),
    "aac" => phonon_io::write_aac(&path_buf, &buf, &phonon_io::AacConfig::default()),
    _ => Err(phonon_core::PhononError::InvalidParam(format!(
      "Unknown format: {}",
      ext_owned
    ))),
  })
  .await;

  match result {
    Ok(Ok(())) => state
      .status_msg
      .set(format!("{} exported successfully", format)),
    Ok(Err(e)) => state.status_msg.set(format!("Export error: {}", e)),
    Err(e) => state.status_msg.set(format!("Task error: {}", e)),
  }
}
