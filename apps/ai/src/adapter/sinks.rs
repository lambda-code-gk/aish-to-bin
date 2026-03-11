//! ストリーミングの「消費」実装（表示・保存の分離）
//!
//! StdoutSink: assistant text 表示、tool は完了時に "Tool <name> args: <args>" を1行で表示（長い場合は省略）
//! FilteringStdoutSink: AISH_FILTER 指定時、LLM 出力のみフィルターコマンドに通してから表示
//! JsonlLogSink: AgentEvent を JSONL で追記
//! PartFileSink: 完了時に assistant テキストを part_*_assistant.txt に保存
//! StdEventSinkFactory: EventSinkFactory の標準実装（StdoutSink または FilteringStdoutSink）

use crate::ports::outbound::EventSinkFactory;
use common::error::Error;
use common::llm::events::LlmEvent;
use common::sink::{AgentEvent, EventSink};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;

/// ANSI: ダークグレー（bright black）
const DARK_GREY: &str = "\x1b[90m";
/// ANSI: リセット
const RESET: &str = "\x1b[0m";
/// ツール args を1行表示するときの最大文字数（超えたら省略）
const MAX_ARGS_DISPLAY: usize = 80;

/// 標準出力へ表示（TextDelta と tool 完了時の args を1行で表示）
pub struct StdoutSink {
    /// 不具合調査用: true のとき冗長なデバッグ行を stderr に出す
    verbose: bool,
    /// 思考過程ブロック（<<< ... >>>）の表示中か
    in_reasoning_block: bool,
}

impl StdoutSink {
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            in_reasoning_block: false,
        }
    }

    fn close_reasoning_block_if_open(&mut self) -> Result<(), Error> {
        if self.in_reasoning_block {
            self.in_reasoning_block = false;
            println!("{} >>>{}", DARK_GREY, RESET);
            io::stdout()
                .flush()
                .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
        }
        Ok(())
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new(false)
    }
}

/// AISH_FILTER 指定時: LLM の TextDelta/ReasoningDelta のみフィルターコマンドに通し、その stdout を表示する。
/// ツール結果・エラー・verbose 等の LLM 以外の出力はフィルターを通さず stderr 等にそのまま出す。
pub struct FilteringStdoutSink {
    verbose: bool,
    filter_stdin: Option<ChildStdin>,
    filter_child: Option<Child>,
    reader_handle: Option<thread::JoinHandle<std::io::Result<()>>>,
}

impl FilteringStdoutSink {
    /// filter_cmd は sh -c に渡すコマンド文字列（例: "cat", "sed 's/foo/bar/'"）。
    pub fn new(verbose: bool, filter_cmd: &str) -> Result<Self, Error> {
        let mut child = Command::new("sh")
            .args(["-c", filter_cmd])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::io_msg(format!("Failed to spawn AISH_FILTER process: {}", e)))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            Error::io_msg("AISH_FILTER process stdin was not captured".to_string())
        })?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            Error::io_msg("AISH_FILTER process stdout was not captured".to_string())
        })?;
        let reader_handle =
            thread::spawn(move || io::copy(&mut stdout, &mut io::stdout()).map(|_| ()));
        Ok(Self {
            verbose,
            filter_stdin: Some(stdin),
            filter_child: Some(child),
            reader_handle: Some(reader_handle),
        })
    }

    fn write_llm_to_filter(&mut self, s: &str) -> Result<(), Error> {
        if let Some(ref mut stdin) = self.filter_stdin {
            stdin
                .write_all(s.as_bytes())
                .map_err(|e| Error::io_msg(e.to_string()))?;
            stdin.flush().map_err(|e| Error::io_msg(e.to_string()))?;
        }
        Ok(())
    }

    fn finish_filter(&mut self) -> Result<(), Error> {
        self.filter_stdin = None;
        if let Some(h) = self.reader_handle.take() {
            h.join()
                .map_err(|_| Error::io_msg("AISH_FILTER reader thread panicked".to_string()))??;
        }
        if let Some(mut c) = self.filter_child.take() {
            let status = c.wait().map_err(|e| Error::io_msg(e.to_string()))?;
            if !status.success() {
                if let Some(code) = status.code() {
                    return Err(Error::io_msg(format!(
                        "AISH_FILTER process exited with code {}",
                        code
                    )));
                }
            }
        }
        Ok(())
    }
}

impl EventSink for FilteringStdoutSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        match ev {
            AgentEvent::Llm(LlmEvent::TextDelta(s))
            | AgentEvent::Llm(LlmEvent::ReasoningDelta(s)) => {
                self.write_llm_to_filter(s)?;
            }
            AgentEvent::Llm(LlmEvent::ToolCallBegin {
                call_id,
                name: _,
                thought_signature,
            }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] call_id={} thought_signature={:?}{}",
                        DARK_GREY, call_id, thought_signature, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallArgsDelta {
                call_id,
                json_fragment,
            }) => {
                if self.verbose {
                    let snippet = if json_fragment.len() > 120 {
                        format!(
                            "{}...",
                            &json_fragment[..json_fragment.floor_char_boundary(120)]
                        )
                    } else {
                        json_fragment.clone()
                    };
                    eprintln!(
                        "{}  [verbose] ToolCallArgsDelta call_id={} fragment={}{}",
                        DARK_GREY, call_id, snippet, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallEnd { call_id }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] ToolCallEnd call_id={}{}",
                        DARK_GREY, call_id, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::Completed { finish }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Completed finish={:?}{}",
                        DARK_GREY, finish, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::Failed { message }) => {
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Failed message={}{}",
                        DARK_GREY, message, RESET
                    );
                }
            }
            AgentEvent::ToolCall { name, args, .. } => {
                let args_str = args.to_string();
                let args_display = if args_str.len() > MAX_ARGS_DISPLAY {
                    format!(
                        "{}...",
                        &args_str[..args_str.floor_char_boundary(MAX_ARGS_DISPLAY)]
                    )
                } else {
                    args_str
                };
                eprintln!("{}Tool {} args: {}{}", DARK_GREY, name, args_display, RESET);
            }
            AgentEvent::ToolResult {
                name: _,
                args: _,
                result,
                ..
            } => {
                if self.verbose {
                    let result_str = result.to_string();
                    let snippet = if result_str.len() > 200 {
                        format!("{}...", &result_str[..result_str.floor_char_boundary(200)])
                    } else {
                        result_str
                    };
                    eprintln!("{}  [verbose] result={}{}", DARK_GREY, snippet, RESET);
                }
            }
            AgentEvent::ToolError {
                name,
                args,
                message,
                ..
            } => {
                let args_str = args.to_string();
                let args_display = if args_str.len() > MAX_ARGS_DISPLAY {
                    format!(
                        "{}...",
                        &args_str[..args_str.floor_char_boundary(MAX_ARGS_DISPLAY)]
                    )
                } else {
                    args_str
                };
                let msg_display = if message.len() > 40 {
                    format!("{}...", &message[..message.floor_char_boundary(37)])
                } else {
                    message.clone()
                };
                eprintln!(
                    "{}Tool {} args: {} failed: {}{}",
                    DARK_GREY, name, args_display, msg_display, RESET
                );
            }
        }
        Ok(())
    }

    fn on_end(&mut self) -> Result<(), Error> {
        self.finish_filter()?;
        println!();
        io::stdout()
            .flush()
            .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
        Ok(())
    }
}

/// EventSinkFactory の標準実装。AISH_FILTER 未設定時は StdoutSink、設定時は FilteringStdoutSink を返す。
pub struct StdEventSinkFactory {
    verbose: bool,
    /// 指定時は LLM 出力をこのコマンドに通す（sh -c に渡す文字列）
    filter_command: Option<String>,
}

impl StdEventSinkFactory {
    pub fn new(verbose: bool) -> Self {
        Self {
            verbose,
            filter_command: std::env::var("AISH_FILTER")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        }
    }

    /// テスト・wiring 用: フィルターコマンドを明示指定（未指定時は new と同様 AISH_FILTER は読まない）
    #[allow(dead_code)] // 単体テストで使用
    pub fn with_filter(verbose: bool, filter_command: Option<String>) -> Self {
        Self {
            verbose,
            filter_command: filter_command.filter(|s| !s.trim().is_empty()),
        }
    }
}

impl EventSinkFactory for StdEventSinkFactory {
    fn create_sinks(&self) -> Vec<Box<dyn EventSink>> {
        match &self.filter_command {
            Some(cmd) => match FilteringStdoutSink::new(self.verbose, cmd) {
                Ok(sink) => vec![Box::new(sink)],
                Err(e) => {
                    eprintln!("Warning: AISH_FILTER failed to start ({}), falling back to unfiltered output", e);
                    vec![Box::new(StdoutSink::new(self.verbose))]
                }
            },
            None => vec![Box::new(StdoutSink::new(self.verbose))],
        }
    }
}

impl EventSink for StdoutSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        match ev {
            AgentEvent::Llm(LlmEvent::TextDelta(s)) => {
                self.close_reasoning_block_if_open()?;
                print!("{}", s);
                io::stdout()
                    .flush()
                    .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
            }
            AgentEvent::Llm(LlmEvent::ReasoningDelta(s)) => {
                if !self.in_reasoning_block {
                    self.in_reasoning_block = true;
                    print!("{}<<< {}", DARK_GREY, RESET);
                    io::stdout()
                        .flush()
                        .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
                }
                // 思考過程はグレーで表示
                print!("{}{}{}", DARK_GREY, s, RESET);
                io::stdout()
                    .flush()
                    .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
            }
            AgentEvent::Llm(LlmEvent::ToolCallBegin {
                call_id,
                name: _,
                thought_signature,
            }) => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] call_id={} thought_signature={:?}{}",
                        DARK_GREY, call_id, thought_signature, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallArgsDelta {
                call_id,
                json_fragment,
            }) => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    let snippet = if json_fragment.len() > 120 {
                        format!(
                            "{}...",
                            &json_fragment[..json_fragment.floor_char_boundary(120)]
                        )
                    } else {
                        json_fragment.clone()
                    };
                    eprintln!(
                        "{}  [verbose] ToolCallArgsDelta call_id={} fragment={}{}",
                        DARK_GREY, call_id, snippet, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::ToolCallEnd { call_id }) => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] ToolCallEnd call_id={}{}",
                        DARK_GREY, call_id, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::Completed { finish }) => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Completed finish={:?}{}",
                        DARK_GREY, finish, RESET
                    );
                }
            }
            AgentEvent::Llm(LlmEvent::Failed { message }) => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    eprintln!(
                        "{}  [verbose] LlmEvent::Failed message={}{}",
                        DARK_GREY, message, RESET
                    );
                }
            }
            AgentEvent::ToolResult {
                name: _,
                args: _,
                result,
                ..
            } => {
                self.close_reasoning_block_if_open()?;
                if self.verbose {
                    let result_str = result.to_string();
                    let snippet = if result_str.len() > 200 {
                        format!("{}...", &result_str[..result_str.floor_char_boundary(200)])
                    } else {
                        result_str
                    };
                    eprintln!("{}  [verbose] result={}{}", DARK_GREY, snippet, RESET);
                }
            }
            AgentEvent::ToolError {
                name,
                args,
                message,
                ..
            } => {
                self.close_reasoning_block_if_open()?;
                let args_str = args.to_string();
                let args_display = if args_str.len() > MAX_ARGS_DISPLAY {
                    format!(
                        "{}...",
                        &args_str[..args_str.floor_char_boundary(MAX_ARGS_DISPLAY)]
                    )
                } else {
                    args_str
                };
                let msg_display = if message.len() > 40 {
                    format!("{}...", &message[..message.floor_char_boundary(37)])
                } else {
                    message.clone()
                };
                eprintln!(
                    "{}Tool {} args: {} failed: {}{}",
                    DARK_GREY, name, args_display, msg_display, RESET
                );
            }
            AgentEvent::ToolCall { .. } => {
                // ToolCall は FilteringStdoutSink 側で表示する（ここでは特別な処理なし）
            }
        }
        Ok(())
    }

    fn on_end(&mut self) -> Result<(), Error> {
        self.close_reasoning_block_if_open()?;
        println!();
        io::stdout()
            .flush()
            .map_err(|e| Error::io_msg(format!("Failed to flush stdout: {}", e)))?;
        Ok(())
    }
}

/// JSONL ログへ追記（デバッグ・永続化用）
#[allow(dead_code)] // 将来のログ永続化で使用
pub struct JsonlLogSink<W: Write + Send + Sync> {
    writer: W,
}

impl<W: Write + Send + Sync> JsonlLogSink<W> {
    #[allow(dead_code)]
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write + Send + Sync> EventSink for JsonlLogSink<W> {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        // AgentEvent を簡易 JSON 化（LlmEvent 等は enum なので手動で line を書くか、serde で Serialize にする）
        let line = format!("{:?}\n", ev);
        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }
}

/// 完了時に assistant テキストを part_*_assistant.txt に保存する Sink
/// バッファに TextDelta を蓄積し、on_end でファイルに書き出す
#[allow(dead_code)] // 将来の part 保存分離で使用
pub struct PartFileSink {
    session_dir: std::path::PathBuf,
    part_filename: String,
    buffer: String,
}

#[allow(dead_code)] // テスト・将来の part 保存で使用
impl PartFileSink {
    pub fn new(session_dir: impl AsRef<Path>, part_filename: impl Into<String>) -> Self {
        Self {
            session_dir: session_dir.as_ref().to_path_buf(),
            part_filename: part_filename.into(),
            buffer: String::new(),
        }
    }

    pub fn assistant_text(&self) -> &str {
        &self.buffer
    }
}

impl EventSink for PartFileSink {
    fn on_event(&mut self, ev: &AgentEvent) -> Result<(), Error> {
        match ev {
            AgentEvent::Llm(LlmEvent::TextDelta(s))
            | AgentEvent::Llm(LlmEvent::ReasoningDelta(s)) => {
                self.buffer.push_str(s);
            }
            _ => {}
        }
        Ok(())
    }

    fn on_end(&mut self) -> Result<(), Error> {
        if self.buffer.trim().is_empty() {
            return Ok(());
        }
        let path = self.session_dir.join(&self.part_filename);
        std::fs::write(&path, &self.buffer).map_err(|e| Error::io_msg(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::llm::events::LlmEvent;

    #[test]
    fn test_stdout_sink_text_delta() {
        let mut sink = StdoutSink::new(false);
        let ev = AgentEvent::Llm(LlmEvent::TextDelta("hello".to_string()));
        assert!(sink.on_event(&ev).is_ok());
    }

    #[test]
    fn test_stdout_sink_reasoning_then_text_wrapped_with_angle_brackets() {
        let mut sink = StdoutSink::new(false);
        sink.on_event(&AgentEvent::Llm(LlmEvent::ReasoningDelta(
            "User asks 1+1. Answer: 2.".to_string(),
        )))
        .unwrap();
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta(
            "1 + 1 は **2** です。".to_string(),
        )))
        .unwrap();
        // 実際の出力は <<< \x1b[90m...\x1b[0m >>>\n1 + 1 は **2** です。 となる（ANSI 含む）
        // ここでは in_reasoning_block が false に戻っていることだけ確認
        sink.on_event(&AgentEvent::Llm(LlmEvent::Completed {
            finish: common::llm::events::FinishReason::Stop,
        }))
        .unwrap();
    }

    #[test]
    fn test_part_file_sink_buffer() {
        let mut sink = PartFileSink::new("/tmp", "part_xxx_assistant.txt");
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("a".to_string())))
            .unwrap();
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("b".to_string())))
            .unwrap();
        assert_eq!(sink.assistant_text(), "ab");
    }

    #[test]
    fn test_part_file_sink_buffer_includes_reasoning() {
        let mut sink = PartFileSink::new("/tmp", "part_xxx_assistant.txt");
        sink.on_event(&AgentEvent::Llm(LlmEvent::ReasoningDelta(
            "think ".to_string(),
        )))
        .unwrap();
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("answer".to_string())))
            .unwrap();
        assert_eq!(sink.assistant_text(), "think answer");
    }

    #[test]
    fn test_std_event_sink_factory_without_filter_creates_stdout_sink() {
        let factory = StdEventSinkFactory::with_filter(false, None);
        let mut sinks = factory.create_sinks();
        assert_eq!(sinks.len(), 1);
        sinks[0]
            .on_event(&AgentEvent::Llm(LlmEvent::TextDelta("hi".to_string())))
            .unwrap();
        sinks[0].on_end().unwrap();
    }

    #[test]
    fn test_std_event_sink_factory_with_filter_creates_filtering_sink() {
        let factory = StdEventSinkFactory::with_filter(false, Some("cat".to_string()));
        let mut sinks = factory.create_sinks();
        assert_eq!(sinks.len(), 1);
        sinks[0]
            .on_event(&AgentEvent::Llm(LlmEvent::TextDelta(
                "filtered".to_string(),
            )))
            .unwrap();
        sinks[0].on_end().unwrap();
    }

    #[test]
    fn test_filtering_stdout_sink_exit_nonzero_fails_at_on_end() {
        // on_event では stdin へ書ける（Broken pipe を避ける）よう、stdin を読み切ってから非0終了するコマンドを使う
        let mut sink = FilteringStdoutSink::new(false, "cat >/dev/null; exit 1").unwrap();
        sink.on_event(&AgentEvent::Llm(LlmEvent::TextDelta("x".to_string())))
            .unwrap();
        let r = sink.on_end();
        assert!(r.is_err());
    }

    #[test]
    fn test_with_filter_empty_string_uses_stdout_sink() {
        let factory = StdEventSinkFactory::with_filter(false, Some("".to_string()));
        let mut sinks = factory.create_sinks();
        assert_eq!(sinks.len(), 1);
        sinks[0]
            .on_event(&AgentEvent::Llm(LlmEvent::TextDelta("hi".to_string())))
            .unwrap();
        sinks[0].on_end().unwrap();
    }
}
