//! TruncateConsoleLog コマンドのユースケース

use crate::domain::ShellAttachmentStatus;
use crate::ports::outbound::ShellAttachmentStore;
use common::error::Error;
use common::ports::outbound::FileSystem;
use common::ports::outbound::{PathResolver, PathResolverInput, Signal};
use common::session::Session;
use std::path::Path;
use std::sync::Arc;

/// TruncateConsoleLog コマンドのユースケース
pub struct TruncateConsoleLogUseCase {
    path_resolver: Arc<dyn PathResolver>,
    fs: Arc<dyn FileSystem>,
    signal: Arc<dyn Signal>,
    attachment_store: Arc<dyn ShellAttachmentStore>,
}

impl TruncateConsoleLogUseCase {
    pub fn new(
        path_resolver: Arc<dyn PathResolver>,
        fs: Arc<dyn FileSystem>,
        signal: Arc<dyn Signal>,
        attachment_store: Arc<dyn ShellAttachmentStore>,
    ) -> Self {
        Self {
            path_resolver,
            fs,
            signal,
            attachment_store,
        }
    }

    /// TruncateConsoleLog を実行する
    pub fn run(&self, path_input: &PathResolverInput) -> Result<i32, Error> {
        let session = self.resolve_session(path_input)?;
        self.truncate_console_log(session.session_dir().as_ref())
    }

    fn resolve_session(&self, path_input: &PathResolverInput) -> Result<Session, Error> {
        let home_dir = self.path_resolver.resolve_home_dir(path_input)?;
        let session_path = self
            .path_resolver
            .resolve_session_dir(path_input, &home_dir)?;
        Session::new(&session_path, &home_dir)
    }

    fn truncate_console_log(&self, session_dir: &Path) -> Result<i32, Error> {
        let Some(pid) = self.resolve_attached_pid(session_dir)? else {
            return Ok(0);
        };

        self.signal.send_signal(pid, libc::SIGUSR2)?;
        Ok(0)
    }

    fn resolve_attached_pid(&self, session_dir: &Path) -> Result<Option<i32>, Error> {
        if let Some(attachment) = self.attachment_store.load(session_dir)? {
            return Ok(match (attachment.status, attachment.pid) {
                (ShellAttachmentStatus::Attached, Some(pid)) => Some(pid as i32),
                _ => None,
            });
        }

        let pid_file_path = session_dir.join("AISH_PID");
        if !self.fs.exists(&pid_file_path) {
            return Ok(None);
        }
        let pid_str = self.fs.read_to_string(&pid_file_path)?;
        let pid: i32 = pid_str
            .trim()
            .parse()
            .map_err(|e| Error::io_msg(format!("Invalid PID in AISH_PID file: {}", e)))?;
        Ok(Some(pid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ShellAttachment;
    use common::adapter::StdFileSystem;
    use common::ports::outbound::Signal as SignalPort;
    use std::sync::{Arc, Mutex};

    struct TestPathResolver;

    impl PathResolver for TestPathResolver {
        fn resolve_home_dir(&self, input: &PathResolverInput) -> Result<String, Error> {
            input
                .home_dir
                .clone()
                .ok_or_else(|| Error::invalid_argument("home_dir is required in test".to_string()))
        }

        fn resolve_session_dir(
            &self,
            input: &PathResolverInput,
            _home_dir: &str,
        ) -> Result<String, Error> {
            input.session_dir.clone().ok_or_else(|| {
                Error::invalid_argument("session_dir is required in test".to_string())
            })
        }
    }

    struct RecordingSignal {
        sent: Mutex<Vec<(i32, i32)>>,
    }

    impl RecordingSignal {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
            }
        }
    }

    impl SignalPort for RecordingSignal {
        fn setup_sigwinch(&self) -> Result<(), Error> {
            Ok(())
        }
        fn setup_sigusr1(&self) -> Result<(), Error> {
            Ok(())
        }
        fn setup_sigusr2(&self) -> Result<(), Error> {
            Ok(())
        }
        fn check_sigwinch(&self) -> bool {
            false
        }
        fn check_sigusr1(&self) -> bool {
            false
        }
        fn check_sigusr2(&self) -> bool {
            false
        }
        fn send_signal(&self, pid: i32, sig: i32) -> Result<(), Error> {
            self.sent.lock().expect("lock poisoned").push((pid, sig));
            Ok(())
        }
    }

    struct TestAttachmentStore {
        attachment: Mutex<Option<ShellAttachment>>,
    }

    impl TestAttachmentStore {
        fn attached(pid: u32) -> Self {
            Self {
                attachment: Mutex::new(Some(ShellAttachment::attached(pid, false, "test-now"))),
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
    fn truncate_sends_sigusr2_when_shell_is_attached() {
        let temp_dir = std::path::PathBuf::from("/tmp").join("aish_test_truncate_attached");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        let path_resolver: Arc<dyn PathResolver> = Arc::new(TestPathResolver);
        let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
        let signal = Arc::new(RecordingSignal::new());
        let usecase = TruncateConsoleLogUseCase::new(
            Arc::clone(&path_resolver),
            Arc::clone(&fs),
            Arc::clone(&signal) as Arc<dyn SignalPort>,
            Arc::new(TestAttachmentStore::attached(654)),
        );

        let input = PathResolverInput {
            home_dir: Some(temp_dir.to_string_lossy().to_string()),
            session_dir: Some(temp_dir.to_string_lossy().to_string()),
        };
        usecase.run(&input).unwrap();

        assert_eq!(
            signal.sent.lock().expect("lock poisoned").as_slice(),
            &[(654, libc::SIGUSR2)]
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
