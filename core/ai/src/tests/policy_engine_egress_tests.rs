//! StdPolicyEngine: egress 判定のテスト（Phase 5: rule chain + sensitive_filter）

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{
    EgressBudgetHardCapRule, EgressSensitiveRule, ShellAllowlistRule, StaticToolProfileProvider,
    StdPolicyEngine, ToolModeRule,
};
use crate::domain::{
    PolicyChain, SensitiveAction, ToolMode, ToolProfile,
    BudgetReport, Budget, BudgetStats, ContextAttachment, ContextPack, PolicyVerdict,
    SensitiveFilterOutcome,
};
use crate::ports::outbound::{PolicyEngine, SensitiveTextFilter, ToolProfileProvider};
use common::error::Error;
use common::msg::Msg;

fn empty_budget_report() -> BudgetReport {
    BudgetReport {
        v: 1,
        budget: Budget {
            max_messages: 100,
            max_chars: 100_000,
        },
        input: BudgetStats {
            message_count: 0,
            char_count: 0,
        },
        output: BudgetStats {
            message_count: 0,
            char_count: 0,
        },
        decisions: vec![],
    }
}

fn simple_pack(messages: Vec<Msg>) -> ContextPack {
    ContextPack {
        v: 1,
        messages,
        attachments: vec![],
        budget_report: empty_budget_report(),
    }
}

/// "SECRET" を含むテキストを Deny する SensitiveTextFilter
struct DenyOnSecretFilter;
impl SensitiveTextFilter for DenyOnSecretFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        if content.contains("SECRET") {
            Ok(SensitiveFilterOutcome::Deny {
                verbose: "found SECRET keyword".to_string(),
            })
        } else {
            Ok(SensitiveFilterOutcome::Clean)
        }
    }
}

/// 常に Err を返す SensitiveTextFilter（fail-closed 検証用）
struct FailingFilter;
impl SensitiveTextFilter for FailingFilter {
    fn filter(&self, _content: &str) -> Result<SensitiveFilterOutcome, Error> {
        Err(Error::system("scan failed".to_string()))
    }
}

/// "SECRET" を含むテキストを "***" にマスクする SensitiveTextFilter
struct MaskOnSecretFilter;
impl SensitiveTextFilter for MaskOnSecretFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        if content.contains("SECRET") {
            let masked = content.replace("SECRET", "***");
            Ok(SensitiveFilterOutcome::Masked {
                masked,
                verbose: "masked SECRET".to_string(),
            })
        } else {
            Ok(SensitiveFilterOutcome::Clean)
        }
    }
}

fn make_egress_engine(
    filter: Option<Arc<dyn SensitiveTextFilter>>,
    action: SensitiveAction,
) -> StdPolicyEngine {
    let egress_rules: Vec<Arc<dyn crate::domain::EgressPolicyRule>> = vec![
        Arc::new(EgressBudgetHardCapRule {
            hard_cap_chars: usize::MAX,
        }),
        Arc::new(EgressSensitiveRule {
            filter,
            action,
        }),
    ];
    let run_shell_profile = ToolProfile {
        tool_name: "run_shell".to_string(),
        mode: ToolMode::RequireApproval,
        capabilities: vec![],
        notes: None,
    };
    let mut profiles = HashMap::new();
    profiles.insert("run_shell".to_string(), run_shell_profile);
    let tool_profiles: Arc<dyn ToolProfileProvider> =
        Arc::new(StaticToolProfileProvider::new(profiles));
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

#[test]
fn test_no_filter_allows_all() {
    let pe = make_egress_engine(None, SensitiveAction::Allow);
    let pack = simple_pack(vec![Msg::user("hello")]);
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.reason, "default_allow");
            assert_eq!(decision.status, "allowed");
        }
        _ => panic!("expected Allow"),
    }
}

#[test]
fn test_clean_content_passes() {
    let pe = make_egress_engine(
        Some(Arc::new(DenyOnSecretFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Deny,
    );
    let pack = simple_pack(vec![Msg::user("hello world")]);
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { decision, .. } => {
            assert_eq!(decision.reason, "default_allow");
        }
        _ => panic!("expected Allow for clean content"),
    }
}

#[test]
fn test_deny_filter_blocks_pack() {
    let pe = make_egress_engine(
        Some(Arc::new(DenyOnSecretFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Deny,
    );
    let pack = simple_pack(vec![Msg::user("my SECRET password")]);
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.status, "blocked");
            assert_eq!(decision.reason, "sensitive_deny");
            assert_eq!(decision.details["hits"], 1);
        }
        _ => panic!("expected Deny for SECRET content"),
    }
}

#[test]
fn test_mask_filter_replaces_content() {
    let pe = make_egress_engine(
        Some(Arc::new(MaskOnSecretFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Mask,
    );
    let pack = simple_pack(vec![Msg::user("my SECRET password")]);
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { value, decision } => {
            assert_eq!(decision.status, "warn");
            assert_eq!(decision.reason, "sensitive_masked");
            match &value.messages[0] {
                Msg::User(s) => {
                    assert_eq!(s, "my *** password");
                    assert!(!s.contains("SECRET"));
                }
                _ => panic!("expected User message"),
            }
        }
        _ => panic!("expected Allow with masked content"),
    }
}

#[test]
fn test_mask_filter_on_attachment() {
    let pe = make_egress_engine(
        Some(Arc::new(MaskOnSecretFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Mask,
    );
    let mut pack = simple_pack(vec![Msg::user("safe")]);
    pack.attachments.push(ContextAttachment {
        kind: "file".to_string(),
        title: "secret.txt".to_string(),
        content_type: "text/plain".to_string(),
        content: Some("contains SECRET data".to_string()),
        artifact_rel_path: None,
        bytes: 20,
        hash64: "0000000000000000".to_string(),
        source: None,
    });
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Allow { value, decision } => {
            assert_eq!(decision.reason, "sensitive_masked");
            let att = &value.attachments[0];
            assert_eq!(att.content.as_deref(), Some("contains *** data"));
            assert_ne!(att.hash64, "0000000000000000", "hash should be recalculated");
        }
        _ => panic!("expected Allow with masked attachment"),
    }
}

#[test]
fn test_filter_error_returns_deny_fail_closed() {
    let pe = make_egress_engine(
        Some(Arc::new(FailingFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Mask,
    );
    let pack = simple_pack(vec![Msg::user("hello")]);
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.reason, "sensitive_scan_error");
            assert_eq!(decision.status, "blocked");
        }
        _ => panic!("expected Deny on filter error (fail-closed)"),
    }
}

#[test]
fn test_deny_filter_on_attachment() {
    let pe = make_egress_engine(
        Some(Arc::new(DenyOnSecretFilter) as Arc<dyn SensitiveTextFilter>),
        SensitiveAction::Deny,
    );
    let mut pack = simple_pack(vec![Msg::user("safe")]);
    pack.attachments.push(ContextAttachment {
        kind: "file".to_string(),
        title: "secret.txt".to_string(),
        content_type: "text/plain".to_string(),
        content: Some("SECRET inside".to_string()),
        artifact_rel_path: None,
        bytes: 13,
        hash64: "0000000000000000".to_string(),
        source: None,
    });
    let verdict = pe.evaluate_egress_context_pack(&pack, false).unwrap();
    match verdict {
        PolicyVerdict::Deny { decision } => {
            assert_eq!(decision.reason, "sensitive_deny");
            assert!(decision.details["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t.as_str().unwrap().contains("secret.txt")));
        }
        _ => panic!("expected Deny for SECRET in attachment"),
    }
}
