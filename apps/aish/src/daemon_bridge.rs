//! 責務: aish wiring を使う daemon read/write handler を組み立てる。

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard, OnceLock};

use aish_daemon::ServerHandlers;
use common::error::Error as CommonError;
use common::ports::outbound::PathResolverInput;
use daemon_api::{AishBackendContext, AishReadRequest, AishWriteRequest};

pub(crate) trait DaemonRequestHandler: Send + Sync {
    fn run_read(&self, request: AishReadRequest) -> Result<serde_json::Value, CommonError>;
    fn run_write(&self, request: AishWriteRequest) -> Result<serde_json::Value, CommonError>;
}

fn process_lock() -> &'static StdMutex<()> {
    static LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| StdMutex::new(()))
}

struct ProcessContextGuard {
    _guard: StdMutexGuard<'static, ()>,
    prev_cwd: PathBuf,
    prev_aish_home: Option<String>,
    prev_aish_session: Option<String>,
}

impl ProcessContextGuard {
    fn apply(context: &AishBackendContext) -> Result<Self, CommonError> {
        let guard = process_lock()
            .lock()
            .map_err(|_| CommonError::system("aish backend process lock poisoned"))?;
        let prev_cwd = std::env::current_dir().map_err(|e| CommonError::io_msg(e.to_string()))?;
        let prev_aish_home = std::env::var("AISH_HOME").ok();
        let prev_aish_session = std::env::var("AISH_SESSION").ok();
        if let Some(ref cwd) = context.cwd {
            std::env::set_current_dir(cwd).map_err(|e| CommonError::io_msg(e.to_string()))?;
        }
        match &context.aish_home {
            Some(v) => std::env::set_var("AISH_HOME", v),
            None => std::env::remove_var("AISH_HOME"),
        }
        match &context.aish_session {
            Some(v) => std::env::set_var("AISH_SESSION", v),
            None => std::env::remove_var("AISH_SESSION"),
        }
        Ok(Self {
            _guard: guard,
            prev_cwd,
            prev_aish_home,
            prev_aish_session,
        })
    }
}

impl Drop for ProcessContextGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
        match &self.prev_aish_home {
            Some(v) => std::env::set_var("AISH_HOME", v),
            None => std::env::remove_var("AISH_HOME"),
        }
        match &self.prev_aish_session {
            Some(v) => std::env::set_var("AISH_SESSION", v),
            None => std::env::remove_var("AISH_SESSION"),
        }
    }
}

pub(crate) fn path_input_from_context(context: &AishBackendContext) -> PathResolverInput {
    PathResolverInput {
        home_dir: context.home_dir.clone(),
        session_dir: context.session_dir.clone(),
    }
}

pub(crate) fn run_read_with_handler(
    handler: &dyn DaemonRequestHandler,
    request: AishReadRequest,
) -> Result<serde_json::Value, CommonError> {
    let context = match &request {
        AishReadRequest::MemoryList { context }
        | AishReadRequest::MemoryGet { context, .. }
        | AishReadRequest::HistoryList { context, .. }
        | AishReadRequest::HistoryGet { context, .. }
        | AishReadRequest::PluginsList { context }
        | AishReadRequest::ToolsList { context } => context,
    };
    let _context_guard = ProcessContextGuard::apply(context)?;
    handler.run_read(request)
}

pub(crate) fn run_write_with_handler(
    handler: &dyn DaemonRequestHandler,
    request: AishWriteRequest,
) -> Result<serde_json::Value, CommonError> {
    let context = match &request {
        AishWriteRequest::MemoryRemove { context, .. } => context,
    };
    let _context_guard = ProcessContextGuard::apply(context)?;
    handler.run_write(request)
}

pub(crate) fn build_server_handlers(
    handler_factory: Arc<dyn Fn() -> Arc<dyn DaemonRequestHandler> + Send + Sync>,
) -> ServerHandlers {
    ServerHandlers {
        run_aish_read: Arc::new({
            let handler_factory = Arc::clone(&handler_factory);
            move |request| {
                let handler = handler_factory();
                let request: AishReadRequest = serde_json::from_value(request)
                    .map_err(|e| CommonError::json(e.to_string()))?;
                run_read_with_handler(handler.as_ref(), request)
            }
        }),
        run_aish_write: Arc::new(move |request| {
            let handler = handler_factory();
            let request: AishWriteRequest =
                serde_json::from_value(request).map_err(|e| CommonError::json(e.to_string()))?;
            run_write_with_handler(handler.as_ref(), request)
        }),
    }
}
