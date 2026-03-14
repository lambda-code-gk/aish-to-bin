//! ShellConsoleSelector のテスト

use crate::adapter::{PassThroughReducer, ShellConsoleSelector, StdContextPackBuilderWithAddons};
use crate::domain::ContextBudget;
use crate::ports::outbound::{
    ContextAddonInput, ContextAddonSelector, ContextPackBuilder, QueryPlacement,
};
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_shell_console_selector_returns_empty_without_session_dir() {
    let selector = ShellConsoleSelector::new(Arc::new(StdFileSystem), 200, 10);
    let project_root = PathBuf::from(".");
    let input = ContextAddonInput {
        history: &[],
        query: None,
        project_root: &project_root,
        session_dir: None,
    };

    let addons = selector.select(&input).expect("select should succeed");
    assert!(addons.is_empty());
}

#[test]
fn test_shell_console_selector_reads_console_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let session_dir = SessionDir::new(tmp.path());
    std::fs::write(
        session_dir.as_ref().join("console.txt"),
        "line1\nline2\nline3\nline4\n",
    )
    .unwrap();

    let selector = ShellConsoleSelector::new(Arc::new(StdFileSystem), 200, 2);
    let project_root = PathBuf::from(".");
    let input = ContextAddonInput {
        history: &[],
        query: None,
        project_root: &project_root,
        session_dir: Some(&session_dir),
    };

    let addons = selector.select(&input).expect("select should succeed");
    assert_eq!(addons.len(), 1);
    let addon = &addons[0];
    assert_eq!(addon.kind, "shell_console");
    let attachment = addon.attachment.as_ref().expect("attachment should exist");
    let content = attachment.content.as_ref().expect("content should exist");
    assert!(content.contains("line3"));
    assert!(content.contains("line4"));
    assert!(!content.contains("line1"));
}

#[test]
fn test_shell_console_selector_truncates_at_utf8_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let session_dir = SessionDir::new(tmp.path());
    std::fs::write(
        session_dir.as_ref().join("console.txt"),
        "aaa\n日本語の行\nbbbb\n",
    )
    .unwrap();

    let selector = ShellConsoleSelector::new(Arc::new(StdFileSystem), 10, 10);
    let project_root = PathBuf::from(".");
    let input = ContextAddonInput {
        history: &[],
        query: None,
        project_root: &project_root,
        session_dir: Some(&session_dir),
    };

    let addons = selector.select(&input).expect("select should succeed");
    let content = addons[0]
        .attachment
        .as_ref()
        .and_then(|a| a.content.as_ref())
        .expect("content should exist");
    assert!(std::str::from_utf8(content.as_bytes()).is_ok());
}

#[test]
fn test_context_pack_builder_includes_shell_console_addon_when_session_dir_is_set() {
    let tmp = tempfile::tempdir().unwrap();
    let session_dir = SessionDir::new(tmp.path());
    std::fs::write(
        session_dir.as_ref().join("console.txt"),
        "prompt\nbuild output\n",
    )
    .unwrap();

    let selector: Arc<dyn ContextAddonSelector> =
        Arc::new(ShellConsoleSelector::new(Arc::new(StdFileSystem), 200, 10));
    let builder = StdContextPackBuilderWithAddons::new(
        Arc::new(PassThroughReducer),
        ContextBudget::legacy(),
        vec![selector],
        ContextBudget {
            max_messages: 4,
            max_chars: 2_000,
        },
        PathBuf::from("."),
        None,
    );

    let pack = builder
        .build(
            &[],
            Some(&session_dir),
            None,
            None,
            QueryPlacement::AlreadyInHistory,
        )
        .expect("build should succeed");

    assert!(pack.messages.iter().any(|m| match m {
        common::msg::Msg::User(text) => text.contains("shell console"),
        _ => false,
    }));
}
