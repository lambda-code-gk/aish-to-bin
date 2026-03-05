//! 派生物再生成: storage::DerivedApplier に委譲（index.sqlite + snapshots/summary.json）

use crate::ports::outbound::SessionDerivedBuilder;
use common::domain::SessionDir;
use common::error::Error;
use std::sync::Arc;
use storage::{read_last_applied_seq, DerivedApplier};

/// storage::DerivedApplier に増分適用を委譲する SessionDerivedBuilder 実装
pub struct StdSessionDerivedBuilder {
    applier: Arc<DerivedApplier>,
}

impl StdSessionDerivedBuilder {
    pub fn new(applier: Arc<DerivedApplier>) -> Self {
        Self { applier }
    }
}

impl SessionDerivedBuilder for StdSessionDerivedBuilder {
    fn rebuild(
        &self,
        session_dir: &SessionDir,
        _events: &mut dyn Iterator<Item = Result<crate::domain::EventEnvelope, Error>>,
    ) -> Result<(), Error> {
        // index.sqlite 内の metadata から最後に適用済みの seq を読み、それ以降のイベントだけを適用する。
        let last_applied = read_last_applied_seq(session_dir);
        let from_seq = last_applied.saturating_add(1).max(1);
        let _range = self.applier.apply_from_seq(session_dir, from_seq)?;
        Ok(())
    }
}
