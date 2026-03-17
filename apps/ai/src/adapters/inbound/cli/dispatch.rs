//! 責務: ai frontend の backend 利用判定と invocation dispatch を扱う。

use std::ffi::OsString;

use common::error::Error;

use crate::cli::{config_to_command, parse_args_from_os, ParseOutcome};
use crate::domain::AiCommand;

fn command_requires_backend(cmd: &AiCommand) -> bool {
    // LocalOnly: help (and completion/list-* are handled via ParseOutcome on the frontend).
    // BackendRequired: all commands that carry ai application semantics (LLM-backed behavior).
    !matches!(cmd, AiCommand::Help)
}

pub(crate) fn run_with_invocation_args(
    args: Vec<OsString>,
    fallback: impl FnOnce(&ParseOutcome) -> Result<i32, Error>,
) -> Result<i32, Error> {
    let outcome = parse_args_from_os(args.clone())?;
    match &outcome {
        // AI application commands: always go through the backend.
        ParseOutcome::Config(c) => {
            let cmd = config_to_command(c.clone());
            if command_requires_backend(&cmd) {
                let request = crate::adapter::backend_client::build_run_request(&args)?;
                crate::adapter::backend_client::run_via_backend(request, c.verbose)
            } else {
                // LocalOnly: handled entirely on the frontend.
                fallback(&outcome)
            }
        }
        // Non-config outcomes (e.g. completion generation) are always local-only.
        _ => fallback(&outcome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Config;

    #[test]
    fn command_requires_backend_for_ai_commands() {
        // ListProfiles, ListTools, PolicyExplain, ConfigExplain, SessionsRebuildDerived, Task,
        // Resume, Query: all must be backend-required.
        let config = Config {
            list_profiles: true,
            ..Config::default()
        };
        let cmd = crate::cli::config_to_command(config);
        assert!(command_requires_backend(&cmd));
    }

    #[test]
    fn help_is_local_only() {
        let config = Config {
            help: true,
            ..Config::default()
        };
        let cmd = crate::cli::config_to_command(config);
        assert!(!command_requires_backend(&cmd));
    }
}

