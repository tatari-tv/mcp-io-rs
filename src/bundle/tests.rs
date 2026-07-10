#![allow(clippy::unwrap_used)]

use rmcp::model::{CallToolResult, ContentBlock, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

// Deliberately NOT `use super::*`: bundle.rs's `crate::Result` alias would
// shadow `std::result::Result` inside the `#[tool_router]`/`#[tool_handler]`
// macro-expanded code below (both textually land in this module). Import only
// what the tests need instead.
use super::{advertised_tools, bundle};
use crate::McpIo;

/// A fake handler advertising two known tools, standing in for slack's real
/// `#[tool_router]` handler until Phase 4 (slack-cli integration, a separate
/// repo) lands. Per the Phase 5 notes' Deviations bucket: bundle is built and
/// tested here against this fake, not slack's real handler, because Phase 4
/// is a cross-repo follow-on gated on this crate's tag existing first. The
/// generic `bundle<H>`/`advertised_tools<H>` seam works identically for any
/// `ServerHandler`, slack's included.
#[derive(Clone)]
struct FakeHandler;

#[tool_router]
impl FakeHandler {
    #[tool(description = "Search for a widget.")]
    async fn search_widgets(&self) -> std::result::Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text("[]")]))
    }

    #[tool(description = "Post a widget.")]
    async fn post_widget(&self) -> std::result::Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text("ok")]))
    }
}

#[tool_handler]
impl ServerHandler for FakeHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default()
    }
}

fn make_io() -> McpIo {
    McpIo::new("fake", "1.2.3", None)
}

/// The mcpb v0.3 JSON schema, fetched from `modelcontextprotocol/mcpb`
/// (`schemas/mcpb-manifest-v0.3.schema.json`) 2026-07-09 and checked in as a
/// fixture -- a real, authoritative schema, not a hand-asserted field list.
fn mcpb_schema() -> serde_json::Value {
    let raw = include_str!("../../tests/fixtures/mcpb-manifest-v0.3.schema.json");
    serde_json::from_str(raw).unwrap()
}

#[tokio::test]
async fn test_advertised_tools_lists_handler_tools() {
    let tools = advertised_tools(FakeHandler).await.unwrap();
    let mut names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(names, vec!["post_widget", "search_widgets"]);
}

/// Success criteria (Phase 5, from the doc, mcp-io-side version): `mcp bundle`
/// produces a `.mcpb` that validates against the mcpb manifest schema, and the
/// manifest lists the same tool names the (fake) handler advertises.
#[tokio::test]
async fn test_bundle_produces_valid_manifest_with_matching_tool_names() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = dir.path().join("fake.mcpb");

    let written = bundle(&make_io(), Some(out.clone()), FakeHandler).await.unwrap();
    assert_eq!(written, out);

    let file = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();

    let manifest: serde_json::Value = {
        let mut entry = zip.by_name("manifest.json").unwrap();
        serde_json::from_reader(&mut entry).unwrap()
    };

    let validator = jsonschema::validator_for(&mcpb_schema()).unwrap();
    let errors: Vec<String> = validator.iter_errors(&manifest).map(|e| e.to_string()).collect();
    assert!(
        errors.is_empty(),
        "manifest failed mcpb v0.3 schema validation: {errors:?}"
    );

    let mut tool_names: Vec<String> = manifest["tools"]
        .as_array()
        .expect("manifest.tools should be an array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool.name should be a string").to_string())
        .collect();
    tool_names.sort();
    assert_eq!(tool_names, vec!["post_widget", "search_widgets"]);

    assert!(
        zip.by_name("server/fake").is_ok(),
        "expected a bundled server/fake binary entry"
    );
}

/// The bundle's `manifest_version`/`name`/`server` fields are also asserted
/// directly (not just schema-shape), since a schema-valid manifest could
/// still carry the wrong values.
#[tokio::test]
async fn test_bundle_manifest_fields_match_mcp_io() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = dir.path().join("fake.mcpb");
    bundle(&make_io(), Some(out.clone()), FakeHandler).await.unwrap();

    let file = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let manifest: serde_json::Value = {
        let mut entry = zip.by_name("manifest.json").unwrap();
        serde_json::from_reader(&mut entry).unwrap()
    };

    assert_eq!(manifest["manifest_version"], "0.3");
    assert_eq!(manifest["name"], "fake");
    assert_eq!(manifest["version"], "1.2.3");
    assert_eq!(manifest["server"]["type"], "binary");
    assert_eq!(manifest["server"]["entry_point"], "server/fake");
    assert_eq!(manifest["server"]["mcp_config"]["command"], "server/fake");
    assert_eq!(
        manifest["server"]["mcp_config"]["args"],
        serde_json::json!(["mcp", "serve"])
    );
}
