#![allow(clippy::unwrap_used)]

use super::*;

fn make_io() -> McpIo {
    McpIo::new("test", "0.0.0", None)
}

#[test]
#[should_panic(expected = "claude mcp add-json")]
fn test_register_user_dispatches_to_claude_stub() {
    register(&make_io(), Target::User);
}

#[test]
#[should_panic(expected = "claude mcp add-json")]
fn test_register_project_dispatches_to_claude_stub() {
    register(&make_io(), Target::Project);
}

#[test]
#[should_panic(expected = "direct atomic write to claude_desktop_config.json")]
fn test_register_desktop_dispatches_to_desktop_stub() {
    register(&make_io(), Target::Desktop);
}

#[test]
#[should_panic(expected = "claude mcp remove")]
fn test_unregister_user_dispatches_to_claude_stub() {
    unregister(&make_io(), Target::User);
}

#[test]
#[should_panic(expected = "direct atomic write removing the entry")]
fn test_unregister_desktop_dispatches_to_desktop_stub() {
    unregister(&make_io(), Target::Desktop);
}

#[test]
#[should_panic(expected = "Phase 3: status orchestration")]
fn test_status_not_yet_implemented() {
    status(&make_io());
}
