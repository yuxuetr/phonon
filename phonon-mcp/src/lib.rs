//! # phonon-mcp
//!
//! Model Context Protocol (MCP) server exposing Phonon audio operations
//! as tool calls for LLM agents (Claude Desktop, Cursor, custom MCP clients).
//!
//! This is a minimal hand-rolled JSON-RPC 2.0 implementation over stdio,
//! per the MCP spec (newline-delimited JSON, methods `initialize`,
//! `tools/list`, `tools/call`).
//!
//! ## Architecture
//!
//! - [`registry`] — in-memory `AudioHandle` registry. Tool calls
//!   reference audio buffers by UUID instead of moving raw PCM data
//!   through JSON.
//! - [`tools`] — concrete tool implementations grouped by category
//!   (basic, DSP, measure, AI, pipeline). Each tool has a JSON Schema
//!   for inputs and a handler that returns a JSON value.
//! - [`server`] — the JSON-RPC dispatch loop. Reads framed messages
//!   from an `AsyncRead`, writes responses to an `AsyncWrite`.

pub mod error;
pub mod registry;
pub mod server;
pub mod tools;

pub use error::McpError;
pub use registry::{AssetRegistry, AudioHandle};
pub use server::{Server, ServerInfo};
pub use tools::ToolRegistry;
