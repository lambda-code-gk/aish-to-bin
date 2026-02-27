//! StaticToolProfileProvider: 未登録ツールは RequireApproval（fail-closed）を返す

use std::collections::HashMap;

use crate::adapter::StaticToolProfileProvider;
use crate::domain::{ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::ToolProfileProvider;

#[test]
fn test_registered_tool_returns_profile() {
    let mut profiles = HashMap::new();
    profiles.insert(
        "run_shell".to_string(),
        ToolProfile {
            tool_name: "run_shell".to_string(),
            mode: ToolMode::RequireApproval,
            capabilities: vec![ToolCapability::Exec {
                allowlist: vec!["ls".to_string()],
            }],
            notes: Some("shell".to_string()),
        },
    );
    let provider = StaticToolProfileProvider::new(profiles);
    let p = provider.get("run_shell");
    assert_eq!(p.tool_name, "run_shell");
    assert_eq!(p.mode, ToolMode::RequireApproval);
    assert_eq!(p.notes.as_deref(), Some("shell"));
}

/// 未登録ツールは RequireApproval + capabilities=[] で fail-closed
#[test]
fn test_unregistered_tool_returns_require_approval_fail_closed() {
    let provider = StaticToolProfileProvider::new(HashMap::new());
    let p = provider.get("unknown_tool");
    assert_eq!(p.tool_name, "unknown_tool");
    assert_eq!(p.mode, ToolMode::RequireApproval);
    assert!(p.capabilities.is_empty());
    assert_eq!(
        p.notes.as_deref(),
        Some("default profile (require_approval)")
    );
}
