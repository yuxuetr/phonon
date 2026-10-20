//! `phonon-mcp` — stdio entry point for the Phonon MCP server.
//!
//! Designed to be launched by an MCP client (Claude Desktop, Cursor, etc.)
//! as a child process. All log output goes to stderr so it doesn't pollute
//! the JSON-RPC channel on stdout.

use phonon_mcp::Server;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> std::io::Result<()> {
  // Logs to stderr — stdout is reserved for the JSON-RPC channel.
  let _ = fmt()
    .with_writer(std::io::stderr)
    .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
    .try_init();

  // File tools are confined to PHONON_MCP_ROOT, or the launch directory.
  let fs_root = match std::env::var_os("PHONON_MCP_ROOT") {
    Some(dir) => std::path::PathBuf::from(dir),
    None => std::env::current_dir()?,
  };
  tracing::info!(root = %fs_root.display(), "file tools sandboxed");

  let server = Server::new(fs_root);
  server.run_stdio().await
}
