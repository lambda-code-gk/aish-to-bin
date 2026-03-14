//! 責務: session dir 配下の shell attachment metadata を JSON として保存・更新する。

use crate::domain::{ShellAttachment, SHELL_ATTACHMENT_FILENAME};
use crate::ports::outbound::ShellAttachmentStore;
use common::error::Error;
use common::ports::outbound::{now_iso8601, FileSystem};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct StdShellAttachmentStore {
    fs: Arc<dyn FileSystem>,
}

impl StdShellAttachmentStore {
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self { fs }
    }

    fn attachment_path(session_dir: &Path) -> PathBuf {
        session_dir.join(SHELL_ATTACHMENT_FILENAME)
    }

    fn save(&self, session_dir: &Path, attachment: &ShellAttachment) -> Result<(), Error> {
        let body = serde_json::to_string_pretty(attachment)
            .map_err(|e| Error::json(format!("failed to serialize shell attachment: {}", e)))?;
        self.fs.write(&Self::attachment_path(session_dir), &body)
    }
}

impl ShellAttachmentStore for StdShellAttachmentStore {
    fn load(&self, session_dir: &Path) -> Result<Option<ShellAttachment>, Error> {
        let path = Self::attachment_path(session_dir);
        if !self.fs.exists(&path) {
            return Ok(None);
        }
        let body = self.fs.read_to_string(&path)?;
        let attachment = serde_json::from_str(&body)
            .map_err(|e| Error::json(format!("failed to parse shell attachment: {}", e)))?;
        Ok(Some(attachment))
    }

    fn mark_attached(&self, session_dir: &Path, pid: u32, muted: bool) -> Result<(), Error> {
        let now = now_iso8601();
        let attachment = match self.load(session_dir)? {
            Some(mut existing) => {
                existing.status = crate::domain::ShellAttachmentStatus::Attached;
                existing.pid = Some(pid);
                if existing.attached_at.is_none() {
                    existing.attached_at = Some(now.clone());
                }
                existing.detached_at = None;
                existing.muted = muted;
                existing.updated_at = now.clone();
                existing
            }
            None => ShellAttachment::attached(pid, muted, &now),
        };
        self.save(session_dir, &attachment)
    }

    fn mark_detached(&self, session_dir: &Path) -> Result<(), Error> {
        let now = now_iso8601();
        let existing = self.load(session_dir)?;
        self.save(session_dir, &ShellAttachment::detached_from(existing, &now))
    }

    fn set_muted(&self, session_dir: &Path, muted: bool) -> Result<(), Error> {
        let now = now_iso8601();
        if let Some(existing) = self.load(session_dir)? {
            self.save(session_dir, &existing.with_muted(muted, &now))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ShellAttachmentStatus;
    use common::adapter::StdFileSystem;

    #[test]
    fn marks_attached_and_detached() {
        let tmp = std::path::PathBuf::from("/tmp").join("aish_test_shell_attachment_store_state");
        if tmp.exists() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        std::fs::create_dir_all(&tmp).unwrap();
        let store = StdShellAttachmentStore::new(Arc::new(StdFileSystem));

        store.mark_attached(&tmp, 1234, false).unwrap();
        let attached = store.load(&tmp).unwrap().unwrap();
        assert_eq!(attached.status, ShellAttachmentStatus::Attached);
        assert_eq!(attached.pid, Some(1234));
        assert!(!attached.muted);

        store.mark_detached(&tmp).unwrap();
        let detached = store.load(&tmp).unwrap().unwrap();
        assert_eq!(detached.status, ShellAttachmentStatus::Detached);
        assert_eq!(detached.pid, None);
        assert!(detached.detached_at.is_some());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn set_muted_updates_existing_attachment() {
        let tmp = std::path::PathBuf::from("/tmp").join("aish_test_shell_attachment_store_mute");
        if tmp.exists() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        std::fs::create_dir_all(&tmp).unwrap();
        let store = StdShellAttachmentStore::new(Arc::new(StdFileSystem));

        store.mark_attached(&tmp, 42, false).unwrap();
        store.set_muted(&tmp, true).unwrap();

        let attachment = store.load(&tmp).unwrap().unwrap();
        assert!(attachment.muted);
        assert_eq!(attachment.pid, Some(42));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
