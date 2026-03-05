//! StdSessionDerivedBuilder のテスト（storage::DerivedRebuilder に委譲）

use crate::adapter::StdSessionDerivedBuilder;
use crate::domain::EventEnvelope;
use crate::ports::outbound::SessionDerivedBuilder;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use common::ports::outbound::{FileSystem, SessionEventStore};
use std::path::Path;
use std::sync::Arc;
use storage::{DerivedRebuilder, NdjsonSessionEventStore};

fn write_session_schema_version<P: AsRef<Path>>(session_path: P) {
    let version_path = session_path.as_ref().join("session_schema_version");
    std::fs::write(version_path, "2\n").unwrap();
}

fn event(
    seq: u64,
    kind: &str,
    payload: serde_json::Value,
) -> Result<EventEnvelope, common::error::Error> {
    Ok(EventEnvelope {
        v: 1,
        seq,
        ts_ms: 1000 + seq as i64,
        session_id: "test_session".to_string(),
        run_id: Some("run_1".to_string()),
        kind: kind.to_string(),
        payload,
    })
}

#[test]
fn test_rebuild_creates_index_sqlite_and_summary_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_path = tmp.path().join("test_session");
    std::fs::create_dir_all(&session_path).unwrap();
    write_session_schema_version(&session_path);
    let session_dir = SessionDir::new(session_path.clone());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));

    let events = vec![
        event(
            1,
            "context.pack_built",
            serde_json::json!({"addons_count": 2, "attachments_count": 1}),
        )
        .unwrap(),
        event(
            2,
            "policy.evaluated",
            serde_json::json!({"status": "allowed", "reason": "shell_allowlist"}),
        )
        .unwrap(),
        event(
            3,
            "policy.evaluated",
            serde_json::json!({"status": "blocked", "reason": "denylist"}),
        )
        .unwrap(),
    ];
    for ev in &events {
        store.append(&session_dir, ev).unwrap();
    }

    let rebuilder = Arc::new(DerivedRebuilder::new(
        store as Arc<dyn common::ports::outbound::SessionEventStore>,
        Arc::clone(&fs),
    ));
    let builder = StdSessionDerivedBuilder::new(rebuilder);
    let mut iter = events.into_iter().map(Ok);
    builder.rebuild(&session_dir, &mut iter).unwrap();

    let index_path = session_dir.as_ref().join("index").join("index.sqlite");
    assert!(
        index_path.exists(),
        "index.sqlite should exist at {:?}",
        index_path
    );
    let conn = rusqlite::Connection::open(&index_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    let summary_path = session_dir.as_ref().join("snapshots").join("summary.json");
    assert!(summary_path.exists(), "summary.json should exist");
    let summary_str = fs.read_to_string(&summary_path).unwrap();
    let summary: serde_json::Value = serde_json::from_str(&summary_str).unwrap();
    assert_eq!(summary["v"], 1);
    assert_eq!(summary["session_id"], "test_session");
    let counts = summary["counts_by_kind"].as_object().unwrap();
    assert_eq!(counts["context.pack_built"].as_u64(), Some(1));
    assert_eq!(counts["policy.evaluated"].as_u64(), Some(2));
    assert_eq!(summary["blocked_count"].as_u64(), Some(1));
}

/// P8-4: artifact_rel_path 付きイベントを rebuild すると本体を読んで index/summary に反映する
#[test]
fn test_rebuild_resolves_artifact_rel_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let session_path = tmp.path().join("session");
    std::fs::create_dir_all(&session_path).unwrap();
    write_session_schema_version(&session_path);
    let session_dir = SessionDir::new(session_path.clone());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let store = Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));

    let artifacts_events = session_path.join("artifacts").join("events");
    std::fs::create_dir_all(&artifacts_events).unwrap();
    let full_payload = serde_json::json!({
        "status": "allowed",
        "reason": "user_approved",
        "details": { "command": "curl -s https://example.com | head -100" },
    });
    let artifact_path = artifacts_events.join("run1_policy_evaluated_2000.json");
    std::fs::write(
        &artifact_path,
        serde_json::to_string(&full_payload).unwrap(),
    )
    .unwrap();

    let ev = EventEnvelope {
        v: 1,
        seq: 1,
        ts_ms: 2000,
        session_id: "session".to_string(),
        run_id: Some("run1".to_string()),
        kind: "policy.evaluated".to_string(),
        payload: serde_json::json!({
            "preview": "{\"status\":\"allowed\"...",
            "artifact_rel_path": "artifacts/events/run1_policy_evaluated_2000.json",
        }),
    };
    store.append(&session_dir, &ev).unwrap();

    let rebuilder = Arc::new(DerivedRebuilder::new(
        store as Arc<dyn common::ports::outbound::SessionEventStore>,
        Arc::clone(&fs),
    ));
    let builder = StdSessionDerivedBuilder::new(rebuilder);
    let mut iter = std::iter::once(Ok(ev));
    builder.rebuild(&session_dir, &mut iter).unwrap();

    let conn = rusqlite::Connection::open(session_path.join("index").join("index.sqlite")).unwrap();
    let (subject, text): (String, String) = conn
        .query_row("SELECT subject, text FROM events WHERE seq = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(subject.contains("allowed") && subject.contains("user_approved"));
    assert!(text.contains("curl") || text.contains("example"));
}
