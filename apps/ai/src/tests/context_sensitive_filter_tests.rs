//! StdContextPackBuilder の sensitive filter 統合テスト

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

struct DenyFilter;
impl SensitiveTextFilter for DenyFilter {
    fn filter(&self, _content: &str) -> Result<SensitiveFilterOutcome, Error> {
        Ok(SensitiveFilterOutcome::Deny {
            verbose: "HIT: test deny".to_string(),
        })
    }
}

struct MaskFilter;
impl SensitiveTextFilter for MaskFilter {
    fn filter(&self, _content: &str) -> Result<SensitiveFilterOutcome, Error> {
        Ok(SensitiveFilterOutcome::Masked {
            masked: "[REDACTED]".to_string(),
            verbose: "HIT: test mask".to_string(),
        })
    }
}

struct CleanFilter;
impl SensitiveTextFilter for CleanFilter {
    fn filter(&self, _content: &str) -> Result<SensitiveFilterOutcome, Error> {
        Ok(SensitiveFilterOutcome::Clean)
    }
}

fn make_addon(id: &str, text: &str) -> ContextAddon {
    ContextAddon {
        id: id.to_string(),
        kind: "test".to_string(),
        title: id.to_string(),
        priority: 80,
        msg: Msg::user(text.to_string()),
        attachment: Some(ContextAttachment {
            kind: "test".to_string(),
            title: id.to_string(),
            content_type: "text/plain".to_string(),
            content: Some(text.to_string()),
            artifact_rel_path: None,
            bytes: text.len() as u64,
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
fn test_deny_filter_drops_addon_with_decision() {
    let addon = make_addon("secret", "my secret content");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(DenyFilter);

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

    let history = vec![LlmMessage::user("hello")];
    let pack = builder
        .build(&history, None, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let deny_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive" && d.action == "deny")
        .collect();
    assert!(!deny_decisions.is_empty(), "should have deny decision");
    assert!(
        pack.attachments.is_empty(),
        "denied addon should have no attachments"
    );

    let has_addon_msg = pack.messages.iter().any(|m| {
        if let Msg::User(s) = m {
            s.contains("my secret content")
        } else {
            false
        }
    });
    assert!(!has_addon_msg, "denied addon msg should not be in messages");
}

#[test]
fn test_mask_filter_replaces_addon_content_with_decision() {
    let addon = make_addon("maskme", "sensitive info");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(MaskFilter);

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

    let history = vec![LlmMessage::user("hello")];
    let pack = builder
        .build(&history, None, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let mask_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive" && d.action == "mask")
        .collect();
    assert!(mask_decisions.len() >= 1, "should have mask decision(s)");

    let has_redacted = pack.messages.iter().any(|m| {
        if let Msg::User(s) = m {
            s.contains("[REDACTED]")
        } else {
            false
        }
    });
    assert!(has_redacted, "masked addon msg should contain [REDACTED]");
}

#[test]
fn test_clean_filter_passes_addon_through() {
    let addon = make_addon("safe", "clean content");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });
    let filter: Arc<dyn SensitiveTextFilter> = Arc::new(CleanFilter);

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

    let history = vec![LlmMessage::user("hello")];
    let pack = builder
        .build(&history, None, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let sensitive_decisions: Vec<_> = pack
        .budget_report
        .decisions
        .iter()
        .filter(|d| d.stage == "addon.sensitive")
        .collect();
    assert!(
        sensitive_decisions.is_empty(),
        "clean should not generate sensitive decisions"
    );

    assert_eq!(
        pack.attachments.len(),
        1,
        "clean addon attachment should remain"
    );
}
