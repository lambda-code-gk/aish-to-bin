//! 責務: ツール呼び出しの要約テキスト生成（文字数ベース切り詰め）のみ。I/O を知らない。

const TOOL_SUMMARY_MAX_CHARS: usize = 200;

/// UTF-8 安全な文字数ベースの切り詰め。
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...(truncated)", truncated)
    }
}

/// ツール呼び出しの概要テキストを生成する純関数。
///
/// `replace_file` は path / old_block / new_block を短く要約する。
/// その他のツールは `tool_name args_json` を切り詰める。
pub fn tool_summary_preview(tool_name: &str, tool_args: &serde_json::Value) -> String {
    if tool_name == "replace_file" {
        let path = tool_args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let old_block = tool_args
            .get("old_block")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let new_block = tool_args
            .get("new_block")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let path_preview = truncate_chars(path, 80);
        let old_first = old_block.lines().next().unwrap_or("");
        let new_first = new_block.lines().next().unwrap_or("");
        let old_preview = truncate_chars(old_first, 40);
        let new_preview = truncate_chars(new_first, 40);

        let summary = format!(
            "replace_file path={} old:[{}] -> new:[{}]",
            path_preview, old_preview, new_preview
        );
        return truncate_chars(&summary, TOOL_SUMMARY_MAX_CHARS);
    }

    let args_str = serde_json::to_string(tool_args).unwrap_or_else(|_| "{}".to_string());
    let summary = format!("{} {}", tool_name, args_str);
    truncate_chars(&summary, TOOL_SUMMARY_MAX_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_short_string_unchanged() {
        assert_eq!(truncate_chars("hello", 10), "hello");
    }

    #[test]
    fn truncate_chars_long_string_truncated() {
        let long = "a".repeat(300);
        let result = truncate_chars(&long, 200);
        assert!(result.ends_with("...(truncated)"));
        assert_eq!(
            result.chars().count(),
            200 + "...(truncated)".chars().count()
        );
    }

    #[test]
    fn truncate_chars_utf8_safe() {
        let s = "あ".repeat(100);
        let result = truncate_chars(&s, 50);
        assert!(result.ends_with("...(truncated)"));
    }

    #[test]
    fn replace_file_summary() {
        let args = serde_json::json!({
            "path": "/workspace/main.rs",
            "old_block": "fn old() {}",
            "new_block": "fn new() {}",
        });
        let summary = tool_summary_preview("replace_file", &args);
        assert!(summary.contains("replace_file"));
        assert!(summary.contains("/workspace/main.rs"));
        assert!(summary.contains("old:["));
        assert!(summary.contains("-> new:["));
    }

    #[test]
    fn generic_tool_summary() {
        let args = serde_json::json!({"pattern": "foo"});
        let summary = tool_summary_preview("grep", &args);
        assert!(summary.starts_with("grep"));
    }
}
