//! GrepHitsSelector のテスト

use crate::adapter::context_addon_selectors_grep::GrepHitsSelector;
use crate::domain::Query;
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector};
use common::adapter::StdFileSystem;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

#[test]
fn test_grep_selector_finds_token_in_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    std::fs::write(
        root.join("a.txt"),
        "line one\nhello_token_123 is here\nline three\n",
    )
    .unwrap();

    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let selector = GrepHitsSelector::new(
        fs,
        100,
        50,
        100_000,
        4,
        vec![".git".to_string(), "target".to_string()],
    );

    let query = Query::new("hello_token_123");
    let input = ContextAddonInput {
        history: &[],
        query: Some(&query),
        project_root: root,
    };
    let addons = selector.select(&input).expect("select should succeed");
    assert!(!addons.is_empty(), "should find at least one addon");

    let addon = &addons[0];
    assert_eq!(addon.kind, "grep_hits");
    assert!(matches!(&addon.msg, Msg::User(s) if s.contains("hello_token_123")));

    let att = addon.attachment.as_ref().unwrap();
    let content = att.content.as_ref().unwrap();
    assert!(
        content.contains("a.txt:2:"),
        "should contain file:line reference (1-based)"
    );
}

#[test]
fn test_grep_selector_ignores_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(
        root.join("target").join("hidden.txt"),
        "findme_token here\n",
    )
    .unwrap();

    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let selector = GrepHitsSelector::new(fs, 100, 50, 100_000, 4, vec!["target".to_string()]);

    let query = Query::new("findme_token");
    let input = ContextAddonInput {
        history: &[],
        query: Some(&query),
        project_root: root,
    };
    let addons = selector.select(&input).expect("select should succeed");
    assert!(
        addons.is_empty(),
        "files in ignored dirs should not be searched"
    );
}

#[test]
fn test_grep_selector_returns_empty_when_no_query() {
    let tmp = tempfile::tempdir().unwrap();
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let selector = GrepHitsSelector::new(fs, 100, 50, 100_000, 4, vec![]);
    let input = ContextAddonInput {
        history: &[],
        query: None,
        project_root: tmp.path(),
    };
    let addons = selector.select(&input).expect("select should succeed");
    assert!(addons.is_empty());
}
