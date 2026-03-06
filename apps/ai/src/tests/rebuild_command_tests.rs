//! sessions rebuild-derived ユースケースのテスト

use crate::adapter::StdSessionDerivedBuilder;
use crate::domain::EventEnvelope;
use crate::ports::outbound::{SessionDerivedBuilder, SessionEventStore};
use crate::usecase::session_usecase::SessionUseCase;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use common::ports::outbound::FileSystem;
use std::path::Path;
use std::sync::Arc;
use storage::{DerivedApplier, NdjsonSessionEventStore};

fn write_session_schema_version<P: AsRef<Path>>(session_path: P) {
    let version_path = session_path.as_ref().join("session_schema_version");
    std::fs::write(version_path, "2\n").unwrap();
}

#[test]
fn test_rebuild_derived_success() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_session_schema_version(tmp.path());
    let session_dir = SessionDir::new(tmp.path());
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);

    let store = Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
    let session_id = session_dir
        .as_ref()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    store
        .append(
            &session_dir,
            &EventEnvelope {
                v: 1,
                seq: 1,
                ts_ms: 1000,
                session_id,
                run_id: None,
                kind: "test".to_string(),
                payload: serde_json::json!(null),
            },
        )
        .unwrap();

    let derived_applier = Arc::new(DerivedApplier::new(
        Arc::clone(&store) as Arc<dyn common::ports::outbound::SessionEventStore>,
        Arc::clone(&fs),
    ));
    let builder = StdSessionDerivedBuilder::new(derived_applier);
    let use_case = SessionUseCase::new(
        store as Arc<dyn SessionEventStore>,
        Arc::new(builder) as Arc<dyn SessionDerivedBuilder>,
    );

    use_case.rebuild_derived(&session_dir).unwrap();

    let summary_path = session_dir.as_ref().join("snapshots").join("summary.json");
    assert!(summary_path.exists());
    let index_path = session_dir.as_ref().join("index").join("index.sqlite");
    assert!(index_path.exists());
}
