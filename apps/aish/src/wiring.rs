//! 配線: 標準アダプタで UseCase を組み立てる（Unix 専用）

use std::sync::Arc;

use common::adapter::{
    FileJsonLog, NoopLog, StdClock, StdEnvResolver, StdFileSystem, StdPathResolver,
    StdRuntimeCatalog,
};
use common::part_id::{IdGenerator, StdIdGenerator};
use common::ports::outbound::{
    EnvResolver, FileSystem, Log, McpHost, PathResolver, RuntimeCatalog, SessionEventStore, Signal,
};
use plugins::StdioJsonRpcMcpBridgeHost;
use storage::NdjsonSessionEventStore;

use crate::adapter::{
    LoggingMemoryRepository, StdMemoryRepository, StdReviewedHistoryReader,
    StdShellAttachmentStore, StdShellRunner, UnixPtySpawn, UnixSignal,
};
use crate::daemon_bridge;
use crate::daemon_handler::WiredDaemonRequestHandler;
use crate::ports::outbound::{
    MemoryRepository, ReviewedHistoryReader, ShellAttachmentStore, ShellRunner,
};
use crate::usecase::{
    ClearUseCase, HistoryUseCase, InitUseCase, JobsUseCase, MemoryUseCase, MuteUseCase,
    ResumeUseCase, RolloutUseCase, SessionsUseCase, ShellStatusUseCase, ShellUseCase,
    TruncateConsoleLogUseCase, UnmuteUseCase,
};

/// 配線で組み立てたポート群とユースケース（main の Command ディスパッチで利用）
#[cfg(unix)]
pub struct App {
    #[allow(dead_code)] // ユースケース構築に使用。main からは use_case 経由で利用
    pub path_resolver: Arc<dyn PathResolver>,
    #[allow(dead_code)]
    pub fs: Arc<dyn FileSystem>,
    #[allow(dead_code)]
    pub signal: Arc<dyn Signal>,
    #[allow(dead_code)]
    pub shell_runner: Arc<dyn ShellRunner>,
    pub memory_use_case: MemoryUseCase,
    pub shell_use_case: ShellUseCase,
    pub shell_status_use_case: ShellStatusUseCase,
    pub clear_use_case: ClearUseCase,
    pub truncate_console_log_use_case: TruncateConsoleLogUseCase,
    pub rollout_use_case: RolloutUseCase,
    pub mute_use_case: MuteUseCase,
    pub unmute_use_case: UnmuteUseCase,
    pub resume_use_case: ResumeUseCase,
    pub sessions_use_case: SessionsUseCase,
    pub history_use_case: HistoryUseCase,
    pub jobs_use_case: JobsUseCase,
    pub init_use_case: InitUseCase,
    /// 外部拡張（MCP互換ホスト）
    pub mcp_host: Arc<dyn McpHost>,
    /// 構造化ログ（ファイルへ JSONL）。エラー時のコンソール表示とは別。main で lifecycle/error に利用予定。
    #[allow(dead_code)]
    pub logger: Arc<dyn Log>,
    /// 現在のターミナル幅（列数）。main の history ls 表示幅に使用。
    pub get_terminal_width: Box<dyn Fn() -> usize + Send + Sync>,
}

/// 配線: 標準アダプタで App を組み立てる（Unix 専用）
#[cfg(unix)]
pub fn wire_aish() -> App {
    let fs: Arc<dyn FileSystem> = Arc::new(StdFileSystem);
    let env_resolver: Arc<dyn EnvResolver> = Arc::new(StdEnvResolver);
    let runtime_catalog: Arc<dyn RuntimeCatalog> = Arc::new(StdRuntimeCatalog::new(
        Arc::clone(&env_resolver),
        Arc::clone(&fs),
    ));
    let logger: Arc<dyn Log> = env_resolver
        .resolve_log_file_path()
        .map(|path| Arc::new(FileJsonLog::new(Arc::clone(&fs), path)) as Arc<dyn Log>)
        .unwrap_or_else(|_| Arc::new(NoopLog));
    let id_gen: Arc<dyn IdGenerator> = Arc::new(StdIdGenerator::new(Arc::new(StdClock)));
    let path_resolver: Arc<dyn PathResolver> =
        Arc::new(StdPathResolver::new(Arc::clone(&env_resolver)));
    let signal: Arc<dyn Signal> = Arc::new(UnixSignal);
    let pty_spawn = Arc::new(UnixPtySpawn);
    let shell_attachment_store: Arc<dyn ShellAttachmentStore> =
        Arc::new(StdShellAttachmentStore::new(Arc::clone(&fs)));
    let shell_runner: Arc<dyn ShellRunner> = Arc::new(StdShellRunner::new(
        Arc::clone(&env_resolver),
        Arc::clone(&fs),
        Arc::clone(&id_gen),
        Arc::clone(&signal) as Arc<dyn Signal>,
        pty_spawn,
        Arc::clone(&shell_attachment_store),
    ));
    let memory_repository: Arc<dyn MemoryRepository> = Arc::new(LoggingMemoryRepository::new(
        Arc::new(StdMemoryRepository::new(
            Arc::clone(&env_resolver),
            Arc::clone(&runtime_catalog),
        )),
        Arc::clone(&logger),
    ));
    let memory_use_case = MemoryUseCase::new(memory_repository);
    let shell_use_case = ShellUseCase::new(Arc::clone(&path_resolver), Arc::clone(&shell_runner));
    let session_event_store: Arc<dyn SessionEventStore> =
        Arc::new(NdjsonSessionEventStore::new(Arc::clone(&fs)));
    let shell_status_use_case = ShellStatusUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&shell_attachment_store),
        Arc::clone(&session_event_store),
    );
    let clear_use_case = ClearUseCase::new(Arc::clone(&path_resolver), Arc::clone(&fs));
    let truncate_console_log_use_case = TruncateConsoleLogUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
        Arc::clone(&shell_attachment_store),
    );
    let rollout_use_case = RolloutUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
        Arc::clone(&shell_attachment_store),
    );
    let mute_use_case = MuteUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&signal),
        Arc::clone(&shell_attachment_store),
    );
    let unmute_use_case = UnmuteUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&shell_attachment_store),
    );
    let resume_use_case = ResumeUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&shell_runner),
    );
    let sessions_use_case = SessionsUseCase::new(
        Arc::clone(&path_resolver),
        Arc::clone(&fs),
        Arc::clone(&env_resolver),
    );
    let reviewed_history_reader: Arc<dyn ReviewedHistoryReader> =
        Arc::new(StdReviewedHistoryReader::new(Arc::clone(&fs)));
    let history_use_case = HistoryUseCase::new(Arc::clone(&path_resolver), reviewed_history_reader);
    let jobs_use_case =
        JobsUseCase::new(Arc::clone(&path_resolver), Arc::clone(&session_event_store));
    let init_use_case = InitUseCase::new(Arc::clone(&env_resolver), Arc::clone(&fs));
    let mcp_host: Arc<dyn McpHost> = Arc::new(StdioJsonRpcMcpBridgeHost::new());
    let get_terminal_width: Box<dyn Fn() -> usize + Send + Sync> = Box::new(|| {
        const STDOUT_FD: std::os::unix::io::RawFd = 1;
        crate::adapter::platform::get_winsize(STDOUT_FD)
            .ok()
            .map(|w| w.ws_col as usize)
            .filter(|&w| w > 0)
            .unwrap_or(120)
    });
    App {
        path_resolver,
        fs,
        signal,
        shell_runner,
        memory_use_case,
        shell_use_case,
        shell_status_use_case,
        clear_use_case,
        truncate_console_log_use_case,
        rollout_use_case,
        mute_use_case,
        unmute_use_case,
        resume_use_case,
        sessions_use_case,
        history_use_case,
        jobs_use_case,
        init_use_case,
        mcp_host,
        logger,
        get_terminal_width,
    }
}

#[cfg(unix)]
pub fn wire_daemon_server_handlers() -> aish_daemon::ServerHandlers {
    daemon_bridge::build_server_handlers(Arc::new(|| {
        let app = wire_aish();
        Arc::new(WiredDaemonRequestHandler::new(
            app.memory_use_case,
            app.history_use_case,
            Arc::clone(&app.mcp_host),
        ))
    }))
}
