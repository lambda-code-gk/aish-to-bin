//! StdPolicyEngine: ツール判定のテスト

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{ShellAllowlistRule, StaticToolProfileProvider, StdPolicyEngine, ToolModeRule};
use crate::domain::{PolicyChain, PolicyDecision, PolicyVerdict, ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::{PolicyEngine, ToolProfileProvider};
use common::tool::{CommandAllowRule, ToolContext};

fn allow_policy() -> StdPolicyEngine {
    let tool_rules: Vec<Arc<dyn crate::domain::ToolPolicyRule>> = vec![
        Arc::new(ShellAllowlistRule {
            shell_tool_name: "run_shell",
        }),
        Arc::new(ToolModeRule),
    ];
    let chain = PolicyChain::new(vec![], tool_rules);
    let mut profiles = HashMap::new();
    profiles.insert(
        "run_shell".to_string(),
        ToolProfile {
            tool_name: "run_shell".to_string(),
            mode: ToolMode::RequireApproval,
            capabilities: vec![ToolCapability::Exec {
                allowlist: vec![],
            }],
            notes: None,
        },
    );
    profiles.insert(
        "read_file".to_string(),
        ToolProfile {
            tool_name: "read_file".to_string(),
            mode: ToolMode::Allow,
            capabilities: vec![],
            notes: None,
        },
    );
    let provider = Arc::new(StaticToolProfileProvider::new(profiles));
    StdPolicyEngine::new(chain, provider)
}

#[test]
fn test_non_shell_tool_always_allowed() {
    let pe = allow_policy();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"path": "/tmp/file"});
    let verdict = pe.evaluate_tool_call("read_file", &args, &ctx, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.status, "allowed");
            assert!(decision.reason == "tool_mode" || decision.reason == "default_allow");
        }
        _ => panic!("expected Allow for non-shell tool"),
    }
}

#[test]
fn test_shell_not_in_allowlist_returns_require_approval() {
    let pe = allow_policy();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, false).unwrap();
    match verdict {
        PolicyVerdict::RequireApproval { prompt, decision, .. } => {
            assert_eq!(prompt, "rm -rf /");
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.subject, "run_shell");
            assert_eq!(decision.status, "warn");
            assert!(decision.reason == "shell_approval_required" || decision.reason == "approval_required");
        }
        _ => panic!("expected RequireApproval for dangerous command"),
    }
}

#[test]
fn test_shell_in_allowlist_returns_allow() {
    let pe = allow_policy();
    let ctx = ToolContext::new(None)
        .with_command_allow_rules(vec![CommandAllowRule::Prefix("ls".to_string())]);
    let args = serde_json::json!({"command": "ls -la"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.status, "allowed");
            assert_eq!(decision.reason, "shell_allowlist");
            assert_eq!(decision.details["program"].as_str(), Some("ls"));
        }
        _ => panic!("expected Allow for allowlisted command"),
    }
}

#[test]
fn test_require_approval_provides_unsafe_context() {
    let pe = allow_policy();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "echo hello"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, false).unwrap();
    match verdict {
        PolicyVerdict::RequireApproval { value, .. } => {
            assert!(value.allow_unsafe, "RequireApproval should provide allow_unsafe context");
        }
        _ => panic!("expected RequireApproval"),
    }
}

#[test]
fn test_decision_to_event_payload_roundtrip() {
    let d = PolicyDecision {
        v: 1,
        scope: "tool".to_string(),
        subject: "run_shell".to_string(),
        status: "allowed".to_string(),
        reason: "allowlist".to_string(),
        details: serde_json::json!({"program": "ls"}),
    };
    let payload = d.to_event_payload();
    assert_eq!(payload["v"], 1);
    assert_eq!(payload["scope"], "tool");
    assert_eq!(payload["subject"], "run_shell");
    assert_eq!(payload["status"], "allowed");
    assert_eq!(payload["details"]["program"], "ls");
}
