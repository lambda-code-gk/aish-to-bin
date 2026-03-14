//! 標準サブプロセス実行（std::process::Command を委譲）

use crate::error::Error;
use crate::ports::outbound::{Process, ProcessOutputObserver, ProcessOutputStream};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

/// 標準ライブラリの Command を使う Process 実装
#[derive(Debug, Clone, Default)]
pub struct StdProcess;

impl Process for StdProcess {
    fn run(&self, program: &Path, args: &[String]) -> Result<i32, Error> {
        let mut command = Command::new(program);
        command.args(args);
        inherit_backend_lineage_env(&mut command);
        let status = command.status().map_err(|e| {
            Error::io_msg(format!("Failed to execute '{}': {}", program.display(), e))
        })?;
        Ok(status.code().unwrap_or(1))
    }

    fn run_observing(
        &self,
        program: &Path,
        args: &[String],
        observer: Option<Arc<dyn ProcessOutputObserver>>,
    ) -> Result<i32, Error> {
        let Some(observer) = observer else {
            return self.run(program, args);
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        inherit_backend_lineage_env(&mut command);
        let mut child = command.spawn().map_err(|e| {
            Error::io_msg(format!("Failed to execute '{}': {}", program.display(), e))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::io_msg("stdout not captured".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::io_msg("stderr not captured".to_string()))?;

        let stdout_observer = Arc::clone(&observer);
        let stdout_handle = thread::spawn(move || {
            read_stream(stdout, ProcessOutputStream::Stdout, stdout_observer)
        });
        let stderr_handle =
            thread::spawn(move || read_stream(stderr, ProcessOutputStream::Stderr, observer));

        let status = child.wait().map_err(|e| {
            Error::io_msg(format!("Failed to wait for '{}': {}", program.display(), e))
        })?;

        stdout_handle
            .join()
            .map_err(|_| Error::system("stdout observer thread panicked"))??;
        stderr_handle
            .join()
            .map_err(|_| Error::system("stderr observer thread panicked"))??;
        Ok(status.code().unwrap_or(1))
    }
}

fn inherit_backend_lineage_env(command: &mut Command) {
    for key in [
        "AISH_JOB_ID",
        "AISH_JOB_DEPTH",
        "AISH_MAX_BACKEND_JOB_DEPTH",
    ] {
        match std::env::var(key) {
            Ok(value) => {
                command.env(key, value);
            }
            Err(_) => {
                command.env_remove(key);
            }
        }
    }
}

fn read_stream<R: Read>(
    mut reader: R,
    stream: ProcessOutputStream,
    observer: Arc<dyn ProcessOutputObserver>,
) -> Result<(), Error> {
    let mut buf = [0u8; 4096];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::io_msg(format!("read child output: {}", e)))?;
        if n == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&buf[..n]);
        observer.on_output(stream, &text)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingObserver {
        chunks: Mutex<Vec<(ProcessOutputStream, String)>>,
    }

    impl ProcessOutputObserver for RecordingObserver {
        fn on_output(&self, stream: ProcessOutputStream, text: &str) -> Result<(), Error> {
            self.chunks
                .lock()
                .expect("recording observer mutex poisoned")
                .push((stream, text.to_string()));
            Ok(())
        }
    }

    #[test]
    fn run_observing_forwards_output() {
        let process = StdProcess;
        let observer = Arc::new(RecordingObserver::default());
        let args = vec![
            "-c".to_string(),
            "echo stdout-line; echo stderr-line >&2".to_string(),
        ];

        let code = process
            .run_observing(Path::new("/bin/sh"), &args, Some(observer.clone()))
            .expect("process should complete");

        assert_eq!(code, 0);
        let recorded = observer
            .chunks
            .lock()
            .expect("recording observer mutex poisoned")
            .clone();
        let combined = recorded
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<String>();
        assert!(
            combined.contains("stdout-line"),
            "expected observed output to contain stdout text, got: {combined:?}"
        );
        assert!(
            combined.contains("stderr-line"),
            "expected observed output to contain stderr text, got: {combined:?}"
        );
        assert!(
            recorded.iter().any(
                |(stream, text)| matches!(stream, ProcessOutputStream::Stdout)
                    && text.contains("stdout-line")
            ),
            "expected stdout chunk in observed output"
        );
        assert!(
            recorded.iter().any(
                |(stream, text)| matches!(stream, ProcessOutputStream::Stderr)
                    && text.contains("stderr-line")
            ),
            "expected stderr chunk in observed output"
        );
    }

    #[test]
    fn run_observing_propagates_backend_lineage_env() {
        std::env::set_var("AISH_JOB_ID", "job-123");
        std::env::set_var("AISH_JOB_DEPTH", "1");
        std::env::set_var("AISH_MAX_BACKEND_JOB_DEPTH", "2");
        let process = StdProcess;
        let observer = Arc::new(RecordingObserver::default());
        let args = vec![
            "-c".to_string(),
            "printf '%s|%s|%s' \"$AISH_JOB_ID\" \"$AISH_JOB_DEPTH\" \"$AISH_MAX_BACKEND_JOB_DEPTH\""
                .to_string(),
        ];

        let code = process
            .run_observing(Path::new("/bin/sh"), &args, Some(observer.clone()))
            .expect("process should complete");

        std::env::remove_var("AISH_JOB_ID");
        std::env::remove_var("AISH_JOB_DEPTH");
        std::env::remove_var("AISH_MAX_BACKEND_JOB_DEPTH");

        assert_eq!(code, 0);
        let combined = observer
            .chunks
            .lock()
            .expect("recording observer mutex poisoned")
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<String>();
        assert!(
            combined.contains("job-123|1|2"),
            "expected child process to inherit backend lineage env, got: {combined:?}"
        );
    }
}
