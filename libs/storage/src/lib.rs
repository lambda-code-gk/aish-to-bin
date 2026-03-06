// Phase 8 slot: SessionEventStore, DerivedBuilder. Single append entry point (P8-3).
// Phase 10: EventAppender（単一ライタ）のローカル実装もここで提供する。
// Phase 10.1: DerivedApplier / DerivedRebuilder（CLI と daemon 共有）

mod derived;
mod local_event_appender;
mod ndjson_session_event_store;

pub use derived::{read_last_applied_seq, AppliedRange, DerivedApplier, DerivedRebuilder};
pub use local_event_appender::LocalEventAppender;
pub use ndjson_session_event_store::NdjsonSessionEventStore;
