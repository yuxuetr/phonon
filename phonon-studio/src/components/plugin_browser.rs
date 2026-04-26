#![allow(non_snake_case)]

use dioxus::prelude::*;

use crate::state::{ActivePluginState, AppState};

/// Plugin browser sidebar panel.
///
/// Lists available plugins from the registry and shows the active
/// plugin chain with parameter sliders.
#[component]
pub fn PluginBrowser() -> Element {
  let mut state = use_context::<AppState>();

  // Collect available plugin info from the registry.
  let registry = (state.plugin_registry)();
  let mut available: Vec<(&str, String)> = registry
    .list_ids()
    .into_iter()
    .map(|id| {
      let name = registry
        .create(id)
        .map(|p| p.name().to_string())
        .unwrap_or_else(|| id.to_string());
      (id, name)
    })
    .collect();
  available.sort_by(|a, b| a.1.cmp(&b.1));

  let active_plugins = (state.active_plugins)();

  rsx! {
    div {
      // ── Available Plugins ─────────────────────────────
      h3 {
        class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3",
        "Available Plugins"
      }

      div { class: "flex flex-col gap-1 mb-4",
        for (id, name) in available.iter() {
          div {
            class: "flex items-center justify-between bg-neutral-700 rounded px-2 py-1",
            span { class: "text-sm text-gray-200", "{name}" }
            button {
              class: "text-xs bg-emerald-600 text-white px-2 py-0.5 rounded hover:opacity-90",
              onclick: {
                let plugin_id = id.to_string();
                move |_| {
                  add_plugin(&mut state, &plugin_id);
                }
              },
              "Add"
            }
          }
        }
      }

      // ── Active Chain ──────────────────────────────────
      h3 {
        class: "text-sm font-semibold text-gray-400 uppercase tracking-wider mb-3",
        "Active Chain ({active_plugins.len()})"
      }

      if active_plugins.is_empty() {
        p { class: "text-xs text-gray-500", "No plugins in the chain." }
      } else {
        div { class: "flex flex-col gap-3 mb-3",
          for (idx, plugin) in active_plugins.iter().enumerate() {
            div {
              class: "bg-neutral-700 rounded p-2",
              div {
                class: "text-sm text-gray-200 font-medium mb-1",
                "{idx + 1}. {plugin.name}"
              }
              for (pname, val, min, max, _default) in plugin.params.iter() {
                div { class: "mb-1",
                  label { class: "text-xs text-gray-500 block", "{pname}" }
                  input {
                    r#type: "range",
                    min: "{min}",
                    max: "{max}",
                    step: "0.01",
                    value: "{val}",
                    class: "w-full accent-emerald-500",
                    oninput: {
                      let param_name = pname.clone();
                      move |evt| {
                        if let Ok(v) = evt.value().parse::<f32>() {
                          update_plugin_param(&mut state, idx, &param_name, v);
                        }
                      }
                    },
                  }
                  span { class: "text-xs text-gray-400", "{val:.2}" }
                }
              }
            }
          }
        }

        button {
          class: "w-full px-3 py-2 rounded-sm bg-red-600 text-white font-medium hover:opacity-90 transition",
          onclick: move |_| {
            clear_all_plugins(&mut state);
          },
          "Remove All"
        }
      }
    }
  }
}

/// Creates a plugin instance and adds it to the player's effect chain.
fn add_plugin(state: &mut AppState, plugin_id: &str) {
  let registry = (state.plugin_registry)();
  let Some(plugin) = registry.create(plugin_id) else {
    state.status_msg.set(format!("Unknown plugin: {plugin_id}"));
    return;
  };

  let active = ActivePluginState {
    id: plugin.id().to_string(),
    name: plugin.name().to_string(),
    params: plugin
      .parameters()
      .into_iter()
      .map(|p| (p.name.clone(), p.current, p.min, p.max, p.default))
      .collect(),
  };

  // Add to player if available.
  let reader = state.player_state.read();
  if let Some(ref player) = *reader {
    // Create a fresh instance for the player (the one above is for UI state).
    if let Some(player_plugin) = registry.create(plugin_id) {
      player.add_plugin(player_plugin);
    }
  }
  drop(reader);

  state.active_plugins.write().push(active);
  state.status_msg.set(format!("Added plugin: {plugin_id}"));
}

/// Updates a parameter on an active plugin and syncs to the player.
fn update_plugin_param(state: &mut AppState, plugin_idx: usize, param_name: &str, value: f32) {
  let mut plugins = state.active_plugins.write();
  if let Some(plugin) = plugins.get_mut(plugin_idx) {
    if let Some(param) = plugin.params.iter_mut().find(|p| p.0 == param_name) {
      param.1 = value.clamp(param.2, param.3);
    }
  }
  // Note: Real-time parameter sync to the AudioPlayer effect chain would
  // require shared mutable access to the plugin instances. For now, updating
  // the UI state; the player would need to be rebuilt with updated params
  // on the next apply or re-add cycle.
}

/// Removes all plugins from the chain.
fn clear_all_plugins(state: &mut AppState) {
  let reader = state.player_state.read();
  if let Some(ref player) = *reader {
    player.clear_effects();
  }
  drop(reader);

  state.active_plugins.write().clear();
  state.status_msg.set("Plugin chain cleared".to_string());
}
