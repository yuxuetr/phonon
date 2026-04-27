# phonon-wasm

WebAssembly bindings for the Phonon audio engine. Exposes a subset of
`phonon-core`'s pure DSP (normalize, fade, volume, speed, EQ, spectrum,
loudness metrics, …) to JavaScript, and ships with a self-contained
demo page in `www/`.

## Build

```bash
# from the repo root
./phonon-wasm/build.sh           # release build (default)
./phonon-wasm/build.sh --dev     # debug build (faster compile, larger .wasm)
```

The script invokes `wasm-pack build --target web --out-dir www/pkg`.
Output lands in `phonon-wasm/www/pkg/` (gitignored except for the
`.gitignore` placeholder).

## Run the demo

```bash
python3 -m http.server --directory phonon-wasm/www 8080
```

Open <http://localhost:8080/> and drag-and-drop a `.wav`/`.mp3`/`.flac`
file. Demo features:

* drag-and-drop file load via the Web Audio API
* normalize, reverse, fade in/out, volume, speed change
* live waveform + spectrum (`compute_spectrum` from phonon-core)
* play / stop with the browser's audio context
* download the processed result as 16-bit WAV

## Deploy to GitHub Pages

`www/` is a static site — once you've run `build.sh`, you can publish
`phonon-wasm/www/` to any static host.

A `workflow_dispatch`-only Pages workflow lives at
`.github/workflows/wasm-pages.yml`. To deploy:

1. Enable Pages on the repo (Settings → Pages → Source: GitHub Actions).
2. Trigger the workflow manually (Actions → "wasm-pages" → Run workflow).

## Available bindings

See `src/lib.rs`. Highlights:

* `process_audio(samples, sr, ch, ops_json)` — chain operations
* `normalize`, `reverse`, `fade_in`, `fade_out`
* `adjust_volume(factor)` — linear gain
* `change_speed(factor)`
* `compute_spectrum(samples, sr, ch, num_bins)` — FFT magnitudes
* `rms`, `peak`, `peak_dbfs`, `duration_secs`
