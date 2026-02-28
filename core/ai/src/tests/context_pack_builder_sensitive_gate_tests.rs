//! StdContextPackBuilder の addons leakscan gate テスト（decisions 検証）

use crate::adapter::{PassThroughReducer, StdContextPackBuilderWithAddons};
use crate::domain::{
    ContextAddon, ContextAttachment, ContextBudget, ContextSource, SensitiveFilterOutcome,
};
use crate::ports::outbound::{
    ContextAddonInput, ContextAddonSelector, ContextPackBuilder, QueryPlacement,
    SensitiveTextFilter,
};
use common::error::Error;
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use std::path::PathBuf;
use std::sync::Arc;

struct StubSelector {
    addons: Vec<ContextAddon>,
}
impl ContextAddonSelector for StubSelector {
    fn name(&self) -> &str {
        "stub"
    }
    fn select(&self, _input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error> {
        Ok(self.addons.clone())
    }
}

/// "SECRET" を含むテキストを Deny
struct SecretDenyFilter;
impl SensitiveTextFilter for SecretDenyFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        if content.contains("SECRET") {
            Ok(SensitiveFilterOutcome::Deny {
                verbose: "SECRET found".to_string(),
            })
        } else {
            Ok(SensitiveFilterOutcome::Clean)
        }
    }
}

/// "SECRET" を含むテキストを "***" にマスク
struct SecretMaskFilter;
impl SensitiveTextFilter for SecretMaskFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        if content.contains("SECRET") {
            Ok(SensitiveFilterOutcome::Masked {
                masked: content.replace("SECRET", "***"),
                verbose: "SECRET masked".to_string(),
            })
        } else {
            Ok(SensitiveFilterOutcome::Clean)
        }
    }
}

/// "SECRET" を含むテキストは Hit（Allow 用）
struct SecretHitFilter;
impl SensitiveTextFilter for SecretHitFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        if content.contains("SECRET") {
            Ok(SensitiveFilterOutcome::Hit {
                verbose: "SECRET hit (allow)".to_string(),
            })
        } else {
            Ok(SensitiveFilterOutcome::Clean)
        }
    }
}

fn make_addon_with_secret(id: &str, msg_text: &str, attachment_text: &str) -> ContextAddon {
    ContextAddon {
        id: id.to_string(),
        kind: "test".to_string(),
        title: id.to_string(),
        priority: 80,
        msg: Msg::user(msg_text.to_string()),
        attachment: Some(ContextAttachment {
            kind: "test".to_string(),
            title: id.to_string(),
            content_type: "text/plain".to_string(),
            content: Some(attachment_text.to_string()),
            artifact_rel_path: None,
            bytes: attachment_text.len() as u64,
            hash64: "0000000000000000".to_string(),
            source: Some(ContextSource {
                kind: "test".to_string(),
                ref_id: id.to_string(),
            }),
        }),
        source: None,
    }
}

#[test]
fn test_sensitive_deny_drops_addon_and_records_decision() {
    let addon = make_addon_with_secret("a1", "Context contains SECRET", "attachment SECRET data");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(SecretDenyFilter);

    let builder = StdContextPackBuilderWithAddons::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![selector],
        ContextBudget {
            max_messages: 10,
            max_chars: 100_000,
        },
        PathBuf::from("."),
        Some(filter),
    );

    let history = vec![LlmMessage::user("hi")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let deny_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive" && d.action == "deny")
        .collect();
    assert!(!deny_decisions.is_empty(), "BudgetReport.decisions should contain addon.sensitive deny");
    assert!(pack.attachments.is_empty(), "denied addon should be dropped");
}

#[test]
fn test_sensitive_mask_replaces_msg_and_attachment() {
    let addon = make_addon_with_secret("a2", "SECRET in msg", "SECRET in attachment");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(SecretMaskFilter);

    let builder = StdContextPackBuilderWithAddons::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![selector],
        ContextBudget {
            max_messages: 10,
            max_chars: 100_000,
        },
        PathBuf::from("."),
        Some(filter),
    );

    let history = vec![LlmMessage::user("hi")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let mask_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive" && d.action == "mask")
        .collect();
    assert!(mask_decisions.len() >= 1, "should have addon.sensitive mask decision(s)");

    let has_masked_msg = pack.messages.iter().any(|m| {
        if let Msg::User(s) = m {
            s.contains("***") && !s.contains("SECRET")
        } else {
            false
        }
    });
    assert!(has_masked_msg, "msg should be masked");

    let att = pack.attachments.first().expect("one attachment");
    let content = att.content.as_ref().expect("content present");
    assert!(content.contains("***"), "attachment content should be masked");
    assert!(!content.contains("SECRET"), "attachment should not contain SECRET");
}

#[test]
fn test_sensitive_allow_keeps_addon_and_records_allow_decision() {
    let addon = make_addon_with_secret("a3", "SECRET allowed", "SECRET in body");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(SecretHitFilter);

    let builder = StdContextPackBuilderWithAddons::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![selector],
        ContextBudget {
            max_messages: 10,
            max_chars: 100_000,
        },
        PathBuf::from("."),
        Some(filter),
    );

    let history = vec![LlmMessage::user("hi")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let allow_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive" && d.action == "allow")
        .collect();
    assert!(!allow_decisions.is_empty(), "should have addon.sensitive allow (warn) decision");

    assert_eq!(pack.attachments.len(), 1, "addon should be kept");
    let has_secret_msg = pack.messages.iter().any(|m| {
        if let Msg::User(s) = m {
            s.contains("SECRET")
        } else {
            false
        }
    });
    assert!(has_secret_msg, "msg should remain unchanged (allow)");
}
