//! コンテキスト追加情報セレクタの実装（変更ファイルスニペット等）

use crate::domain::{hash64, ContextAddon, ContextAttachment, ContextSource};
use crate::ports::outbound::{ContextAddonInput, ContextAddonSelector};
use common::error::Error;
use common::msg::Msg;
use common::ports::outbound::FileSystem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// git diff --name-only を実行。失敗時は Err を返し builder が addon.selector error decision を積む。
fn git_changed_files(project_root: &Path, max_files: usize) -> Result<Vec<PathBuf>, Error> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only"])
        .current_dir(project_root)
        .output()
        .map_err(|e| Error::system(format!("git diff failed: {}", e)))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::system(format!("git diff failed: {}", stderr)));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(max_files)
        .map(PathBuf::from)
        .collect())
}

/// 変更ファイルのスニペットを ContextAddon として返すセレクタ
pub struct ChangedFilesSnippetSelector {
    fs: Arc<dyn FileSystem>,
    max_files: usize,
    max_lines: usize,
    max_bytes: usize,
}

impl ChangedFilesSnippetSelector {
    /// 引数順: max_files, max_lines, max_bytes（仕様 v0.2）
    pub fn new(
        fs: Arc<dyn FileSystem>,
        max_files: usize,
        max_lines: usize,
        max_bytes: usize,
    ) -> Self {
        Self {
            fs,
            max_files,
            max_lines,
            max_bytes,
        }
    }
}

fn truncate_snippet(content: &str, max_bytes: usize, max_lines: usize) -> String {
    let mut result = String::new();
    let mut lines = 0;
    for line in content.lines() {
        if lines >= max_lines || result.len() + line.len() + 1 > max_bytes {
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        lines += 1;
    }
    result
}

impl ContextAddonSelector for ChangedFilesSnippetSelector {
    fn name(&self) -> &str {
        "changed_files_snippet"
    }

    fn select(&self, input: &ContextAddonInput) -> Result<Vec<ContextAddon>, Error> {
        let _ = input.history;
        let _ = input.query;
        let files = git_changed_files(input.project_root, self.max_files)?;
        let mut addons = Vec::new();
        for rel_path in files {
            let abs_path = input.project_root.join(&rel_path);
            let content = match self.fs.read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let snippet = truncate_snippet(&content, self.max_bytes, self.max_lines);
            if snippet.is_empty() {
                continue;
            }
            let path_str = rel_path.display().to_string();
            let hash = hash64(&snippet);
            let bytes = snippet.len() as u64;
            let msg_text = format!(
                "Context: file snippet: {}\n```text\n{}\n```",
                path_str, snippet
            );
            addons.push(ContextAddon {
                id: format!("changed_file:{}", path_str),
                kind: "file_snippet".to_string(),
                title: path_str.clone(),
                priority: 50,
                msg: Msg::user(msg_text),
                attachment: Some(ContextAttachment {
                    kind: "file_snippet".to_string(),
                    title: path_str.clone(),
                    content_type: "text/plain".to_string(),
                    content: Some(snippet),
                    artifact_rel_path: None,
                    bytes,
                    hash64: hash,
                    source: Some(ContextSource {
                        kind: "file".to_string(),
                        ref_id: format!("path:{}", path_str),
                    }),
                }),
                source: Some(ContextSource {
                    kind: "file".to_string(),
                    ref_id: format!("path:{}", path_str),
                }),
            });
        }
        Ok(addons)
    }
}
