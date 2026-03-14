//! 責務: shell が保持する console.txt 末尾を optional context source として読む。

use crate::domain::{hash64, ContextAddon, ContextAttachment, ContextSource};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::path::Path;
use std::sync::Arc;

pub struct ShellConsoleSelector {
    fs: Arc<dyn FileSystem>,
    max_chars: usize,
    max_lines: usize,
}

impl ShellConsoleSelector {
    pub fn new(fs: Arc<dyn FileSystem>, max_chars: usize, max_lines: usize) -> Self {
        Self {
            fs,
            max_chars,
            max_lines,
        }
    }

    fn tail_chars(&self, text: &str) -> String {
        if text.len() <= self.max_chars {
            return text.to_string();
        }
        let mut start = text.len().saturating_sub(self.max_chars);
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_string()
    }

    fn tail_lines<'a>(&self, text: &'a str) -> &'a str {
        if self.max_lines == 0 {
            return "";
        }
        let mut starts = vec![0usize];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' && idx + 1 < text.len() {
                starts.push(idx + 1);
            }
        }
        if starts.len() <= self.max_lines {
            text
        } else {
            &text[starts[starts.len() - self.max_lines]..]
        }
    }

    fn load_console_tail(&self, session_dir: &Path) -> Result<Option<String>, Error> {
        let console_path = session_dir.join("console.txt");
        if !self.fs.exists(&console_path) {
            return Ok(None);
        }
        let raw = self.fs.read_to_string(&console_path)?;
        if raw.trim().is_empty() {
            return Ok(None);
        }
        let tailed = self.tail_chars(&raw);
        let tailed = self.tail_lines(&tailed).trim().to_string();
        if tailed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(tailed))
        }
    }
}

impl ContextAddonSelector for ShellConsoleSelector {
    fn name(&self) -> &str {
        "shell_console"
    }

    fn select(&self, input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error> {
        let Some(session_dir) = input.session_dir else {
            return Ok(vec![]);
        };
        let Some(console_tail) = self.load_console_tail(session_dir.as_ref())? else {
            return Ok(vec![]);
        };

        let content_text = format!("Shell console snapshot\n---\n{}", console_tail);
        let msg_text = format!("Context: shell console\n```text\n{}\n```", content_text);
        let hash = hash64(&content_text);
        let source = ContextSource {
            kind: "shell_console".to_string(),
            ref_id: "session:console.txt".to_string(),
        };

        Ok(vec![ContextAddon {
            id: "shell_console:tail".to_string(),
            kind: "shell_console".to_string(),
            title: "shell console".to_string(),
            priority: 60,
            msg: Msg::user(msg_text),
            attachment: Some(ContextAttachment {
                kind: "shell_console".to_string(),
                title: "console.txt".to_string(),
                content_type: "text/plain".to_string(),
                content: Some(content_text.clone()),
                artifact_rel_path: None,
                bytes: content_text.len() as u64,
                hash64: hash,
                source: Some(source.clone()),
            }),
            source: Some(source),
        }])
    }
}
