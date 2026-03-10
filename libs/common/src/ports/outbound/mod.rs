//! Outbound ポート: アプリが外界（FS・時刻・プロセス・LLM・ツール・Sink 等）を使うための trait

pub mod clock;
pub mod env_resolver;
pub mod event_appender;
pub mod event_sink;
pub mod fs;
pub mod log;
pub mod mcp_host;
pub mod path_resolver;
pub mod process;
pub mod runtime_catalog;
pub mod session_event_store;
pub mod sink;
pub mod tool;

#[cfg(unix)]
pub mod pty;
#[cfg(unix)]
pub mod signal;

pub mod id_generator;
pub mod llm_provider;

pub use clock::Clock;
pub use env_resolver::EnvResolver;
pub use event_appender::EventAppender;
pub use event_sink::EventRecordSink;
pub use fs::{FileMetadata, FileSystem};
pub use id_generator::IdGenerator;
pub use llm_provider::LlmProvider;
pub use log::{now_iso8601, Log, LogLevel, LogRecord};
pub use mcp_host::{
    McpCallContext, McpCallResult, McpHost, McpServerDescriptor, McpServerId, McpToolId,
    ToolDescriptor,
};
pub use path_resolver::{PathResolver, PathResolverInput};
pub use process::Process;
pub use runtime_catalog::RuntimeCatalog;
pub use session_event_store::SessionEventStore;
pub use sink::{AgentEvent, EventSink};
pub use tool::Tool;

#[cfg(unix)]
pub use pty::{Pty, PtyProcessStatus, PtySpawn, Winsize};
#[cfg(unix)]
pub use signal::Signal;
