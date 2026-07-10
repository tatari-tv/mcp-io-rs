//! `mcp-io` - shared scaffolding for local stdio MCP servers across the Tatari CLI fleet.
//!
//! Phase 0 is a throwaway spike (`examples/spike.rs`) that de-risks the one
//! foundational seam the whole library rests on: a generic
//! `serve<H: rmcp::ServerHandler>(handler)` that compiles and handshakes over
//! stdio on rmcp 2.1.0. No product code lands until Phase 1.
