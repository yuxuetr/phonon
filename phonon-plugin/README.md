# phonon-plugin

VST3 / CLAP plugin compatibility layer for the Phonon audio engine.

## What this crate gives you

* `PluginManifest` — a host-agnostic descriptor (id, name, vendor,
  version, parameter list) derived automatically from any
  `phonon_core::DspPlugin`.
* `PluginExport` trait + `ExportedPlugin` wrapper — used by host
  adapters to fetch metadata, push parameter changes, and run the
  audio process callback without depending on a specific plugin type.
* `clap` and `vst3` modules — placeholder factory entries that wire
  Phonon's built-in plugins into the CLAP / VST3 entry-point shape.
  The actual FFI layer is gated behind cargo features.

## Status

The architectural seams are in place and unit-tested. The actual
loadable `.clap` and `.vst3` binaries are not yet emitted because the
SDK choices have explicit licensing and dependency tradeoffs:

| Format | Crate | Source | Licensing |
|--------|-------|--------|-----------|
| CLAP   | [`clap-sys`](https://crates.io/crates/clap-sys) `0.5` | crates.io | MIT/Apache-2.0 |
| VST3   | `vst3-sys`                                            | git-only  | Steinberg SDK (GPLv3 / proprietary) |

Both will be wired in behind cargo features (`--features clap`,
`--features vst3`) so the workspace stays buildable on machines that
don't have the SDKs installed.

## Build (today — Rust-only)

```bash
cargo test -p phonon-plugin           # exercises manifest + factory tests
cargo build -p phonon-plugin --release # produces a cdylib stub
```

The cdylib has no exported entry points yet — it's there so the build
target is correct once the FFI bindings drop in.

## Build (planned — CLAP `.clap` bundle)

Once the `clap` feature is wired:

```bash
cargo build -p phonon-plugin --release --features clap
# Resulting libphonon_plugin.{dylib,so,dll} is renamed to phonon_plugin.clap
# and copied into:
#   ~/Library/Audio/Plug-Ins/CLAP/        (macOS)
#   ~/.clap/                              (Linux)
#   %CommonProgramFiles%/CLAP/            (Windows)
```

## Build (planned — VST3 `.vst3` bundle)

Once the `vst3` feature is wired:

```bash
cargo build -p phonon-plugin --release --features vst3
# Bundled into a .vst3 directory and installed to:
#   ~/Library/Audio/Plug-Ins/VST3/        (macOS)
#   ~/.vst3/                              (Linux)
#   %CommonProgramFiles%/VST3/            (Windows)
```

## Path forward

The work this crate scaffolds is, in order:

1. **CLAP first** (smallest API, MIT/Apache-2.0 SDK):
   * Add `clap-sys = "0.5"` behind the `clap` feature.
   * Implement `clap_entry`, the factory, and the per-plugin vtable
     using the existing `ClapPluginEntry` + `PluginExport` plumbing.
   * Validate against `clap-validator` (or the `clap-info` CLI).
2. **VST3** when SDK distribution is sorted:
   * Add `vst3-sys` (git) behind the `vst3` feature.
   * Implement the COM-style entry points using the same
     `Vst3PluginEntry` + `PluginExport` plumbing.
   * Validate with Steinberg's `validator` tool.

Both directions reuse the same `PluginExport` trait, so the audio /
parameter logic stays in `phonon-core` and never has to be ported.
