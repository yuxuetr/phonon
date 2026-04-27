//! # phonon-plugin
//!
//! VST3 / CLAP plugin compatibility layer for [`phonon_core::DspPlugin`].
//!
//! ## What this crate is for
//!
//! Phonon already has a generic plugin trait — [`phonon_core::DspPlugin`] —
//! used by `phonon-engine`'s effect chain and by `phonon-studio`. This
//! crate is the bridge between that trait and the two cross-DAW plugin
//! standards:
//!
//! * **CLAP** — the open Cross-platform Audio Plugin format
//! * **VST3** — Steinberg's plugin standard
//!
//! ## Status — what's wired today
//!
//! * [`PluginManifest`] — the host-agnostic descriptor (id, name,
//!   vendor, version, parameter list) derived from any `DspPlugin`.
//! * [`manifest_from_plugin`] — pure-Rust function that turns a
//!   `DspPlugin` into a `PluginManifest` suitable for both CLAP's
//!   `clap_plugin_descriptor_t` / `clap_param_info_t` and VST3's
//!   `PClassInfo` / `IParameterInfo`.
//! * [`PluginExport`] — trait that lets a host adapter pull descriptor
//!   metadata, an audio-process callback, and a parameter setter from
//!   any boxed `DspPlugin` without owning it directly.
//! * Built-in re-exports of phonon-core's plugins so the crate can be
//!   used as a one-stop bundle for the common case.
//!
//! ## Status — what's not yet wired
//!
//! The actual FFI bindings live behind opt-in feature flags so the
//! workspace stays buildable on machines that don't have host SDK
//! headers (Steinberg's VST3 SDK is GPLv3 / proprietary dual-licensed
//! and `vst3-sys` is a git-only dep).
//!
//! * `clap` feature — will pull `clap-sys` from crates.io and emit a
//!   `clap_entry` symbol that wraps a registered manifest.
//! * `vst3` feature — will pull `vst3-sys` (git) and emit the equivalent
//!   COM-style entry points.
//!
//! See `README.md` for the build / install flow and the path forward.

#![warn(missing_docs)]

mod manifest;

pub use manifest::{ExportedPlugin, ParamInfo, PluginExport, PluginManifest, manifest_from_plugin};

#[cfg(feature = "clap")]
pub mod clap;

#[cfg(feature = "vst3")]
pub mod vst3;

// Re-export phonon-core plugin types so consumers can grab everything
// they need from a single crate.
pub use phonon_core::{
  CompressorPlugin, DspPlugin, HighPassPlugin, LowPassPlugin, ParametricEqPlugin, PluginParam,
  VolumePlugin,
};
