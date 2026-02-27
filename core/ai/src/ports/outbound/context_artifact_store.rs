//! コンテキスト添付ファイルを永続化する Outbound ポート

use crate::domain::ContextAttachment;
use common::domain::event::RunId;
use common::domain::SessionDir;
use common::error::Error;

/// 添付ファイルを session_dir/artifacts/context/<run_id>/ に保存し、
/// content を None に置換した参照版を返す
pub trait ContextArtifactStore: Send + Sync {
    fn store(
        &self,
        session_dir: &SessionDir,
        run_id: &RunId,
        attachments: &[ContextAttachment],
    ) -> Result<Vec<ContextAttachment>, Error>;
}
