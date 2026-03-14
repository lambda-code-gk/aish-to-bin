//! daemon jobs コマンド向けに persisted job.lifecycle を集約する

use crate::domain::JobListEntry;
use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::{PathResolver, PathResolverInput, SessionEventStore};
use common::session::Session;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
struct JobLifecyclePayload {
    job_id: String,
    #[serde(default)]
    parent_job_id: Option<String>,
    state: String,
    #[serde(default)]
    exit_code: Option<i32>,
}

pub struct JobsUseCase {
    path_resolver: Arc<dyn PathResolver>,
    store: Arc<dyn SessionEventStore>,
}

impl JobsUseCase {
    pub fn new(path_resolver: Arc<dyn PathResolver>, store: Arc<dyn SessionEventStore>) -> Self {
        Self {
            path_resolver,
            store,
        }
    }

    pub fn list(
        &self,
        path_input: &PathResolverInput,
        session_explicitly_specified: bool,
    ) -> Result<Vec<JobListEntry>, Error> {
        if !session_explicitly_specified {
            return Err(Error::invalid_argument(
                "The 'daemon jobs' command requires a session. Use -s/--session-dir, -d/--home-dir, or set AISH_SESSION.",
            ));
        }
        let session_dir = self.resolve_session(path_input)?;
        let mut latest: HashMap<String, JobListEntry> = HashMap::new();
        for event in self.store.read_all(&session_dir)? {
            let event = event?;
            if event.kind != "job.lifecycle" {
                continue;
            }
            let payload: JobLifecyclePayload = serde_json::from_value(event.payload)
                .map_err(|e| Error::json(format!("job.lifecycle payload parse failed: {}", e)))?;
            latest.insert(
                payload.job_id.clone(),
                JobListEntry {
                    job_id: payload.job_id,
                    parent_job_id: payload.parent_job_id,
                    state: payload.state,
                    exit_code: payload.exit_code,
                },
            );
        }
        let mut entries = latest.into_values().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.job_id.cmp(&b.job_id));
        Ok(entries)
    }

    pub fn resolve_session(&self, path_input: &PathResolverInput) -> Result<SessionDir, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        Ok(Session::new(&session_path, &home_dir)?
            .session_dir()
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::adapter::{StdEnvResolver, StdFileSystem, StdPathResolver};
    use common::ports::outbound::{EnvResolver, FileSystem, PathResolver, SessionEventStore};
    use std::fs;
    use std::sync::Arc;
    use storage::NdjsonSessionEventStore;

    #[test]
    fn list_returns_latest_state_per_job() {
        let tmp = std::path::Path::new("/tmp").join(format!(
            "aish_jobs_usecase_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home_dir = tmp.join("home");
        let session_dir = tmp.join("session");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("session_schema_version"), "2\n").unwrap();
        fs::write(
            session_dir.join("events.jsonl"),
            concat!(
                "{\"v\":1,\"seq\":1,\"ts_ms\":1,\"session_id\":\"session\",\"run_id\":\"job-1\",\"kind\":\"job.lifecycle\",\"payload\":{\"job_id\":\"job-1\",\"parent_job_id\":null,\"state\":\"queued\"}}\n",
                "{\"v\":1,\"seq\":2,\"ts_ms\":2,\"session_id\":\"session\",\"run_id\":\"job-1\",\"kind\":\"job.lifecycle\",\"payload\":{\"job_id\":\"job-1\",\"parent_job_id\":null,\"state\":\"completed\",\"exit_code\":0}}\n",
                "{\"v\":1,\"seq\":3,\"ts_ms\":3,\"session_id\":\"session\",\"run_id\":\"job-2\",\"kind\":\"job.lifecycle\",\"payload\":{\"job_id\":\"job-2\",\"parent_job_id\":\"job-1\",\"state\":\"running\"}}\n"
            ),
        )
        .unwrap();

        let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
        let path_resolver: Arc<dyn PathResolver> =
            Arc::new(StdPathResolver::new(Arc::clone(&env_resolver)));
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let store: Arc<dyn SessionEventStore> =
            Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
        let usecase = JobsUseCase::new(path_resolver, store);

        let entries = usecase
            .list(
                &PathResolverInput {
                    home_dir: Some(home_dir.display().to_string()),
                    session_dir: Some(session_dir.display().to_string()),
                },
                true,
            )
            .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            JobListEntry {
                job_id: "job-1".to_string(),
                parent_job_id: None,
                state: "completed".to_string(),
                exit_code: Some(0),
            }
        );
        assert_eq!(
            entries[1],
            JobListEntry {
                job_id: "job-2".to_string(),
                parent_job_id: Some("job-1".to_string()),
                state: "running".to_string(),
                exit_code: None,
            }
        );

        let _ = fs::remove_dir_all(&tmp);
    }
}
