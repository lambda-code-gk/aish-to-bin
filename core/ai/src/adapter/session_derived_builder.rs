//! 派生物再生成: storage::DerivedRebuilder に委譲（index.sqlite + snapshots/summary.json）

use crate::ports::outbound::SessionDerivedBuilder;
use common::domain::SessionDir;
use common::error::Error;
use std::sync::Arc;
use storage::DerivedRebuilder;

/// storage::DerivedRebuilder に全再構築を委譲する SessionDerivedBuilder 実装
pub struct StdSessionDerivedBuilder {
    rebuilder: Arc<DerivedRebuilder>,
}

impl StdSessionDerivedBuilder {
    pub fn new(rebuilder: Arc<DerivedRebuilder>) -> Self {
        Self { rebuilder }
    }
}

impl SessionDerivedBuilder for StdSessionDerivedBuilder {
    fn rebuild(
        &self,
        session_dir: &SessionDir,
        _events: &mut dyn Iterator<Item = Result<crate::domain::EventEnvelope, Error>>,
    ) -> Result<(), Error> {
        self.rebuilder.rebuild_all(session_dir)
    }
}
