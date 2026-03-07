//! MemorySelector のテスト

use crate::adapter::context::memory_storage;
use crate::adapter::MemorySelector;
use crate::domain::{MemoryEntry, Query};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector, ResolveMemoryDir};
use common::error::Error;
use common::msg::Msg;
use std::path::PathBuf;
use std::sync::Arc;

struct StubResolveMemoryDir {
    global_dir: PathBuf,
}

impl ResolveMemoryDir for StubResolveMemoryDir {
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error> {
        Ok((None, self.global_dir.clone()))
    }
}

#[test]
fn test_memory_selector_returns_addon_for_matching_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let global_dir = tmp.path().join("global_memory");
    std::fs::create_dir_all(&global_dir).unwrap();

    let entry = MemoryEntry::new(
        "mem1",
        "Rust is great for systems programming",
        "tech",
        vec!["rust".to_string(), "systems".to_string()],
        "Rust overview",
        "2026-01-01T00:00:00Z",
    );
    memory_storage::save_entry(&global_dir, &entry, None).unwrap();

    let resolve: Arc<dyn ResolveMemoryDir> = Arc::new(StubResolveMemoryDir {
        global_dir: global_dir.clone(),
    });
    let selector = MemorySelector::new(resolve, 5, 1200);
    let query = Query::new("rust systems");
    let project_root = PathBuf::from(".");
    let input = ContextAddonInput {
        history: &[],
        query: Some(&query),
        project_root: &project_root,
    };
    let addons = selector.select(&input).expect("select should succeed");
    assert!(!addons.is_empty(), "should return at least one addon");

    let addon = &addons[0];
    assert_eq!(addon.kind, "memory");
    assert!(matches!(&addon.msg, Msg::User(_)));
    assert!(addon.attachment.is_some());
    let att = addon.attachment.as_ref().unwrap();
    assert!(att.content.is_some());
    assert!(att.content.as_ref().unwrap().contains("Rust is great"));
}

#[test]
fn test_memory_selector_returns_empty_when_no_query() {
    let tmp = tempfile::tempdir().unwrap();
    let resolve: Arc<dyn ResolveMemoryDir> = Arc::new(StubResolveMemoryDir {
        global_dir: tmp.path().to_path_buf(),
    });
    let selector = MemorySelector::new(resolve, 5, 1200);
    let project_root = PathBuf::from(".");
    let input = ContextAddonInput {
        history: &[],
        query: None,
        project_root: &project_root,
    };
    let addons = selector.select(&input).expect("select should succeed");
    assert!(addons.is_empty());
}
