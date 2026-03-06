//! StdPolicyEngine: ツール判定のテスト（Phase 5: rule chain + ToolProfile）

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{
    EgressBudgetHardCapRule, EgressSensitiveRule, ShellAllowlistRule, StaticToolProfileProvider,
    StdPolicyEngine, ToolModeRule,
};
use crate::domain::{
    PolicyChain, PolicyDecision, PolicyVerdict, ToolCapability, ToolMode, ToolProfile,
};
use crate::ports::outbound::{PolicyEngine, ToolProfileProvider};
use common::tool::ToolContext;

fn engine_with_allowlist(allowlist: Vec<&str>) -> StdPolicyEngine {
    let run_shell_profile = ToolProfile {
        tool_name: "run_shell".to_string(),
        mode: ToolMode::RequireApproval,
        capabilities: vec![ToolCapability::Exec {
            allowlist: allowlist.iter().map(|s| (*s).to_string()).collect(),
        }],
        notes: None,
    };
    let mut profiles = HashMap::new();
    profiles.insert("run_shell".to_string(), run_shell_profile);
    let tool_profiles: Arc<dyn ToolProfileProvider> =
        Arc::new(StaticToolProfileProvider::new(profiles));
    let egress_rules: Vec<Arc<dyn crate::domain::EgressPolicyRule>> = vec![
        Arc::new(EgressBudgetHardCapRule {
            hard_cap_chars: usize::MAX,
        }),
        Arc::new(EgressSensitiveRule {
            filter: None,
            action: crate::domain::SensitiveAction::Allow,
        }),
    ];
    let tool_rules: Vec<Arc<dyn crate::domain::ToolPolicyRule>> = vec![
        Arc::new(ShellAllowlistRule {
            shell_tool_name: "run_shell",
        }),
        Arc::new(ToolModeRule),
    ];
    let chain = PolicyChain {
        egress_rules,
        tool_rules,
    };
    StdPolicyEngine::new(chain, tool_profiles)
}

/// 未登録ツールは profile 既定で RequireApproval → ToolModeRule が RequireApproval を返す
#[test]
fn test_unregistered_tool_gets_require_approval_by_default() {
    let pe = engine_with_allowlist(vec!["ls"]);
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"path": "/tmp/file"});
    let verdict = pe
        .evaluate_tool_call("read_file", &args, &ctx, false)
        .unwrap();
    match verdict {
        PolicyVerdict::RequireApproval { decision, .. } => {
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.subject, "read_file");
            assert_eq!(decision.status, "warn");
            assert_eq!(decision.reason, "tool_mode_require_approval");
        }
        _ => panic!("expected RequireApproval for unregistered tool (default profile)"),
    }
}

#[test]
fn test_shell_in_allowlist_returns_allow() {
    let pe = engine_with_allowlist(vec!["ls"]);
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "ls -la"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, false)
        .unwrap();
    match verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.status, "allowed");
            assert_eq!(decision.reason, "shell_allowlist");
            assert_eq!(decision.details["command"].as_str().unwrap(), "ls -la");
        }
        _ => panic!("expected Allow for allowlisted command"),
    }
}

#[test]
fn test_shell_not_in_allowlist_returns_require_approval_when_interactive() {
    let pe = engine_with_allowlist(vec!["ls"]);
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, false)
        .unwrap();
    match verdict {
        PolicyVerdict::RequireApproval {
            prompt, decision, ..
        } => {
            assert_eq!(prompt, "rm -rf /");
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.subject, "run_shell");
            assert_eq!(decision.status, "warn");
            assert_eq!(decision.reason, "shell_approval_required");
        }
        _ => panic!("expected RequireApproval for dangerous command"),
    }
}

#[test]
fn test_shell_not_in_allowlist_returns_deny_when_non_interactive() {
    let pe = engine_with_allowlist(vec!["ls"]);
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, true)
        .unwrap();
    match verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.scope, "tool");
            assert_eq!(decision.status, "blocked");
            assert_eq!(decision.reason, "shell_not_allowlisted_non_interactive");
        }
        _ => panic!("expected Deny when non_interactive"),
    }
}

#[test]
fn test_require_approval_provides_unsafe_context() {
    let pe = engine_with_allowlist(vec!["ls"]);
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "echo hello"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, false)
        .unwrap();
    match verdict {
        PolicyVerdict::RequireApproval { value, .. } => {
            assert!(
                value.allow_unsafe,
                "RequireApproval should provide allow_unsafe context"
            );
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
        reason: "shell_allowlist".to_string(),
        details: serde_json::json!({"command": "ls -la"}),
    };
    let payload = d.to_event_payload();
    assert_eq!(payload["v"], 1);
    assert_eq!(payload["scope"], "tool");
    assert_eq!(payload["subject"], "run_shell");
    assert_eq!(payload["status"], "allowed");
    assert_eq!(payload["details"]["command"], "ls -la");
}
