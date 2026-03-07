//! セッション repository・event log・reviewed history・state の標準アダプタ

pub(crate) mod agent_state_storage;
pub(crate) mod compactor_deterministic;
pub(crate) mod daemon_event_appender;
pub(crate) mod fallback_event_appender;
pub(crate) mod manifest_reviewed_session_storage;
pub(crate) mod part_session_storage;
pub(crate) mod reviewed_session_storage;
pub(crate) mod session_derived_builder;
pub(crate) mod session_event_store_ndjson;
pub(crate) mod session_manifest;

pub(crate) use agent_state_storage::FileAgentStateStorage;
pub(crate) use compactor_deterministic::DeterministicCompactionStrategy;
pub(crate) use daemon_event_appender::DaemonEventAppender;
pub(crate) use fallback_event_appender::FallbackEventAppender;
pub(crate) use manifest_reviewed_session_storage::{
    ManifestReviewedSessionStorage, ManifestTailCompactionViewStrategy, ReviewedTailViewStrategy,
};
pub(crate) use part_session_storage::PartSessionStorage;
#[allow(unused_imports)] // テストで crate::adapter::session::ReviewedSessionStorage として参照
pub(crate) use reviewed_session_storage::ReviewedSessionStorage;
pub(crate) use session_derived_builder::StdSessionDerivedBuilder;
