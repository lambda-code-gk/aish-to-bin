//! shell attachment と console log まわりの状態を集約して返す

use crate::domain::{JobListEntry, ShellStatusSnapshot, ShellStorageLayout};
use crate::ports::outbound::ShellAttachmentStore;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::{FileSystem, PathResolver, PathResolverInput, SessionEventStore};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ShellStatusUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    attachment_store: Arc<dyn ShellAttachmentStore>,
    event_store: Arc<dyn SessionEventStore>,
}

impl ShellStatusUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        attachment_store: Arc<dyn ShellAttachmentStore>,
        event_store: Arc<dyn SessionEventStore>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            attachment_store,
            event_store,
        }
    }

    pub fn get(&self, path_input: &PathResolverInput) -> Result<ShellStatusSnapshot, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_dir = PathBuf::from(
            self.path_resolver
                .resolve_session_dir(path_input, &home_dir)?,
        );
        let attachment = self.attachment_store.load(&session_dir)?;
        let storage = attachment
            .as_ref()
            .map(|attachment| ShellStorageLayout {
                console_path: attachment.console_path.clone(),
                pending_input_path: attachment.pending_input_path.clone(),
                prompt_suggestion_path: attachment.prompt_suggestion_path.clone(),
                mute_flag_path: attachment.mute_flag_path.clone(),
                part_file_prefix: attachment.part_file_prefix.clone(),
            })
            .unwrap_or_default();
        let console_path = storage.console_file(&session_dir);
        let prompt_suggestion_path = storage.prompt_suggestion_file(&session_dir);
        let pending_input_path = storage.pending_input_file(&session_dir);
        let mute_flag_path = storage.mute_flag_file(&session_dir);
        let console_bytes = self.fs.metadata(&console_path).ok().map(|meta| meta.len());
        let (part_file_count, latest_part_file) =
            self.load_part_file_summary(&session_dir, &storage)?;
        let session_id = session_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let (persisted_job_count, latest_job) =
            self.load_job_summary(&SessionDir::new(session_dir.clone()))?;
        Ok(ShellStatusSnapshot {
            session_id,
            attachment,
            console_exists: self.fs.exists(&console_path),
            console_bytes,
            part_file_count,
            latest_part_file,
            mute_flag_exists: self.fs.exists(&mute_flag_path),
            pending_input_exists: self.fs.exists(&pending_input_path),
            prompt_suggestion_exists: self.fs.exists(&prompt_suggestion_path),
            persisted_job_count,
            latest_job,
        })
    }

    fn load_part_file_summary(
        &self,
        session_dir: &PathBuf,
        storage: &ShellStorageLayout,
    ) -> Result<(usize, Option<String>), Error> {
        let mut part_names = self
            .fs
            .read_dir(session_dir)?
            .into_iter()
            .filter_map(|path| {
                let file_name = path.file_name()?.to_str()?;
                if !storage.is_part_file_name(file_name) {
                    return None;
                }
                let is_file = self
                    .fs
                    .metadata(&path)
                    .ok()
                    .map(|meta| meta.is_file())
                    .unwrap_or(false);
                if is_file {
                    Some(file_name.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        part_names.sort();
        let latest_part_file = part_names.last().cloned();
        Ok((part_names.len(), latest_part_file))
    }

    fn load_job_summary(
        &self,
        session_dir: &SessionDir,
    ) -> Result<(usize, Option<JobListEntry>), Error> {
        #[derive(serde::Deserialize)]
        struct JobLifecyclePayload {
            job_id: String,
            #[serde(default)]
            parent_job_id: Option<String>,
            state: String,
            #[serde(default)]
            exit_code: Option<i32>,
        }

        let mut latest_by_job = std::collections::BTreeMap::<String, JobListEntry>::new();
        let mut latest_seen = None;
        for event in self.event_store.read_all(session_dir)? {
            let event = event?;
            if event.kind != "job.lifecycle" {
                continue;
            }
            let payload: JobLifecyclePayload = serde_json::from_value(event.payload)
                .map_err(|e| Error::json(format!("job.lifecycle payload parse failed: {}", e)))?;
            let entry = JobListEntry {
                job_id: payload.job_id.clone(),
                parent_job_id: payload.parent_job_id,
                state: payload.state,
                exit_code: payload.exit_code,
            };
            latest_by_job.insert(payload.job_id, entry.clone());
            latest_seen = Some(entry);
        }
        Ok((latest_by_job.len(), latest_seen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ShellAttachment, ShellAttachmentStatus, ShellJobLinkMode, ShellPartTrackingMode,
    };
    use crate::ports::outbound::ShellAttachmentStore;
    use common::adapter::{StdEnvResolver, StdFileSystem, StdPathResolver};
    use common::ports::outbound::{EnvResolver, FileSystem, PathResolver, SessionEventStore};
    use std::path::Path;
    use std::sync::Mutex;
    use storage::NdjsonSessionEventStore;

    struct TestAttachmentStore {
        attachment: Mutex<Option<ShellAttachment>>,
    }

    impl TestAttachmentStore {
        fn new(attachment: Option<ShellAttachment>) -> Self {
            Self {
                attachment: Mutex::new(attachment),
            }
        }
    }

    impl ShellAttachmentStore for TestAttachmentStore {
        fn load(&self, _session_dir: &Path) -> Result<Option<ShellAttachment>, Error> {
            Ok(self.attachment.lock().expect("lock poisoned").clone())
        }

        fn mark_attached(&self, _session_dir: &Path, _pid: u32, _muted: bool) -> Result<(), Error> {
            Ok(())
        }

        fn mark_detached(&self, _session_dir: &Path) -> Result<(), Error> {
            Ok(())
        }

        fn set_muted(&self, _session_dir: &Path, _muted: bool) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn get_returns_shell_attachment_and_console_artifacts() {
        let tmp = std::path::Path::new("/tmp").join(format!(
            "aish_shell_status_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home_dir = tmp.join("home");
        let session_dir = tmp.join("session");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("session_schema_version"), "2\n").unwrap();
        std::fs::write(session_dir.join("console.txt"), "hello").unwrap();
        std::fs::write(session_dir.join("part_001_user.txt"), "older").unwrap();
        std::fs::write(session_dir.join("part_002_user.txt"), "newer").unwrap();
        std::fs::write(session_dir.join("console.muted"), "").unwrap();
        std::fs::write(session_dir.join("pending_input.json"), "{}").unwrap();
        std::fs::write(session_dir.join("prompt_suggestion.txt"), "suggest").unwrap();
        std::fs::write(
            session_dir.join("events.jsonl"),
            "{\"v\":1,\"seq\":1,\"ts_ms\":1,\"session_id\":\"session\",\"run_id\":\"job-1\",\"kind\":\"job.lifecycle\",\"payload\":{\"job_id\":\"job-1\",\"parent_job_id\":null,\"state\":\"completed\",\"exit_code\":0}}\n",
        )
        .unwrap();

        let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let path_resolver: Arc<dyn PathResolver> =
            Arc::new(StdPathResolver::new(Arc::clone(&env_resolver)));
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let event_store: Arc<dyn SessionEventStore> =
            Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
        let attachment_store = Arc::new(TestAttachmentStore::new(Some(ShellAttachment::attached(
            42, true, "test-now",
        ))));
        let usecase = ShellStatusUseCase::new(path_resolver, fs, attachment_store, event_store);

        let snapshot = usecase
            .get(&PathResolverInput {
                home_dir: Some(home_dir.display().to_string()),
                session_dir: Some(session_dir.display().to_string()),
            })
            .unwrap();

        assert_eq!(
            snapshot.attachment,
            Some(ShellAttachment {
                v: 1,
                status: ShellAttachmentStatus::Attached,
                pid: Some(42),
                attached_at: Some("test-now".to_string()),
                detached_at: None,
                updated_at: "test-now".to_string(),
                muted: true,
                console_path: "console.txt".to_string(),
                pending_input_path: "pending_input.json".to_string(),
                prompt_suggestion_path: "prompt_suggestion.txt".to_string(),
                mute_flag_path: "console.muted".to_string(),
                part_file_prefix: "part_".to_string(),
                job_link_mode: ShellJobLinkMode::SessionEvents,
                part_file_tracking_mode: ShellPartTrackingMode::LayoutOnly,
            })
        );
        assert!(snapshot.console_exists);
        assert_eq!(snapshot.console_bytes, Some(5));
        assert_eq!(snapshot.part_file_count, 2);
        assert_eq!(
            snapshot.latest_part_file,
            Some("part_002_user.txt".to_string())
        );
        assert!(snapshot.mute_flag_exists);
        assert!(snapshot.pending_input_exists);
        assert!(snapshot.prompt_suggestion_exists);
        assert_eq!(snapshot.session_id, "session");
        assert_eq!(snapshot.persisted_job_count, 1);
        assert_eq!(
            snapshot.latest_job,
            Some(JobListEntry {
                job_id: "job-1".to_string(),
                parent_job_id: None,
                state: "completed".to_string(),
                exit_code: Some(0),
            })
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
