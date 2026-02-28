//! sessions rebuild-derived ユースケースのテスト

use crate::adapter::StdSessionDerivedBuilder;
use crate::domain::EventEnvelope;
use crate::ports::outbound::{SessionDerivedBuilder, SessionEventStore};
use crate::usecase::session_usecase::SessionUseCase;
use common::adapter::StdFileSystem;
use common::domain::SessionDir;
use common::ports::outbound::FileSystem;
use std::sync::Arc;
use storage::{DerivedRebuilder, NdjsonSessionEventStore};

#[test]
fn test_rebuild_derived_success() {
    let tmp = tempfile::tempdir().expect("tempdir");
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

    let derived_rebuilder = Arc::new(DerivedRebuilder::new(
        Arc::clone(&store) as Arc<dyn common::ports::outbound::SessionEventStore>,
        Arc::clone(&fs),
    ));
    let builder = StdSessionDerivedBuilder::new(derived_rebuilder);
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
