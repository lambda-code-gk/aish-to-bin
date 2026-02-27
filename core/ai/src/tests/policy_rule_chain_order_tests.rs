//! ルール順の検証: ShellAllowlistRule が先に評価され、一致時は Allow・不一致時は RequireApproval/Deny

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{ShellAllowlistRule, StaticToolProfileProvider, StdPolicyEngine, ToolModeRule};
use crate::domain::{PolicyChain, PolicyVerdict, ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::PolicyEngine;
use common::tool::{CommandAllowRule, ToolContext};

fn engine() -> StdPolicyEngine {
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
    let provider = Arc::new(StaticToolProfileProvider::new(profiles));
    StdPolicyEngine::new(chain, provider)
}

/// allowlist 一致 → ShellAllowlistRule が先に評価され Allow(shell_allowlist)
#[test]
fn test_rule_order_allowlist_match_returns_allow_first() {
    let pe = engine();
    let ctx = ToolContext::new(None)
        .with_command_allow_rules(vec![CommandAllowRule::Prefix("ls".to_string())]);
    let args = serde_json::json!({"command": "ls -la"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, false).unwrap();
    match &verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.reason, "shell_allowlist");
        }
        _ => panic!("expected Allow with shell_allowlist"),
    }
}

/// allowlist 外・対話的 → ShellAllowlistRule が RequireApproval を返す（ToolModeRule には流れない）
#[test]
fn test_rule_order_allowlist_miss_interactive_returns_require_approval() {
    let pe = engine();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, false).unwrap();
    match &verdict {
        PolicyVerdict::RequireApproval { decision, .. } => {
            assert_eq!(decision.reason, "shell_approval_required");
        }
        _ => panic!("expected RequireApproval"),
    }
}

/// allowlist 外・non_interactive → ShellAllowlistRule が Deny を返す
#[test]
fn test_rule_order_allowlist_miss_non_interactive_returns_deny() {
    let pe = engine();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe.evaluate_tool_call("run_shell", &args, &ctx, true).unwrap();
    match &verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.reason, "shell_not_allowlisted_non_interactive");
        }
        _ => panic!("expected Deny"),
    }
}
