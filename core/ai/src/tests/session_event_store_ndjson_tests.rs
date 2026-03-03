//! NdjsonSessionEventStore のテスト

use crate::domain::EventEnvelope;
use crate::ports::outbound::SessionEventStore;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use common::ports::outbound::FileSystem;
use std::sync::Arc;
use storage::NdjsonSessionEventStore;

#[test]
fn test_append_and_read_all_same_seq_kind() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = NdjsonSessionEventStore::new(Arc::clone(&fs));

    let seq1 = store.next_seq(&session_dir).unwrap();
    assert_eq!(seq1, 1);
    store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: seq1,
                ts_ms: 1000,
                session_id: "s1".to_string(),
                run_id: Some("r1".to_string()),
                kind: "context.pack_built".to_string(),
                payload: serde_json::json!({"addons_count": 2}),
            },
        )
        .unwrap();

    let seq2 = store.next_seq(&session_dir).unwrap();
    assert_eq!(seq2, 2);
    store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: seq2,
                ts_ms: 2000,
                session_id: "s1".to_string(),
                run_id: Some("r1".to_string()),
                kind: "policy.evaluated".to_string(),
                payload: serde_json::json!({"status": "allowed"}),
            },
        )
        .unwrap();

    let read: Vec<_> = store.read_all(&session_dir).unwrap().collect();
    assert_eq!(read.len(), 2);
    assert!(read[0].as_ref().is_ok());
    assert!(read[1].as_ref().is_ok());
    let ev1 = read[0].as_ref().unwrap();
    let ev2 = read[1].as_ref().unwrap();
    assert_eq!(ev1.seq, 1);
    assert_eq!(ev1.kind, "context.pack_built");
    assert_eq!(ev2.seq, 2);
    assert_eq!(ev2.kind, "policy.evaluated");
}

#[test]
fn test_next_seq_monotonic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = NdjsonSessionEventStore::new(Arc::clone(&fs));

    assert_eq!(store.next_seq(&session_dir).unwrap(), 1);
    store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: 1,
                ts_ms: 0,
                session_id: "s".to_string(),
                run_id: None,
                kind: "test".to_string(),
                payload: serde_json::json!(null),
            },
        )
        .unwrap();
    assert_eq!(store.next_seq(&session_dir).unwrap(), 2);
    store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: 2,
                ts_ms: 0,
                session_id: "s".to_string(),
                run_id: None,
                kind: "test".to_string(),
                payload: serde_json::json!(null),
            },
        )
        .unwrap();
    assert_eq!(store.next_seq(&session_dir).unwrap(), 3);
}

#[test]
fn test_append_seq_zero_returns_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = NdjsonSessionEventStore::new(Arc::clone(&fs));

    let err = store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: 0,
                ts_ms: 0,
                session_id: "s".to_string(),
                run_id: None,
                kind: "test".to_string(),
                payload: serde_json::json!(null),
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("seq"));
}

#[test]
fn test_read_all_yields_err_on_corrupted_line() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_dir = SessionDir::new(tmp.path());
    let events_dir = tmp.path().join("events");
    std::fs::create_dir_all(&events_dir).unwrap();
    let path = events_dir.join("events.ndjson");
    std::fs::write(
        &path,
        r#"{"v":1,"seq":1,"ts_ms":0,"session_id":"s","run_id":null,"kind":"ok","payload":null}
not valid json
"#,
    )
    .unwrap();

    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = NdjsonSessionEventStore::new(Arc::clone(&fs));
    let read: Vec<_> = store.read_all(&session_dir).unwrap().collect();
    assert_eq!(read.len(), 2);
    assert!(read[0].as_ref().is_ok());
    assert!(read[1].as_ref().is_err());
}
