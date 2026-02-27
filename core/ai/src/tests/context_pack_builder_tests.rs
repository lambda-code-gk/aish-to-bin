//! ContextPackBuilder のテスト（QueryPlacement 動作が ContextMessageBuilder と等価であること、BudgetReport の検証）

use crate::adapter::{PassThroughReducer, StdContextPackBuilder, TailWindowReducer};
use crate::domain::{ContextBudget, Query};
use crate::ports::outbound::{ContextPackBuilder, QueryPlacement};
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;
use std::path::PathBuf;
use std::sync::Arc;

fn no_addons_budget() -> ContextBudget {
    ContextBudget {
        max_messages: 0,
        max_chars: 0,
    }
}

fn make_passthrough_builder() -> StdContextPackBuilder {
    StdContextPackBuilder::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![],
        no_addons_budget(),
        PathBuf::from("."),
    )
}

fn make_tail_builder(max_messages: usize, max_chars: usize) -> StdContextPackBuilder {
    StdContextPackBuilder::new(
        Arc::new(TailWindowReducer),
        ContextBudget {
            max_messages,
            max_chars,
        },
        vec![],
        no_addons_budget(),
        PathBuf::from("."),
    )
}

#[test]
fn test_append_at_end_adds_query_and_budget_report() {
    let builder = make_passthrough_builder();
    let history = vec![LlmMessage::user("prior")];
    let query = Query::new("new query");
    let pack = builder
        .build(&history, Some(&query), Some("sys"), QueryPlacement::AppendAtEnd)
        .expect("build should succeed");

    assert!(pack.messages.len() >= 2);
    assert!(matches!(&pack.messages[0], Msg::System(_)));
    let last = pack.messages.last().unwrap();
    assert!(matches!(last, Msg::User(s) if s == "new query"));

    assert_eq!(pack.budget_report.v, 1);
    assert!(pack.attachments.is_empty());
}

#[test]
fn test_already_in_history_does_not_duplicate_query() {
    let builder = make_passthrough_builder();
    let history = vec![
        LlmMessage::user("prior"),
        LlmMessage::user("already in history"),
    ];
    let query = Query::new("already in history");
    let pack = builder
        .build(
            &history,
            Some(&query),
            Some("sys"),
            QueryPlacement::AlreadyInHistory,
        )
        .expect("build should succeed");

    let user_count = pack.messages.iter().filter(|m| matches!(m, Msg::User(_))).count();
    assert_eq!(user_count, 2);
    let last = pack.messages.last().unwrap();
    assert!(matches!(last, Msg::User(s) if s == "already in history"));
}

#[test]
fn test_resume_no_query() {
    let builder = make_passthrough_builder();
    let history = vec![LlmMessage::user("only one")];
    let pack = builder
        .build(&history, None, Some("sys"), QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    assert_eq!(pack.messages.len(), 2);
    assert!(matches!(&pack.messages[1], Msg::User(s) if s == "only one"));
}

#[test]
fn test_tail_window_truncates_and_reports() {
    let builder = make_tail_builder(2, 100_000);
    let history = vec![
        LlmMessage::user("a"),
        LlmMessage::user("b"),
        LlmMessage::user("c"),
    ];
    let query = Query::new("d");
    let pack = builder
        .build(&history, Some(&query), None, QueryPlacement::AppendAtEnd)
        .expect("build should succeed");

    let history_decision = pack.budget_report.decisions.iter().find(|d| d.stage == "history.reduce").unwrap();
    assert_eq!(history_decision.action, "truncate");

    assert_eq!(pack.messages.len(), 2);
    assert!(matches!(&pack.messages[0], Msg::User(s) if s == "c"));
    assert!(matches!(&pack.messages[1], Msg::User(s) if s == "d"));
}

#[test]
fn test_tail_window_char_budget() {
    let builder = make_tail_builder(100, 5);
    let history = vec![
        LlmMessage::user("aaa"),
        LlmMessage::user("bb"),
        LlmMessage::user("c"),
    ];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let history_decision = pack.budget_report.decisions.iter().find(|d| d.stage == "history.reduce").unwrap();
    assert_eq!(history_decision.action, "truncate");
    assert!(matches!(&pack.messages[0], Msg::User(s) if s == "bb"));
    assert!(matches!(&pack.messages[1], Msg::User(s) if s == "c"));
}

#[test]
fn test_budget_report_serializable() {
    let builder = make_passthrough_builder();
    let history = vec![LlmMessage::user("hello")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    let json = serde_json::to_string(&pack.budget_report).expect("should serialize");
    assert!(json.contains("\"v\":1"));
    assert!(json.contains("\"max_messages\""));
}

#[test]
fn test_system_instruction_prepended() {
    let builder = make_passthrough_builder();
    let history = vec![LlmMessage::user("msg")];
    let pack = builder
        .build(&history, None, Some("system prompt"), QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    assert!(matches!(&pack.messages[0], Msg::System(s) if s == "system prompt"));
    assert!(matches!(&pack.messages[1], Msg::User(s) if s == "msg"));
}

#[test]
fn test_no_system_instruction() {
    let builder = make_passthrough_builder();
    let history = vec![LlmMessage::user("msg")];
    let pack = builder
        .build(&history, None, None, QueryPlacement::AlreadyInHistory)
        .expect("build should succeed");

    assert_eq!(pack.messages.len(), 1);
    assert!(matches!(&pack.messages[0], Msg::User(s) if s == "msg"));
}
