//! ContextArtifactStore のテスト

use crate::adapter::StdContextArtifactStore;
use crate::domain::ContextAttachment;
use crate::ports::outbound::ContextArtifactStore;
use common::adapter::StdFileSystem;
use common::domain::event::RunId;
use common::domain::SessionDir;
use common::ports::outbound::FileSystem;
use std::sync::Arc;

#[test]
fn test_store_writes_files_and_returns_refs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let run_id = RunId::new("test_run_001");
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

    let store = StdContextArtifactStore::new(Arc::clone(&fs));

    let attachments = vec![
        ContextAttachment {
            kind: "file_snippet".to_string(),
            title: "src/main.rs".to_string(),
            content_type: "text/plain".to_string(),
            content: Some("fn main() {}".to_string()),
            artifact_rel_path: None,
            bytes: 12,
            hash64: "0000000000000000".to_string(),
            source: None,
        },
        ContextAttachment {
            kind: "file_snippet".to_string(),
            title: "README.md".to_string(),
            content_type: "text/plain".to_string(),
            content: Some("# Hello".to_string()),
            artifact_rel_path: None,
            bytes: 7,
            hash64: "0000000000000000".to_string(),
            source: None,
        },
    ];

    let result = store
        .store(&session_dir, &run_id, &attachments)
        .expect("store should succeed");

    assert_eq!(result.len(), 2);
    for att in &result {
        assert!(att.content.is_none(), "content should be None after store");
        assert!(
            att.artifact_rel_path.is_some(),
            "artifact_rel_path should be set"
        );
        let rel_path = att.artifact_rel_path.as_ref().unwrap();
        assert!(rel_path.starts_with("artifacts/context/test_run_001/"));
        let full_path = tmp.path().join(rel_path);
        assert!(
            fs.exists(&full_path),
            "artifact file should exist: {:?}",
            full_path
        );
    }

    // Verify written content
    let first_path = tmp
        .path()
        .join(result[0].artifact_rel_path.as_ref().unwrap());
    let content = fs.read_to_string(&first_path).expect("read");
    assert_eq!(content, "fn main() {}");
}

#[test]
fn test_store_skips_none_content() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let run_id = RunId::new("test_run_002");
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

    let store = StdContextArtifactStore::new(Arc::clone(&fs));

    let attachments = vec![ContextAttachment {
        kind: "ref".to_string(),
        title: "already_stored".to_string(),
        content_type: "text/plain".to_string(),
        content: None,
        artifact_rel_path: Some("existing/path.txt".to_string()),
        bytes: 0,
        hash64: "0000000000000000".to_string(),
        source: None,
    }];

    let result = store
        .store(&session_dir, &run_id, &attachments)
        .expect("store should succeed");
    assert_eq!(result.len(), 1);
    assert!(result[0].content.is_none());
    assert_eq!(
        result[0].artifact_rel_path.as_deref(),
        Some("existing/path.txt")
    );
}

#[test]
fn test_store_sanitizes_title_in_filename() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let run_id = RunId::new("run_san");
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

    let store = StdContextArtifactStore::new(Arc::clone(&fs));

    let attachments = vec![ContextAttachment {
        kind: "test".to_string(),
        title: "../../etc/passwd".to_string(),
        content_type: "text/plain".to_string(),
        content: Some("safe content".to_string()),
        artifact_rel_path: None,
        bytes: 12,
        hash64: "0000000000000000".to_string(),
        source: None,
    }];

    let result = store
        .store(&session_dir, &run_id, &attachments)
        .expect("store");
    let rel = result[0].artifact_rel_path.as_ref().unwrap();
    assert!(!rel.contains(".."), "path traversal should be sanitized");
    assert!(rel.starts_with("artifacts/context/run_san/"));
}
