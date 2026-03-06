//! Phase 5: rule chain 順序 — run_shell allowlist 一致で ShellAllowlistRule が Allow、不一致で RequireApproval/Deny

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{
    EgressBudgetHardCapRule, EgressSensitiveRule, ShellAllowlistRule, StaticToolProfileProvider,
    StdPolicyEngine, ToolModeRule,
};
use crate::domain::{PolicyChain, PolicyVerdict, ToolCapability, ToolMode, ToolProfile};
use crate::ports::outbound::{PolicyEngine, ToolProfileProvider};
use common::tool::ToolContext;

fn engine() -> StdPolicyEngine {
    let run_shell_profile = ToolProfile {
        tool_name: "run_shell".to_string(),
        mode: ToolMode::RequireApproval,
        capabilities: vec![ToolCapability::Exec {
            allowlist: vec!["ls".to_string()],
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

/// allowlist 一致 → ShellAllowlistRule が Allow(shell_allowlist) を返し、ToolModeRule に到達しない
#[test]
fn test_rule_order_allowlist_match_returns_allow_first() {
    let pe = engine();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "ls -la"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, false)
        .unwrap();
    match &verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.reason, "shell_allowlist");
        }
        _ => panic!("expected Allow with shell_allowlist"),
    }
}

/// allowlist 外・対話的 → RequireApproval(shell_approval_required)
#[test]
fn test_rule_order_allowlist_miss_interactive_returns_require_approval() {
    let pe = engine();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, false)
        .unwrap();
    match &verdict {
        PolicyVerdict::RequireApproval { decision, .. } => {
            assert_eq!(decision.reason, "shell_approval_required");
        }
        _ => panic!("expected RequireApproval"),
    }
}

/// allowlist 外・non_interactive → Deny(shell_not_allowlisted_non_interactive)
#[test]
fn test_rule_order_allowlist_miss_non_interactive_returns_deny() {
    let pe = engine();
    let ctx = ToolContext::new(None);
    let args = serde_json::json!({"command": "rm -rf /"});
    let verdict = pe
        .evaluate_tool_call("run_shell", &args, &ctx, true)
        .unwrap();
    match &verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.reason, "shell_not_allowlisted_non_interactive");
        }
        _ => panic!("expected Deny"),
    }
}
