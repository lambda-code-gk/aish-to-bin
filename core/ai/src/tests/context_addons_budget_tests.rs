//! ContextPackBuilder の addons 予算テスト

use crate::adapter::{PassThroughReducer, StdContextPackBuilder};
use crate::domain::{ContextAddon, ContextAttachment, ContextBudget, ContextSource, Query};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector, ContextPackBuilder, QueryPlacement};
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

struct FailingSelector;

impl ContextAddonSelector for FailingSelector {
    fn name(&self) -> &str {
        "failing"
    }
    fn select(&self, _input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error> {
        Err(Error::system("selector exploded"))
    }
}

fn make_addon(id: &str, priority: u32, text: &str) -> ContextAddon {
    ContextAddon {
        id: id.to_string(),
        kind: "test".to_string(),
        title: id.to_string(),
        priority,
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
fn test_addons_budget_limits_to_one() {
    let addon_a = make_addon("a", 100, "addon-a-content");
    let addon_b = make_addon("b", 50, "addon-b-content");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon_a, addon_b],
    });

    let builder = StdContextPackBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget { max_messages: 100, max_chars: 100_000 },
        vec![selector],
        ContextBudget { max_messages: 1, max_chars: 100_000 },
        PathBuf::from("."),
        None,
    );

    let history = vec![LlmMessage::user("history")];
    let query = Query::new("user query");
    let pack = builder
        .build(&history, Some(&query), None, QueryPlacement::AppendAtEnd)
        .expect("build should succeed");

    let keep_count = pack.budget_report.decisions.iter()
        .filter(|d| d.stage == "addon.select" && d.action == "keep")
        .count();
    let drop_count = pack.budget_report.decisions.iter()
        .filter(|d| d.stage == "addon.select" && d.action == "drop")
        .count();
    assert_eq!(keep_count, 1);
    assert_eq!(drop_count, 1);

    assert_eq!(pack.attachments.len(), 1);

    // "a" has higher priority so it should be kept
    let kept_decision = pack.budget_report.decisions.iter()
        .find(|d| d.stage == "addon.select" && d.action == "keep")
        .unwrap();
    assert_eq!(kept_decision.details["addon_id"], "a");
}

#[test]
fn test_addons_inserted_before_query() {
    let addon = make_addon("ctx", 50, "context info");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon],
    });

    let builder = StdContextPackBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget { max_messages: 100, max_chars: 100_000 },
        vec![selector],
        ContextBudget { max_messages: 10, max_chars: 100_000 },
        PathBuf::from("."),
        None,
    );

    let history = vec![LlmMessage::user("history msg")];
    let query = Query::new("my query");
    let pack = builder
        .build(&history, Some(&query), Some("sys"), QueryPlacement::AppendAtEnd)
        .expect("build should succeed");

    // last user message should be the query, not the addon
    let last = pack.messages.last().unwrap();
    assert!(matches!(last, Msg::User(s) if s == "my query"), "query should be last user message");

    // addon should be somewhere before the query
    let addon_pos = pack.messages.iter().position(|m| {
        if let Msg::User(s) = m { s.contains("context info") } else { false }
    });
    let query_pos = pack.messages.iter().rposition(|m| {
        if let Msg::User(s) = m { s == "my query" } else { false }
    });
    assert!(addon_pos.unwrap() < query_pos.unwrap());
}

#[test]
fn test_selector_failure_recorded_as_decision() {
    let failing: Arc<dyn ContextAddonSelector> = Arc::new(FailingSelector);

    let builder = StdContextPackBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![failing],
        ContextBudget { max_messages: 8, max_chars: 8_000 },
        PathBuf::from("."),
        None,
    );

    let history = vec![LlmMessage::user("hello")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed despite selector failure");

    let error_decision = pack.budget_report.decisions.iter()
        .find(|d| d.stage == "addon.selector" && d.action == "error");
    assert!(error_decision.is_some());
    assert_eq!(error_decision.unwrap().reason, "failing");
}

#[test]
fn test_addons_char_budget_limits() {
    let addon_big = make_addon("big", 100, &"x".repeat(5000));
    let addon_small = make_addon("small", 50, "tiny");
    let selector: Arc<dyn ContextAddonSelector> = Arc::new(StubSelector {
        addons: vec![addon_big, addon_small],
    });

    let builder = StdContextPackBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget { max_messages: 100, max_chars: 100_000 },
        vec![selector],
        ContextBudget { max_messages: 10, max_chars: 100 },
        PathBuf::from("."),
        None,
    );

    let history = vec![LlmMessage::user("history")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let keep_count = pack.budget_report.decisions.iter()
        .filter(|d| d.stage == "addon.select" && d.action == "keep")
        .count();
    let drop_count = pack.budget_report.decisions.iter()
        .filter(|d| d.stage == "addon.select" && d.action == "drop")
        .count();

    // "big" is 5000 chars, exceeds addons_budget.max_chars=100, so it's dropped
    // "small" is 4 chars, fits
    assert_eq!(drop_count, 1);
    assert_eq!(keep_count, 1);

    let kept = pack.budget_report.decisions.iter()
        .find(|d| d.stage == "addon.select" && d.action == "keep")
        .unwrap();
    assert_eq!(kept.details["addon_id"], "small");
}
