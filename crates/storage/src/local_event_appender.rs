//! ローカル（in-proc）用 EventAppender 実装
//!
//! - NdjsonSessionEventStore を用いて seq 採番 + append を行う
//! - Phase10 では daemon 経由の EventAppender と差し替え可能な基準実装とする

use common::domain::{EventEnvelope, EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error;
use common::ports::outbound::{EventAppender, SessionEventStore};
use std::sync::Arc;

/// NdjsonSessionEventStore ベースのローカル append 実装
pub struct LocalEventAppender {
    store: Arc<dyn SessionEventStore>,
}

impl LocalEventAppender {
    pub fn new(store: Arc<dyn SessionEventStore>) -> Self {
        Self { store }
    }
}

impl EventAppender for LocalEventAppender {
    fn append(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Error> {
        let seq = self.store.next_seq(session_dir)?;
        let with_seq = EventEnvelope::from_without_seq(seq, envelope);
        self.store.append(session_dir, &with_seq)?;
        Ok(with_seq)
    }
}

