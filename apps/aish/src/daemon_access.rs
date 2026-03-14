//! 責務: aish frontend の daemon 文脈生成と read/write fallback 制御を扱う。

use common::error::Error;
use common::ports::outbound::McpServerDescriptor;
use serde::de::DeserializeOwned;

use aish_daemon as daemon_client;

use crate::{cli, wiring};

pub(crate) fn backend_required_error(detail: impl Into<String>) -> Error {
    Error::invalid_argument(format!(
        "AISH backend is required for this aish command. Start it with 'aish daemon start'. {}",
        detail.into()
    ))
}

pub(crate) fn backend_context(config: &cli::Config) -> daemon_api::AishBackendContext {
    daemon_api::AishBackendContext {
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.display().to_string()),
        aish_home: std::env::var("AISH_HOME").ok(),
        aish_session: std::env::var("AISH_SESSION").ok(),
        home_dir: config.home_dir.clone(),
        session_dir: config.session_dir.clone(),
    }
}

fn try_aish_read<T>(
    request: daemon_api::AishReadRequest,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: DeserializeOwned,
{
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(
        |e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::other(e.to_string()))
        },
    )?;
    rt.block_on(daemon_client::run_aish_read(&path, request))
}

fn is_backend_unavailable(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<std::io::Error>().is_some_and(|io_err| {
        matches!(
            io_err.kind(),
            std::io::ErrorKind::NotFound
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::TimedOut
        )
    })
}

pub(crate) fn run_aish_read_with_fallback<T, F>(
    request: daemon_api::AishReadRequest,
    fallback: F,
) -> Result<T, Error>
where
    T: DeserializeOwned,
    F: FnOnce() -> Result<T, Error>,
{
    match try_aish_read(request) {
        Ok(value) => Ok(value),
        Err(err) if is_backend_unavailable(err.as_ref()) => fallback(),
        Err(err) => Err(backend_required_error(err.to_string())),
    }
}

fn try_aish_write(
    request: daemon_api::AishWriteRequest,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = daemon_client::default_socket_path();
    let rt = tokio::runtime::Runtime::new().map_err(
        |e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::other(e.to_string()))
        },
    )?;
    rt.block_on(daemon_client::run_aish_write(&path, request))
}

pub(crate) fn run_aish_write_with_fallback<F>(
    request: daemon_api::AishWriteRequest,
    fallback: F,
) -> Result<(), Error>
where
    F: FnOnce() -> Result<(), Error>,
{
    match try_aish_write(request) {
        Ok(()) => Ok(()),
        Err(err) if is_backend_unavailable(err.as_ref()) => fallback(),
        Err(err) => Err(backend_required_error(err.to_string())),
    }
}

pub(crate) fn list_tools_locally(
    app: &wiring::App,
) -> Result<daemon_api::AishToolsListResult, Error> {
    let mut tool_ids: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let servers = app.mcp_host.discover()?;
    for server in servers.into_iter().filter(|s| s.enabled) {
        let tools = match app.mcp_host.list_tools(&server.id) {
            Ok(tools) => tools,
            Err(e) => {
                warnings.push(format!(
                    "warning: failed to list tools for plugin '{}': {}",
                    server.id.0, e
                ));
                continue;
            }
        };
        for tool in tools {
            tool_ids.push(tool.id.0);
        }
    }
    tool_ids.sort();
    tool_ids.dedup();
    Ok(daemon_api::AishToolsListResult { tool_ids, warnings })
}

pub(crate) fn sort_plugins(mut list: Vec<McpServerDescriptor>) -> Vec<McpServerDescriptor> {
    list.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    list
}
