//! JSON-RPC 2.0 stdio server implementing the MCP wire protocol.
//!
//! MCP over stdio uses newline-delimited JSON: each line is one
//! complete JSON-RPC 2.0 message (request, response, or notification).
//!
//! Methods supported:
//! - `initialize` — handshake; returns server info + capabilities
//! - `notifications/initialized` — no-op acknowledgement
//! - `ping` — keep-alive, returns `{}`
//! - `tools/list` — returns the registered tools
//! - `tools/call` — invokes a tool
//!
//! Anything else returns a JSON-RPC method-not-found error.

use crate::tools::ToolRegistry;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

/// MCP protocol version that this server speaks. Mirrors the spec id used
/// by Claude Desktop and other MCP clients in 2025.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Server-identifying info returned by `initialize`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
  pub name: String,
  pub version: String,
}

impl Default for ServerInfo {
  fn default() -> Self {
    Self {
      name: "phonon-mcp".into(),
      version: env!("CARGO_PKG_VERSION").into(),
    }
  }
}

/// Stdio MCP server.
pub struct Server {
  pub info: ServerInfo,
  pub tools: ToolRegistry,
}

impl Server {
  pub fn new() -> Self {
    Self {
      info: ServerInfo::default(),
      tools: ToolRegistry::new(),
    }
  }

  pub fn with_tools(tools: ToolRegistry) -> Self {
    Self {
      info: ServerInfo::default(),
      tools,
    }
  }

  /// Run the request loop on the given async reader / writer until the
  /// stream closes. Each line of input is one JSON-RPC message.
  pub async fn run<R, W>(&self, reader: R, mut writer: W) -> std::io::Result<()>
  where
    R: tokio::io::AsyncRead + Unpin + Send,
    W: tokio::io::AsyncWrite + Unpin + Send,
  {
    let mut lines = BufReader::new(reader).lines();
    info!("phonon-mcp server starting");
    while let Some(line) = lines.next_line().await? {
      if line.trim().is_empty() {
        continue;
      }
      let response = self.handle_line(&line).await;
      if let Some(out) = response {
        let mut s = serde_json::to_string(&out).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        writer.write_all(s.as_bytes()).await?;
        writer.flush().await?;
      }
    }
    info!("phonon-mcp server stopping");
    Ok(())
  }

  /// Convenience entry point — runs against actual stdin/stdout.
  pub async fn run_stdio(&self) -> std::io::Result<()> {
    self.run(tokio::io::stdin(), tokio::io::stdout()).await
  }

  /// Handle a single line of JSON-RPC. Returns `None` for notifications
  /// (which by spec must not get a response) and parse errors that have
  /// no id to attach a response to.
  pub(crate) async fn handle_line(&self, line: &str) -> Option<Value> {
    let req: Value = match serde_json::from_str(line) {
      Ok(v) => v,
      Err(e) => {
        warn!(err = %e, line = line, "JSON parse error");
        // Per spec, we can't reply without an id, but emit a parse-error
        // response with id=null to surface the problem.
        return Some(json!({
          "jsonrpc": "2.0",
          "id": Value::Null,
          "error": { "code": -32700, "message": format!("Parse error: {}", e) }
        }));
      }
    };

    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    debug!(method = method, id = ?id, "incoming request");

    // Notifications have no id and never get a response.
    let is_notification = id.is_none();

    let result = self.dispatch(method, params).await;

    if is_notification {
      return None;
    }

    let id = id.unwrap_or(Value::Null);
    Some(match result {
      Ok(value) => json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": value,
      }),
      Err((code, message)) => json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
      }),
    })
  }

  async fn dispatch(&self, method: &str, params: Value) -> Result<Value, (i64, String)> {
    match method {
      "initialize" => Ok(json!({
        "protocolVersion": PROTOCOL_VERSION,
        "serverInfo": self.info,
        "capabilities": {
          "tools": { "listChanged": false }
        }
      })),
      "notifications/initialized" => Ok(Value::Null),
      "ping" => Ok(json!({})),
      "tools/list" => Ok(json!({ "tools": self.tools.list_json() })),
      "tools/call" => {
        let name = params
          .get("name")
          .and_then(|v| v.as_str())
          .ok_or_else(|| (-32602, "tools/call: missing 'name'".to_string()))?;
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        match self.tools.call(name, args).await {
          Ok(value) => Ok(json!({
            "content": [
              { "type": "text", "text": value.to_string() }
            ],
            "isError": false,
            "structuredContent": value,
          })),
          Err(e) => Ok(json!({
            "content": [
              { "type": "text", "text": e.to_string() }
            ],
            "isError": true,
          })),
        }
      }
      _ => Err((-32601, format!("Method not found: {}", method))),
    }
  }
}

impl Default for Server {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use phonon_core::AudioBuffer;
  use serde_json::json;

  fn server() -> Server {
    Server::new()
  }

  #[tokio::test]
  async fn test_initialize_returns_server_info() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(resp["result"]["serverInfo"]["name"], "phonon-mcp");
  }

  #[tokio::test]
  async fn test_notifications_have_no_response() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(s.handle_line(&req.to_string()).await.is_none());
  }

  #[tokio::test]
  async fn test_tools_list_includes_audio_info() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    let tools = resp["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"audio_info"));
  }

  #[tokio::test]
  async fn test_tools_call_audio_info_roundtrip() {
    let s = server();
    let buf = AudioBuffer {
      samples: vec![0.5; 48000],
      sample_rate: 48000,
      channels: 1,
    };
    let h = s.tools.registry.insert(buf);
    let req = json!({
      "jsonrpc": "2.0",
      "id": 3,
      "method": "tools/call",
      "params": {
        "name": "audio_info",
        "arguments": { "handle": h }
      }
    });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 3);
    assert_eq!(resp["result"]["isError"], false);
    let structured = &resp["result"]["structuredContent"];
    assert_eq!(structured["sample_rate"], 48000);
  }

  #[tokio::test]
  async fn test_tools_call_unknown_tool_returns_error_content() {
    let s = server();
    let req = json!({
      "jsonrpc": "2.0",
      "id": 4,
      "method": "tools/call",
      "params": { "name": "no_such_tool", "arguments": {} }
    });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["result"]["isError"], true);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Tool not found"));
  }

  #[tokio::test]
  async fn test_unknown_method_returns_jsonrpc_error() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 5, "method": "bogus/method" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["error"]["code"], -32601);
  }

  #[tokio::test]
  async fn test_parse_error_returns_id_null_response() {
    let s = server();
    let resp = s.handle_line("{ this is not json").await.unwrap();
    assert_eq!(resp["id"], Value::Null);
    assert_eq!(resp["error"]["code"], -32700);
  }

  #[tokio::test]
  async fn test_ping() {
    let s = server();
    let req = json!({ "jsonrpc": "2.0", "id": 6, "method": "ping" });
    let resp = s.handle_line(&req.to_string()).await.unwrap();
    assert_eq!(resp["id"], 6);
    assert_eq!(resp["result"], json!({}));
  }
}
