//! AISH_DAEMON=auto 用: daemon に接続できれば daemon、できなければ in-proc にフォールバック

use common::domain::{EventEnvelope, EventEnvelopeWithoutSeq, SessionDir};
use common::error::Error;
use common::ports::outbound::EventAppender;
use std::path::Path;
use std::sync::Arc;

/// daemon 接続を試し、失敗時は local にフォールバックする EventAppender
pub struct FallbackEventAppender {
    daemon: super::daemon_event_appender::DaemonEventAppender,
    local: Arc<dyn EventAppender>,
}

impl FallbackEventAppender {
    pub fn new(socket_path: impl AsRef<Path>, local: Arc<dyn EventAppender>) -> Self {
        let daemon = super::daemon_event_appender::DaemonEventAppender::new(socket_path);
        Self { daemon, local }
    }
}

impl EventAppender for FallbackEventAppender {
    fn append(
        &self,
        session_dir: &SessionDir,
        envelope: EventEnvelopeWithoutSeq,
    ) -> Result<EventEnvelope, Error> {
        match self.daemon.append(session_dir, envelope.clone()) {
            Ok(env) => Ok(env),
            Err(_) => self.local.append(session_dir, envelope),
        }
    }
}
