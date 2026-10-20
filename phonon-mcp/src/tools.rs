//! Tool implementations exposed via the MCP server.
//!
//! Each tool has:
//! - a `name` (e.g. `"audio_normalize"`)
//! - a JSON Schema for inputs (built once at registry construction)
//! - a `description` shown to the LLM
//! - a handler that takes JSON arguments and returns a JSON result
//!
//! Tools are grouped by category but all live in a flat `ToolRegistry`.
//! Categories — basic, dsp, measure, ai, pipeline — are documented in
//! `RoadMap.md`.
//!
//! NOTE: AI / pipeline tools are placeholders that error with a helpful
//! message until a provider is plugged in via `with_tts` etc. Pure-DSP
//! tools work out of the box.

use crate::error::McpError;
use crate::registry::{AssetRegistry, AudioHandle};
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

/// Boxed async handler signature.
pub type ToolHandler =
  Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, McpError>> + Send>> + Send + Sync>;

/// Schema + description metadata for an MCP tool.
#[derive(Clone)]
pub struct Tool {
  pub name: String,
  pub description: String,
  pub input_schema: Value,
  pub handler: ToolHandler,
}

/// Registry of tools indexed by name. Built once at server startup.
#[derive(Clone)]
pub struct ToolRegistry {
  tools: BTreeMap<String, Tool>,
  pub registry: AssetRegistry,
  /// Directory that `audio_load` / `audio_save` are confined to. Paths come
  /// from the LLM, so they are resolved against this root and rejected if
  /// they escape it (including via `..` or symlinks).
  fs_root: PathBuf,
}

impl ToolRegistry {
  /// Build the default registry (basic + DSP + measure tools) with file
  /// access confined to `fs_root`.
  pub fn new(fs_root: impl Into<PathBuf>) -> Self {
    Self::with_asset_registry(AssetRegistry::new(), fs_root)
  }

  pub fn with_asset_registry(asset: AssetRegistry, fs_root: impl Into<PathBuf>) -> Self {
    let mut r = Self {
      tools: BTreeMap::new(),
      registry: asset,
      fs_root: fs_root.into(),
    };
    register_builtins(&mut r);
    r
  }

  pub fn register(&mut self, tool: Tool) {
    self.tools.insert(tool.name.clone(), tool);
  }

  pub fn get(&self, name: &str) -> Option<&Tool> {
    self.tools.get(name)
  }

  pub fn names(&self) -> Vec<&str> {
    self.tools.keys().map(|s| s.as_str()).collect()
  }

  /// JSON Schema list shape for `tools/list` MCP responses.
  pub fn list_json(&self) -> Value {
    let arr: Vec<Value> = self
      .tools
      .values()
      .map(|t| {
        json!({
          "name": t.name,
          "description": t.description,
          "inputSchema": t.input_schema,
        })
      })
      .collect();
    Value::Array(arr)
  }

  /// Dispatch a tool call: returns the raw JSON result on success.
  pub async fn call(&self, name: &str, args: Value) -> Result<Value, McpError> {
    let tool = self
      .get(name)
      .ok_or_else(|| McpError::ToolNotFound(name.to_string()))?;
    (tool.handler)(args).await
  }
}

// ─── Tool argument structs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LoadArgs {
  path: String,
}

#[derive(Debug, Deserialize)]
struct SaveArgs {
  handle: AudioHandle,
  path: String,
  #[serde(default)]
  overwrite: bool,
}

#[derive(Debug, Deserialize)]
struct HandleOnlyArgs {
  handle: AudioHandle,
}

#[derive(Debug, Deserialize)]
struct MixArgs {
  a: AudioHandle,
  b: AudioHandle,
}

#[derive(Debug, Deserialize)]
struct TrimArgs {
  handle: AudioHandle,
  start_secs: f64,
  end_secs: f64,
}

#[derive(Debug, Deserialize)]
struct FadeArgs {
  handle: AudioHandle,
  /// "in" or "out"
  kind: String,
  duration_secs: f64,
}

#[derive(Debug, Deserialize)]
struct NormalizeArgs {
  handle: AudioHandle,
  #[serde(default = "default_norm_target")]
  target_peak: f32,
}

fn default_norm_target() -> f32 {
  0.95
}

#[derive(Debug, Deserialize)]
struct NormalizeLufsArgs {
  handle: AudioHandle,
  target_lufs: f32,
  #[serde(default = "default_dbtp_ceiling")]
  true_peak_max_dbtp: f32,
}

fn default_dbtp_ceiling() -> f32 {
  -1.0
}

#[derive(Debug, Deserialize)]
struct VolumeDbArgs {
  handle: AudioHandle,
  db: f32,
}

#[derive(Debug, Deserialize)]
struct PanArgs {
  handle: AudioHandle,
  position: f32,
}

#[derive(Debug, Deserialize)]
struct ResampleArgs {
  handle: AudioHandle,
  target_rate: u32,
}

// ─── Tool result helpers ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct HandleResult {
  handle: AudioHandle,
  duration_secs: f64,
  sample_rate: u32,
  channels: u16,
}

/// Move `buf` into the registry and describe the new handle. The metadata
/// is read before the move, so no PCM is copied.
fn store(registry: &AssetRegistry, buf: AudioBuffer) -> Result<Value, McpError> {
  let (duration_secs, sample_rate, channels) = (buf.duration_secs(), buf.sample_rate, buf.channels);
  let handle = registry.insert(buf)?;
  Ok(serde_json::to_value(HandleResult {
    handle,
    duration_secs,
    sample_rate,
    channels,
  })?)
}

// ─── Filesystem sandbox ────────────────────────────────────────────────────

fn canonical_root(root: &Path) -> Result<PathBuf, McpError> {
  root
    .canonicalize()
    .map_err(|e| McpError::Io(format!("sandbox root {}: {e}", root.display())))
}

fn ensure_within(root: &Path, resolved: PathBuf, requested: &str) -> Result<PathBuf, McpError> {
  if resolved.starts_with(root) {
    Ok(resolved)
  } else {
    Err(McpError::PathDenied(format!(
      "{requested} is outside the allowed root {}",
      root.display()
    )))
  }
}

/// Resolves a path to read: it must exist and, after following symlinks,
/// lie inside `root`. Relative paths are taken relative to `root`.
fn resolve_for_read(root: &Path, path: &str) -> Result<PathBuf, McpError> {
  let root = canonical_root(root)?;
  let resolved = root
    .join(path)
    .canonicalize()
    .map_err(|e| McpError::Io(format!("{path}: {e}")))?;
  ensure_within(&root, resolved, path)
}

/// Resolves a path to write: its parent directory must exist inside `root`,
/// and an existing target (file or symlink) is only replaced with
/// `overwrite`, and only if it also resolves inside `root`.
fn resolve_for_write(root: &Path, path: &str, overwrite: bool) -> Result<PathBuf, McpError> {
  let root = canonical_root(root)?;
  let joined = root.join(path);
  let name = joined
    .file_name()
    .ok_or_else(|| McpError::InvalidArgs(format!("{path}: not a file path")))?;
  let parent = joined
    .parent()
    .unwrap_or(&root)
    .canonicalize()
    .map_err(|e| McpError::Io(format!("{path}: {e}")))?;
  let target = ensure_within(&root, parent, path)?.join(name);

  if target.symlink_metadata().is_ok() {
    if !overwrite {
      return Err(McpError::PathDenied(format!(
        "{path} already exists; pass \"overwrite\": true to replace it"
      )));
    }
    let resolved = target
      .canonicalize()
      .map_err(|e| McpError::Io(format!("{path}: {e}")))?;
    ensure_within(&root, resolved, path)?;
  }
  Ok(target)
}

// ─── Built-in tool registration ────────────────────────────────────────────

fn register_builtins(r: &mut ToolRegistry) {
  // basic
  register_audio_load(r);
  register_audio_save(r);
  register_audio_info(r);
  register_audio_list(r);
  register_audio_drop(r);

  // DSP
  register_audio_mix(r);
  register_audio_trim(r);
  register_audio_fade(r);
  register_audio_normalize(r);
  register_audio_normalize_lufs(r);
  register_audio_volume_db(r);
  register_audio_pan(r);
  register_audio_resample(r);

  // measure
  register_audio_loudness(r);
}

fn register_audio_load(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let LoadArgs { path } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let path = resolve_for_read(&fs_root, &path)?;
      let buf = phonon_io::read_audio(&path).map_err(McpError::from)?;
      store(&registry, buf)
    })
  });
  r.register(Tool {
    name: "audio_load".into(),
    description: "Load an audio file (WAV/MP3/FLAC/OGG/AAC) and return a handle. \
                  Paths are confined to the server's sandbox root."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": { "path": { "type": "string", "description": "Path to the audio file, relative to the sandbox root." } },
      "required": ["path"],
    }),
    handler,
  });
}

fn register_audio_save(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let fs_root = r.fs_root.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    let fs_root = fs_root.clone();
    Box::pin(async move {
      let SaveArgs {
        handle,
        path,
        overwrite,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let target = resolve_for_write(&fs_root, &path, overwrite)?;
      phonon_io::write_audio(&target, &buf).map_err(McpError::from)?;
      Ok(json!({ "path": target.to_string_lossy() }))
    })
  });
  r.register(Tool {
    name: "audio_save".into(),
    description: "Save an audio handle to a file inside the sandbox root. Format inferred \
                  from extension. Existing files are only replaced with overwrite=true."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":    { "type": "string" },
        "path":      { "type": "string", "description": "Output path, relative to the sandbox root." },
        "overwrite": { "type": "boolean", "default": false }
      },
      "required": ["handle", "path"],
    }),
    handler,
  });
}

fn register_audio_info(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let HandleOnlyArgs { handle } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      Ok(json!({
        "handle": handle,
        "duration_secs": buf.duration_secs(),
        "sample_rate": buf.sample_rate,
        "channels": buf.channels,
        "samples": buf.samples.len(),
        "peak": phonon_core::peak(&buf),
        "rms": phonon_core::rms(&buf),
        "dbfs": phonon_core::dbfs(&buf),
      }))
    })
  });
  r.register(Tool {
    name: "audio_info".into(),
    description: "Inspect an audio handle: duration, sample rate, channels, peak, RMS, dBFS."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": { "handle": { "type": "string" } },
      "required": ["handle"],
    }),
    handler,
  });
}

fn register_audio_list(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |_args| {
    let registry = registry.clone();
    Box::pin(async move {
      let handles = registry.handles();
      Ok(json!({ "handles": handles, "count": handles.len() }))
    })
  });
  r.register(Tool {
    name: "audio_list".into(),
    description: "List all audio handles currently in the registry.".into(),
    input_schema: json!({ "type": "object", "properties": {} }),
    handler,
  });
}

fn register_audio_drop(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let HandleOnlyArgs { handle } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let removed = registry.drop(&handle);
      Ok(json!({ "removed": removed }))
    })
  });
  r.register(Tool {
    name: "audio_drop".into(),
    description: "Remove an audio handle from the registry, freeing memory.".into(),
    input_schema: json!({
      "type": "object",
      "properties": { "handle": { "type": "string" } },
      "required": ["handle"],
    }),
    handler,
  });
}

fn register_audio_mix(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let MixArgs { a, b } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let ba = registry.get(&a)?;
      let bb = registry.get(&b)?;
      let mixed = phonon_core::mix(&ba, &bb).map_err(McpError::from)?;
      store(&registry, mixed)
    })
  });
  r.register(Tool {
    name: "audio_mix".into(),
    description: "Mix two audio handles into a new handle (samples added, clamped to [-1,1])."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "a": { "type": "string" },
        "b": { "type": "string" }
      },
      "required": ["a", "b"],
    }),
    handler,
  });
}

fn register_audio_trim(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let TrimArgs {
        handle,
        start_secs,
        end_secs,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let trimmed = phonon_core::trim(&buf, start_secs, end_secs).map_err(McpError::from)?;
      store(&registry, trimmed)
    })
  });
  r.register(Tool {
    name: "audio_trim".into(),
    description: "Trim a buffer to [start_secs, end_secs] and return a new handle.".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":     { "type": "string" },
        "start_secs": { "type": "number" },
        "end_secs":   { "type": "number" }
      },
      "required": ["handle", "start_secs", "end_secs"],
    }),
    handler,
  });
}

fn register_audio_fade(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let FadeArgs {
        handle,
        kind,
        duration_secs,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let mut owned: AudioBuffer = (*buf).clone();
      match kind.as_str() {
        "in" => phonon_core::fade_in(&mut owned, duration_secs),
        "out" => phonon_core::fade_out(&mut owned, duration_secs),
        other => {
          return Err(McpError::InvalidArgs(format!(
            "kind must be 'in' or 'out', got '{}'",
            other
          )));
        }
      }
      store(&registry, owned)
    })
  });
  r.register(Tool {
    name: "audio_fade".into(),
    description: "Apply a linear fade-in or fade-out to a buffer.".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":        { "type": "string" },
        "kind":          { "type": "string", "enum": ["in", "out"] },
        "duration_secs": { "type": "number" }
      },
      "required": ["handle", "kind", "duration_secs"],
    }),
    handler,
  });
}

fn register_audio_normalize(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let NormalizeArgs {
        handle,
        target_peak,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let mut owned: AudioBuffer = (*buf).clone();
      phonon_core::normalize(&mut owned, target_peak);
      store(&registry, owned)
    })
  });
  r.register(Tool {
    name: "audio_normalize".into(),
    description: "Peak-normalize a buffer to the given target amplitude (0.0–1.0).".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":      { "type": "string" },
        "target_peak": { "type": "number", "default": 0.95 }
      },
      "required": ["handle"],
    }),
    handler,
  });
}

fn register_audio_normalize_lufs(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let NormalizeLufsArgs {
        handle,
        target_lufs,
        true_peak_max_dbtp,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let mut owned: AudioBuffer = (*buf).clone();
      phonon_core::normalize_lufs(&mut owned, target_lufs, true_peak_max_dbtp);
      store(&registry, owned)
    })
  });
  r.register(Tool {
    name: "audio_normalize_lufs".into(),
    description: "Loudness-normalize to a target LUFS-I value with a true-peak ceiling (dBTP). \
       Use -16 LUFS / -1 dBTP for podcast publishing."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":             { "type": "string" },
        "target_lufs":        { "type": "number", "description": "e.g. -16 for podcasts" },
        "true_peak_max_dbtp": { "type": "number", "default": -1.0 }
      },
      "required": ["handle", "target_lufs"],
    }),
    handler,
  });
}

fn register_audio_volume_db(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let VolumeDbArgs { handle, db } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let mut owned: AudioBuffer = (*buf).clone();
      phonon_core::adjust_volume_db(&mut owned, db);
      store(&registry, owned)
    })
  });
  r.register(Tool {
    name: "audio_volume_db".into(),
    description: "Apply a gain in decibels (positive = louder, negative = quieter).".into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle": { "type": "string" },
        "db":     { "type": "number" }
      },
      "required": ["handle", "db"],
    }),
    handler,
  });
}

fn register_audio_pan(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let PanArgs { handle, position } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let new_buf = if buf.channels == 1 {
        phonon_core::pan_mono_to_stereo(&buf, position).map_err(McpError::from)?
      } else if buf.channels == 2 {
        phonon_core::pan_stereo(&buf, position).map_err(McpError::from)?
      } else {
        return Err(McpError::InvalidArgs(format!(
          "audio_pan requires mono or stereo input, got {} channels",
          buf.channels
        )));
      };
      store(&registry, new_buf)
    })
  });
  r.register(Tool {
    name: "audio_pan".into(),
    description:
      "Constant-power stereo panning. Position -1.0 = full left, 0.0 = center, +1.0 = full right. \
       Mono input is upmixed to stereo; stereo input is collapsed to mid then re-panned."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":   { "type": "string" },
        "position": { "type": "number", "minimum": -1.0, "maximum": 1.0 }
      },
      "required": ["handle", "position"],
    }),
    handler,
  });
}

fn register_audio_resample(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let ResampleArgs {
        handle,
        target_rate,
      } = serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let resampled = phonon_core::resample(&buf, target_rate).map_err(McpError::from)?;
      store(&registry, resampled)
    })
  });
  r.register(Tool {
    name: "audio_resample".into(),
    description: "Resample to a target sample rate using windowed-sinc (Lanczos) interpolation."
      .into(),
    input_schema: json!({
      "type": "object",
      "properties": {
        "handle":      { "type": "string" },
        "target_rate": { "type": "integer" }
      },
      "required": ["handle", "target_rate"],
    }),
    handler,
  });
}

fn register_audio_loudness(r: &mut ToolRegistry) {
  let registry = r.registry.clone();
  let handler: ToolHandler = Arc::new(move |args| {
    let registry = registry.clone();
    Box::pin(async move {
      let HandleOnlyArgs { handle } =
        serde_json::from_value(args).map_err(|e| McpError::InvalidArgs(e.to_string()))?;
      let buf = registry.get(&handle)?;
      let report = phonon_core::measure_loudness(&buf);
      Ok(json!({
        "integrated_lufs":     finite_or_null(report.integrated_lufs),
        "momentary_max_lufs":  finite_or_null(report.momentary_max_lufs),
        "short_term_max_lufs": finite_or_null(report.short_term_max_lufs),
        "range_lu":            report.range_lu,
        "true_peak_dbtp":      finite_or_null(report.true_peak_dbtp),
        "peak":                phonon_core::peak(&buf),
        "rms":                 phonon_core::rms(&buf),
        "dbfs":                finite_or_null(phonon_core::dbfs(&buf)),
      }))
    })
  });
  r.register(Tool {
    name: "audio_loudness".into(),
    description:
      "Measure loudness per ITU-R BS.1770-4 / EBU R128 (integrated, short-term, momentary, \
       LRA, true peak), plus simple peak / RMS / dBFS."
        .into(),
    input_schema: json!({
      "type": "object",
      "properties": { "handle": { "type": "string" } },
      "required": ["handle"],
    }),
    handler,
  });
}

fn finite_or_null(v: f32) -> Value {
  if v.is_finite() { json!(v) } else { Value::Null }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A sandbox root plus a sibling directory outside of it.
  struct Dirs {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    outside: std::path::PathBuf,
  }

  fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    Dirs {
      _tmp: tmp,
      root,
      outside,
    }
  }

  fn write_dummy_wav(path: &std::path::Path) {
    phonon_io::write_audio(path, &dummy()).unwrap();
  }

  #[tokio::test]
  async fn test_load_rejects_paths_outside_root() {
    let d = dirs();
    let secret = d.outside.join("secret.wav");
    write_dummy_wav(&secret);
    let r = ToolRegistry::new(&d.root);

    for path in [
      secret.to_string_lossy().into_owned(),
      "../outside/secret.wav".to_string(),
    ] {
      let err = r.call("audio_load", json!({ "path": path })).await;
      assert!(
        matches!(err, Err(McpError::PathDenied(_))),
        "{path}: {err:?}"
      );
    }
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn test_load_rejects_symlink_escaping_root() {
    let d = dirs();
    let secret = d.outside.join("secret.wav");
    write_dummy_wav(&secret);
    std::os::unix::fs::symlink(&secret, d.root.join("link.wav")).unwrap();
    let r = ToolRegistry::new(&d.root);
    let err = r.call("audio_load", json!({ "path": "link.wav" })).await;
    assert!(matches!(err, Err(McpError::PathDenied(_))), "{err:?}");
  }

  #[tokio::test]
  async fn test_load_and_save_inside_root() {
    let d = dirs();
    write_dummy_wav(&d.root.join("in.wav"));
    let r = ToolRegistry::new(&d.root);
    let loaded = r
      .call("audio_load", json!({ "path": "in.wav" }))
      .await
      .unwrap();
    let h = loaded["handle"].clone();
    r.call("audio_save", json!({ "handle": h, "path": "out.wav" }))
      .await
      .unwrap();
    assert!(d.root.join("out.wav").exists());
  }

  #[tokio::test]
  async fn test_save_rejects_paths_outside_root() {
    let d = dirs();
    let r = ToolRegistry::new(&d.root);
    let h = r.registry.insert(dummy()).unwrap();
    for path in [
      d.outside.join("x.wav").to_string_lossy().into_owned(),
      "../outside/x.wav".to_string(),
    ] {
      let err = r
        .call("audio_save", json!({ "handle": h, "path": path }))
        .await;
      assert!(
        matches!(err, Err(McpError::PathDenied(_))),
        "{path}: {err:?}"
      );
    }
    assert!(!d.outside.join("x.wav").exists());
  }

  #[tokio::test]
  async fn test_save_requires_overwrite_flag_for_existing_file() {
    let d = dirs();
    write_dummy_wav(&d.root.join("existing.wav"));
    let r = ToolRegistry::new(&d.root);
    let h = r.registry.insert(dummy()).unwrap();
    let err = r
      .call("audio_save", json!({ "handle": h, "path": "existing.wav" }))
      .await;
    assert!(matches!(err, Err(McpError::PathDenied(_))), "{err:?}");
    r.call(
      "audio_save",
      json!({ "handle": h, "path": "existing.wav", "overwrite": true }),
    )
    .await
    .unwrap();
  }

  fn dummy() -> AudioBuffer {
    AudioBuffer {
      samples: vec![0.5; 48000],
      sample_rate: 48000,
      channels: 1,
    }
  }

  #[tokio::test]
  async fn test_audio_info_returns_metadata() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let h = r.registry.insert(dummy()).unwrap();
    let out = r.call("audio_info", json!({ "handle": h })).await.unwrap();
    assert_eq!(out["sample_rate"], 48000);
    assert_eq!(out["channels"], 1);
    assert!(out["duration_secs"].as_f64().unwrap() > 0.99);
  }

  #[tokio::test]
  async fn test_audio_drop_removes() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let h = r.registry.insert(dummy()).unwrap();
    let out = r.call("audio_drop", json!({ "handle": h })).await.unwrap();
    assert_eq!(out["removed"], true);
  }

  #[tokio::test]
  async fn test_audio_normalize_returns_new_handle() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let h = r.registry.insert(dummy()).unwrap();
    let out = r
      .call(
        "audio_normalize",
        json!({ "handle": h, "target_peak": 0.9 }),
      )
      .await
      .unwrap();
    let new_h: AudioHandle = serde_json::from_value(out["handle"].clone()).unwrap();
    assert_ne!(new_h, h);
  }

  #[tokio::test]
  async fn test_audio_pan_mono_to_stereo() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let h = r.registry.insert(dummy()).unwrap();
    let out = r
      .call("audio_pan", json!({ "handle": h, "position": -1.0 }))
      .await
      .unwrap();
    assert_eq!(out["channels"], 2);
  }

  #[tokio::test]
  async fn test_audio_loudness_returns_report() {
    let r = ToolRegistry::new(std::env::temp_dir());
    // ~5 s sine for proper LUFS integration.
    let n = 48000 * 5;
    let samples: Vec<f32> = (0..n)
      .map(|i| {
        let t = i as f32 / 48000.0;
        0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
      })
      .collect();
    let buf = AudioBuffer {
      samples,
      sample_rate: 48000,
      channels: 1,
    };
    let h = r.registry.insert(buf).unwrap();
    let out = r
      .call("audio_loudness", json!({ "handle": h }))
      .await
      .unwrap();
    assert!(out["integrated_lufs"].is_number(), "got {:?}", out);
    assert!(out["peak"].is_number());
  }

  #[tokio::test]
  async fn test_unknown_tool_errors() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let err = r.call("does_not_exist", json!({})).await.unwrap_err();
    assert!(matches!(err, McpError::ToolNotFound(_)));
  }

  #[test]
  fn test_list_json_includes_all_tools() {
    let r = ToolRegistry::new(std::env::temp_dir());
    let list = r.list_json();
    let arr = list.as_array().unwrap();
    let names: Vec<&str> = arr.iter().filter_map(|v| v["name"].as_str()).collect();
    for expected in [
      "audio_load",
      "audio_save",
      "audio_info",
      "audio_list",
      "audio_drop",
      "audio_mix",
      "audio_trim",
      "audio_fade",
      "audio_normalize",
      "audio_normalize_lufs",
      "audio_volume_db",
      "audio_pan",
      "audio_resample",
      "audio_loudness",
    ] {
      assert!(names.contains(&expected), "missing tool: {}", expected);
    }
  }
}
