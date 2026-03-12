//! タスク名のドメイン型（task 解決規約の入口）

/// TaskName のパースエラー
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskNameParseError {
    #[error("task name must not be empty")]
    Empty,
    #[error("task name must not contain '/' or '\\\\'")]
    ContainsSeparator,
    #[error("task name must not be '.' or '..'")]
    DotOrDotDot,
    #[error("task name must not contain NUL")]
    ContainsNul,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskName(String);

impl TaskName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// ユーザー入力などの生文字列から TaskName を構築する入口。
    ///
    /// ルール:
    /// - 前後の空白は無視（trim）する
    /// - 空文字不可
    /// - `/` と `\` は不可（パスセグメントを跨がない）
    /// - `.` / `..` は不可
    /// - NUL を含む文字列は不可
    pub fn parse(raw: &str) -> Result<Self, TaskNameParseError> {
        // 固定方針: 前後空白はトリムして扱う
        let s = raw.trim();
        if s.is_empty() {
            return Err(TaskNameParseError::Empty);
        }
        if s.contains('/') || s.contains('\\') {
            return Err(TaskNameParseError::ContainsSeparator);
        }
        if s == "." || s == ".." {
            return Err(TaskNameParseError::DotOrDotDot);
        }
        if s.chars().any(|c| c == '\0') {
            return Err(TaskNameParseError::ContainsNul);
        }
        Ok(TaskName::new(s.to_string()))
    }

    /// `task_name.sh` 形式のスクリプトファイル名を返す。
    pub fn to_script_file_name(&self) -> String {
        format!("{}.sh", self.0)
    }

    /// ディレクトリ名として使う文字列を返す（現在は生のタスク名と同一）。
    pub fn as_dir_name(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for TaskName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for TaskName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trims_and_accepts_simple_name() {
        let name = TaskName::parse("  build ").expect("should parse");
        assert_eq!(name.as_ref(), "build");
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(TaskName::parse(""), Err(TaskNameParseError::Empty));
        assert_eq!(TaskName::parse("   "), Err(TaskNameParseError::Empty));
    }

    #[test]
    fn parse_rejects_separators_and_dotdot() {
        assert_eq!(
            TaskName::parse("foo/bar"),
            Err(TaskNameParseError::ContainsSeparator)
        );
        assert_eq!(
            TaskName::parse("foo\\bar"),
            Err(TaskNameParseError::ContainsSeparator)
        );
        assert_eq!(TaskName::parse("."), Err(TaskNameParseError::DotOrDotDot));
        assert_eq!(TaskName::parse(".."), Err(TaskNameParseError::DotOrDotDot));
    }

    #[test]
    fn script_and_dir_conversion() {
        let name = TaskName::new("deploy");
        assert_eq!(name.as_dir_name(), "deploy");
        assert_eq!(name.to_script_file_name(), "deploy.sh".to_string());
    }
}
