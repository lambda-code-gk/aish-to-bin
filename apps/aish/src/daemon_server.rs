//! 責務: aish daemon start から backend server runtime を起動する。

use aish_daemon::run_server as run_daemon_server;
use std::path::PathBuf;

pub async fn run_server(
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_daemon_server(socket_path, crate::wiring::wire_daemon_server_handlers()).await
}
