//! leakscan を用いた機微情報フィルタ（ContextAddon 用）

use crate::domain::{SensitiveAction, SensitiveFilterOutcome};
use crate::ports::outbound::SensitiveTextFilter;
use common::error::Error;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct LeakscanTextFilter {
    leakscan_binary: PathBuf,
    rules_path: PathBuf,
    action: SensitiveAction,
}

impl LeakscanTextFilter {
    pub fn new(leakscan_binary: PathBuf, rules_path: PathBuf, action: SensitiveAction) -> Self {
        Self {
            leakscan_binary,
            rules_path,
            action,
        }
    }

    fn check(&self, content: &str) -> Result<(bool, String), Error> {
        let rules = self.rules_path.to_string_lossy();
        let mut cmd = Command::new(&self.leakscan_binary);
        cmd.args(["-v", rules.as_ref()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| Error::io_msg(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(content.as_bytes()) {
                if e.kind() != ErrorKind::BrokenPipe {
                    return Err(Error::io_msg(e.to_string()));
                }
            }
        }
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .map_err(|e| Error::io_msg(e.to_string()))?;
        let status = child.wait().map_err(|e| Error::io_msg(e.to_string()))?;
        let hit = status.success() && !stdout.trim().is_empty();
        Ok((hit, stdout))
    }

    fn mask(&self, content: &str) -> Result<String, Error> {
        let rules = self.rules_path.to_string_lossy();
        let mut cmd = Command::new(&self.leakscan_binary);
        cmd.args(["--mask", rules.as_ref()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| Error::io_msg(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(content.as_bytes()) {
                if e.kind() != ErrorKind::BrokenPipe {
                    return Err(Error::io_msg(e.to_string()));
                }
            }
        }
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .map_err(|e| Error::io_msg(e.to_string()))?;
        let _ = child.wait();
        Ok(stdout)
    }
}

impl SensitiveTextFilter for LeakscanTextFilter {
    fn filter(&self, content: &str) -> Result<SensitiveFilterOutcome, Error> {
        let (hit, verbose) = self.check(content)?;
        if !hit {
            return Ok(SensitiveFilterOutcome::Clean);
        }
        match self.action {
            SensitiveAction::Allow => Ok(SensitiveFilterOutcome::Hit { verbose }),
            SensitiveAction::Deny => Ok(SensitiveFilterOutcome::Deny { verbose }),
            SensitiveAction::Mask => {
                let masked = self.mask(content)?;
                Ok(SensitiveFilterOutcome::Masked { masked, verbose })
            }
        }
    }
}
