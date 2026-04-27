# phonon-mcp

A [Model Context Protocol](https://modelcontextprotocol.io/) server that
exposes Phonon's audio engine as tool calls for LLM agents (Claude
Desktop, Cursor, Continue, custom MCP clients).

It's a minimal hand-rolled JSON-RPC 2.0 implementation over stdio — no
SDK, no Python sidecar, just a single static binary.

## Install

```bash
cargo install --path phonon-mcp
# or, from the workspace root:
cargo install --path .
```

The resulting binary is `phonon-mcp` (lives in `~/.cargo/bin`).

## Wire it into an MCP client

### Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`
(macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "phonon": {
      "command": "/Users/you/.cargo/bin/phonon-mcp",
      "args": []
    }
  }
}
```

Restart Claude Desktop. The 14 audio tools show up in the agent's
toolbox automatically.

### Cursor / Continue / custom MCP clients

Any MCP client that can spawn a stdio child process works. The command
is `phonon-mcp` with no flags — environment variables (`RUST_LOG`,
`OPENAI_API_KEY`, etc.) control behaviour.

## Tools

Phonon ships 14 built-in tools, all working out of the box (no API
keys needed):

| Category | Tools |
|---|---|
| **Basic** (asset registry) | `audio_load`, `audio_save`, `audio_info`, `audio_list`, `audio_drop` |
| **DSP** | `audio_mix`, `audio_trim`, `audio_fade`, `audio_normalize`, `audio_normalize_lufs`, `audio_volume_db`, `audio_pan`, `audio_resample` |
| **Measure** | `audio_loudness` (ITU-R BS.1770-4 / EBU R128 — LUFS-I/S/M, LRA, dBTP) |

For the full input/output schema, error model, and per-tool examples,
see [`docs/mcp-tools.md`](../docs/mcp-tools.md).

## Wire format

Audio buffers cross the JSON-RPC boundary as `AudioHandle = UUID`
references into an in-memory registry. **PCM data never travels on the
wire** — the agent says "load this WAV and normalize it to -16 LUFS,"
and the actual samples stay on the host.

A typical chain looks like:

```
audio_load(path)            → handle_a
audio_normalize_lufs(a, …)  → handle_b
audio_save(b, path)         → { path }
audio_drop(a); audio_drop(b)
```

## Logs

Logs go to **stderr** so they don't pollute the JSON-RPC channel on
stdout. Filter with the standard `RUST_LOG` env var:

```bash
RUST_LOG=phonon_mcp=debug phonon-mcp < requests.jsonl > responses.jsonl
```

## Manual smoke test

You can drive the server by hand from a shell — every line is one
JSON-RPC message:

```bash
phonon-mcp <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"audio_load","arguments":{"path":"./song.wav"}}}
EOF
```

## Status

Pure-DSP and measurement tools (13 of 14) are production-ready and
covered by integration tests in `phonon-tests`. Two tool categories
from the v0.7 plan are intentionally **not** registered here:

* **AI tools** (`tts_synthesize`, `tts_list_voices`, `sound_generate`)
* **Pipeline tools** (`podcast_generate`, `podcast_export`)

These are deliberately deferred to downstream projects so provider
configuration (API keys, model choice, voice catalog) lives with the
caller, not in this binary. If you need them today, build a small
shim that wraps `phonon-podcast` / `phonon-ai` and registers extra
`Tool { handler, … }` entries via `ToolRegistry::register`.
