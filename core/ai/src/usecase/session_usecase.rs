//! セッション派生物の再生成など、セッションまわりのユースケース

use crate::ports::outbound::{SessionDerivedBuilder, SessionEventStore};
use common::domain::SessionDir;
use common::error::Error;
use std::sync::Arc;

pub struct SessionUseCase {
    session_event_store: Arc<dyn SessionEventStore>,
    session_derived_builder: Arc<dyn SessionDerivedBuilder>,
}

impl SessionUseCase {
    pub fn new(
        session_event_store: Arc<dyn SessionEventStore>,
        session_derived_builder: Arc<dyn SessionDerivedBuilder>,
    ) -> Self {
        Self {
            session_event_store,
            session_derived_builder,
        }
    }

    /// events.jsonl から index.sqlite と snapshots/summary.json を再生成する
    pub fn rebuild_derived(&self, session_dir: &SessionDir) -> Result<(), Error> {
        let mut iter = self.session_event_store.read_all(session_dir)?;
        self.session_derived_builder.rebuild(session_dir, &mut iter)
    }
}
