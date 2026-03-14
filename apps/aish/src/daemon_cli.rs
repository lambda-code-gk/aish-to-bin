//! 責務: aish frontend から daemon command と job 表示を扱う。

use common::domain::SessionDir;
use common::error::Error;
use common::ports::outbound::PathResolverInput;
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aish_daemon as daemon_client;

use crate::{cli, daemon_server, domain, wiring};

pub(crate) fn run_start() -> Result<i32, Error> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    rt.block_on(daemon_server::run_server(path)).map_err(
        |e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()),
    )?;
    Ok(0)
}

fn start_daemon_detached() -> Result<(), Error> {
    let exe = std::env::current_exe()
        .map_err(|e| Error::invalid_argument(format!("resolve current exe: {}", e)))?;
    Command::new(exe)
        .arg("daemon")
        .arg("start")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::invalid_argument(format!("spawn detached daemon: {}", e)))?;
    Ok(())
}

pub(crate) fn run_start_detached() -> Result<i32, Error> {
    start_daemon_detached()?;
    Ok(0)
}

pub(crate) fn run_ensure() -> Result<i32, Error> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    // already running
    let ok = rt
        .block_on(daemon_client::run_ping(&path))
        .map_err(|e: Box<dyn std::error::Error + Send + Sync>| {
            Error::invalid_argument(e.to_string())
        })?;
    if ok {
        return Ok(0);
    }
    // not responding: try to start in background and wait until it responds
    start_daemon_detached()?;
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let ok = rt
            .block_on(daemon_client::run_ping(&path))
            .map_err(|e: Box<dyn std::error::Error + Send + Sync>| {
                Error::invalid_argument(e.to_string())
            })?;
        if ok {
            return Ok(0);
        }
        if start.elapsed() >= timeout {
            return Err(Error::invalid_argument(
                "daemon ensure timed out waiting for daemon to become ready".to_string(),
            ));
        }
    }
}

pub(crate) fn run_ping() -> Result<i32, Error> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    let ok = rt.block_on(daemon_client::run_ping(&path)).map_err(
        |e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()),
    )?;
    Ok(if ok { 0 } else { 1 })
}

pub(crate) fn run_status() -> Result<i32, Error> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    rt.block_on(daemon_client::run_status(&path)).map_err(
        |e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()),
    )?;
    Ok(0)
}

pub(crate) fn run_stop() -> Result<i32, Error> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    rt.block_on(daemon_client::run_stop(&path)).map_err(
        |e: Box<dyn std::error::Error + Send + Sync>| Error::invalid_argument(e.to_string()),
    )?;
    Ok(0)
}

pub(crate) fn run_cancel(job_id: &str) -> Result<i32, Error> {
    if job_id.is_empty() {
        return Err(Error::invalid_argument(
            "daemon cancel requires a job_id".to_string(),
        ));
    }
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::invalid_argument(e.to_string()))?;
    let result = rt
        .block_on(daemon_client::run_cancel_ai(&path, job_id))
        .map_err(|e: Box<dyn std::error::Error + Send + Sync>| {
            Error::invalid_argument(e.to_string())
        })?;
    if result.found {
        if result.signaled {
            println!("cancel requested for {}", result.job_id);
            Ok(0)
        } else {
            eprintln!("job {} was already exiting", result.job_id);
            Ok(1)
        }
    } else {
        eprintln!("job {} not found", result.job_id);
        Ok(1)
    }
}

pub(crate) fn run_jobs(
    config: &cli::Config,
    app: &wiring::App,
    path_input: &PathResolverInput,
    active_only: bool,
    persisted_only: bool,
    session_explicitly_specified: bool,
) -> Result<i32, Error> {
    let show_active = active_only || !persisted_only;
    let show_persisted = persisted_only || !active_only;
    let active_entries = if show_active {
        try_active_jobs().ok()
    } else {
        None
    };
    let persisted_session_dir = if show_persisted && session_explicitly_specified {
        Some(app.jobs_use_case.resolve_session(path_input)?)
    } else {
        None
    };
    let persisted_entries = if show_persisted && session_explicitly_specified {
        Some(
            app.jobs_use_case
                .list(path_input, session_explicitly_specified)?,
        )
    } else {
        None
    };
    if active_entries.is_none() && persisted_entries.is_none() {
        return Err(Error::invalid_argument(if show_active && !show_persisted {
            "daemon jobs --active requires a running daemon".to_string()
        } else if show_persisted && !show_active {
            "daemon jobs --persisted requires an explicit session (-s/-d/AISH_SESSION)".to_string()
        } else {
            "daemon jobs requires a running daemon or an explicit session (-s/-d/AISH_SESSION)"
                .to_string()
        }));
    }
    let mut printed_any = false;
    if let Some(entries) = active_entries {
        let entries = filter_active_jobs(entries, persisted_session_dir.as_ref());
        if !entries.is_empty() {
            println!("[active jobs]");
            for entry in order_active_jobs(&entries) {
                print_job_line_active(entry, &entries);
            }
            printed_any = true;
        }
    }
    if let Some(entries) = persisted_entries {
        if printed_any && !entries.is_empty() {
            println!();
        }
        if !entries.is_empty() {
            println!("[persisted lifecycle]");
            for entry in order_persisted_jobs(&entries) {
                print_job_line_persisted(entry, &entries);
            }
            printed_any = true;
        }
    }
    if !printed_any {
        println!("no jobs");
    }
    let _ = config;
    Ok(0)
}

fn try_active_jobs(
) -> Result<Vec<daemon_api::ActiveJobInfo>, Box<dyn std::error::Error + Send + Sync>> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    rt.block_on(daemon_client::run_list_active_jobs(&path))
}

fn filter_active_jobs(
    entries: Vec<daemon_api::ActiveJobInfo>,
    session_dir: Option<&SessionDir>,
) -> Vec<daemon_api::ActiveJobInfo> {
    match session_dir {
        Some(session_dir) => {
            let session_dir = session_dir.display().to_string();
            entries
                .into_iter()
                .filter(|entry| entry.session_dir.as_deref() == Some(session_dir.as_str()))
                .collect()
        }
        None => entries,
    }
}

fn print_job_line_active(
    entry: &daemon_api::ActiveJobInfo,
    all_entries: &[daemon_api::ActiveJobInfo],
) {
    let label = job_label(&entry.job_id, entry.parent_job_id.as_deref(), |job_id| {
        all_entries
            .iter()
            .any(|candidate| candidate.job_id == job_id)
    });
    println!("{}\t{}", label, state_name(&entry.state));
}

fn print_job_line_persisted(entry: &domain::JobListEntry, all_entries: &[domain::JobListEntry]) {
    let label = job_label(&entry.job_id, entry.parent_job_id.as_deref(), |job_id| {
        all_entries
            .iter()
            .any(|candidate| candidate.job_id == job_id)
    });
    match entry.exit_code {
        Some(exit_code) => println!("{}\t{}\texit={}", label, entry.state, exit_code),
        None => println!("{}\t{}", label, entry.state),
    }
}

fn state_name(state: &daemon_api::AiJobState) -> &'static str {
    match state {
        daemon_api::AiJobState::Queued => "queued",
        daemon_api::AiJobState::Running => "running",
        daemon_api::AiJobState::WaitingInteraction => "waiting_interaction",
        daemon_api::AiJobState::Cancelled => "cancelled",
        daemon_api::AiJobState::Completed => "completed",
        daemon_api::AiJobState::Failed => "failed",
    }
}

fn order_active_jobs(entries: &[daemon_api::ActiveJobInfo]) -> Vec<&daemon_api::ActiveJobInfo> {
    let parent_of = entries
        .iter()
        .map(|entry| (entry.job_id.as_str(), entry.parent_job_id.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let mut by_parent: BTreeMap<Option<&str>, Vec<&daemon_api::ActiveJobInfo>> = BTreeMap::new();
    for entry in entries {
        let key = match entry.parent_job_id.as_deref() {
            Some(parent_job_id) if parent_of.contains_key(parent_job_id) => Some(parent_job_id),
            _ => None,
        };
        by_parent.entry(key).or_default().push(entry);
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    }
    let mut ordered = Vec::new();
    let mut visited = BTreeSet::new();
    collect_active_jobs(None, &by_parent, &mut visited, &mut ordered);
    ordered
}

fn collect_active_jobs<'a>(
    parent_job_id: Option<&'a str>,
    by_parent: &BTreeMap<Option<&'a str>, Vec<&'a daemon_api::ActiveJobInfo>>,
    visited: &mut BTreeSet<&'a str>,
    ordered: &mut Vec<&'a daemon_api::ActiveJobInfo>,
) {
    if let Some(children) = by_parent.get(&parent_job_id) {
        for entry in children {
            if !visited.insert(entry.job_id.as_str()) {
                continue;
            }
            ordered.push(*entry);
            collect_active_jobs(Some(entry.job_id.as_str()), by_parent, visited, ordered);
        }
    }
}

fn order_persisted_jobs(entries: &[domain::JobListEntry]) -> Vec<&domain::JobListEntry> {
    let parent_of = entries
        .iter()
        .map(|entry| (entry.job_id.as_str(), entry.parent_job_id.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let mut by_parent: BTreeMap<Option<&str>, Vec<&domain::JobListEntry>> = BTreeMap::new();
    for entry in entries {
        let key = match entry.parent_job_id.as_deref() {
            Some(parent_job_id) if parent_of.contains_key(parent_job_id) => Some(parent_job_id),
            _ => None,
        };
        by_parent.entry(key).or_default().push(entry);
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    }
    let mut ordered = Vec::new();
    let mut visited = BTreeSet::new();
    collect_persisted_jobs(None, &by_parent, &mut visited, &mut ordered);
    ordered
}

fn collect_persisted_jobs<'a>(
    parent_job_id: Option<&'a str>,
    by_parent: &BTreeMap<Option<&'a str>, Vec<&'a domain::JobListEntry>>,
    visited: &mut BTreeSet<&'a str>,
    ordered: &mut Vec<&'a domain::JobListEntry>,
) {
    if let Some(children) = by_parent.get(&parent_job_id) {
        for entry in children {
            if !visited.insert(entry.job_id.as_str()) {
                continue;
            }
            ordered.push(*entry);
            collect_persisted_jobs(Some(entry.job_id.as_str()), by_parent, visited, ordered);
        }
    }
}

fn job_label<F>(job_id: &str, parent_job_id: Option<&str>, has_job: F) -> String
where
    F: Fn(&str) -> bool,
{
    match parent_job_id {
        Some(parent_job_id) if has_job(parent_job_id) => {
            format!("child parent={}\t{}", parent_job_id, job_id)
        }
        Some(parent_job_id) => format!("orphan parent={}\t{}", parent_job_id, job_id),
        None => format!("root\t{}", job_id),
    }
}

#[cfg(test)]
mod tests {
    use super::{job_label, order_persisted_jobs};
    use crate::domain::JobListEntry;

    #[test]
    fn job_label_marks_children_and_orphans() {
        let entries = [
            JobListEntry {
                job_id: "job-root".to_string(),
                parent_job_id: None,
                state: "completed".to_string(),
                exit_code: Some(0),
            },
            JobListEntry {
                job_id: "job-child".to_string(),
                parent_job_id: Some("job-root".to_string()),
                state: "completed".to_string(),
                exit_code: Some(0),
            },
        ];
        let has_job = |job_id: &str| entries.iter().any(|entry| entry.job_id == job_id);
        assert_eq!(job_label("job-root", None, has_job), "root\tjob-root");
        assert_eq!(
            job_label("job-child", Some("job-root"), has_job),
            "child parent=job-root\tjob-child"
        );
        assert_eq!(
            job_label("job-orphan", Some("job-missing"), has_job),
            "orphan parent=job-missing\tjob-orphan"
        );
    }

    #[test]
    fn order_persisted_jobs_places_children_after_parent() {
        let entries = vec![
            JobListEntry {
                job_id: "job-child".to_string(),
                parent_job_id: Some("job-root".to_string()),
                state: "completed".to_string(),
                exit_code: Some(0),
            },
            JobListEntry {
                job_id: "job-root".to_string(),
                parent_job_id: None,
                state: "completed".to_string(),
                exit_code: Some(0),
            },
            JobListEntry {
                job_id: "job-orphan".to_string(),
                parent_job_id: Some("job-missing".to_string()),
                state: "failed".to_string(),
                exit_code: Some(1),
            },
        ];

        let ordered = order_persisted_jobs(&entries);
        let labels = ordered
            .iter()
            .map(|entry| entry.job_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["job-orphan", "job-root", "job-child"]);
    }
}
