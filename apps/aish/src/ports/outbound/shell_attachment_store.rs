//! shell attachment metadata の永続化ポート

use crate::domain::ShellAttachment;
use common::error::Error;
use std::path::Path;

pub trait ShellAttachmentStore: Send + Sync {
    fn load(&self, session_dir: &Path) -> Result<Option<ShellAttachment>, Error>;
    fn mark_attached(&self, session_dir: &Path, pid: u32, muted: bool) -> Result<(), Error>;
    fn mark_detached(&self, session_dir: &Path) -> Result<(), Error>;
    fn set_muted(&self, session_dir: &Path, muted: bool) -> Result<(), Error>;
}
