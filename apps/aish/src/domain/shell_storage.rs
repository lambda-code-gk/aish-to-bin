//! shell attachment / console log 周辺の保存パス定義

use std::path::{Path, PathBuf};

pub const CONSOLE_FILENAME: &str = "console.txt";
pub const PENDING_INPUT_FILENAME: &str = "pending_input.json";
pub const PROMPT_SUGGESTION_FILENAME: &str = "prompt_suggestion.txt";
pub const MUTE_FLAG_FILENAME: &str = "console.muted";
pub const PART_FILE_PREFIX: &str = "part_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellStorageLayout {
    pub console_path: String,
    pub pending_input_path: String,
    pub prompt_suggestion_path: String,
    pub mute_flag_path: String,
    pub part_file_prefix: String,
}

impl Default for ShellStorageLayout {
    fn default() -> Self {
        Self {
            console_path: CONSOLE_FILENAME.to_string(),
            pending_input_path: PENDING_INPUT_FILENAME.to_string(),
            prompt_suggestion_path: PROMPT_SUGGESTION_FILENAME.to_string(),
            mute_flag_path: MUTE_FLAG_FILENAME.to_string(),
            part_file_prefix: PART_FILE_PREFIX.to_string(),
        }
    }
}

impl ShellStorageLayout {
    pub fn console_file(&self, session_dir: &Path) -> PathBuf {
        session_dir.join(&self.console_path)
    }

    pub fn pending_input_file(&self, session_dir: &Path) -> PathBuf {
        session_dir.join(&self.pending_input_path)
    }

    pub fn prompt_suggestion_file(&self, session_dir: &Path) -> PathBuf {
        session_dir.join(&self.prompt_suggestion_path)
    }

    pub fn mute_flag_file(&self, session_dir: &Path) -> PathBuf {
        session_dir.join(&self.mute_flag_path)
    }

    pub fn is_part_file_name(&self, file_name: &str) -> bool {
        file_name.starts_with(&self.part_file_prefix)
    }
}
