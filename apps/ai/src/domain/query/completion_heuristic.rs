//! 責務: アシスタント応答の「完了 / 質問 / コマンド提示」形状判定のみ。I/O を知らない。

/// コードフェンス（``` / ~~~）で囲まれたブロックを除去し、地の文だけを返す。
pub fn strip_fenced_code_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in s.lines() {
        let l = line.trim_start();
        if l.starts_with("```") || l.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// 最後の非空行を返す。
pub fn last_non_empty_line(s: &str) -> Option<&str> {
    s.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim())
}

/// アシスタントの応答が「質問/入力待ち」で終わっているかを判定する純関数。
///
/// コードフェンス内は無視し、地の文の最終行が:
/// - `?` / `？` で終わる -> 質問
/// - `:` / `：` で終わる短いラベル -> 入力待ち
pub fn looks_like_question_or_need_user_input(assistant_text: &str) -> bool {
    let outside = strip_fenced_code_blocks(assistant_text);
    let last = match last_non_empty_line(&outside) {
        Some(l) => l,
        None => return false,
    };
    if last.ends_with('?') || last.ends_with('？') {
        return true;
    }
    if last.ends_with(':') || last.ends_with('：') {
        let core = last.trim_end_matches(|c| c == ':' || c == '：').trim();
        if !core.is_empty()
            && core.chars().count() <= 24
            && !core.chars().any(|c| c.is_whitespace())
        {
            return true;
        }
    }
    false
}

/// アシスタントの応答が「コマンド/手順の提示で止まった」形状かを判定する純関数。
pub fn looks_like_command_block(s: &str) -> bool {
    let t = s.trim();
    if t.contains("```") || t.contains("~~~") {
        return true;
    }
    let mut shell_prompt_lines = 0usize;
    let mut pipe_like_lines = 0usize;
    for line in t.lines() {
        let l = line.trim_start();
        if l.starts_with("$ ") || l.starts_with("> ") || l.starts_with("PS>") {
            shell_prompt_lines += 1;
        }
        if l.contains("&&") || l.contains('|') {
            pipe_like_lines += 1;
        }
    }
    shell_prompt_lines >= 1 || pipe_like_lines >= 2
}

/// retry 用のデフォルト followup プロンプトを生成する純関数。
pub fn default_followup() -> String {
    let marker = "[AISH_INTERNAL] retry_for_completion_v1";
    format!(
        "{marker}\nobjective: complete the user's request end-to-end\nrequirements:\n  - do not stop at suggested commands only\n  - prefer using tools to execute and verify\n  - if critical information is missing, ask exactly one clarification question\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_mark_detected() {
        assert!(looks_like_question_or_need_user_input(
            "How should I do this?"
        ));
        assert!(looks_like_question_or_need_user_input("何をしますか？"));
    }

    #[test]
    fn colon_label_detected() {
        assert!(looks_like_question_or_need_user_input("Filename:"));
    }

    #[test]
    fn normal_text_not_detected() {
        assert!(!looks_like_question_or_need_user_input("Done."));
        assert!(!looks_like_question_or_need_user_input(
            "I have completed the task."
        ));
    }

    #[test]
    fn code_fence_detected() {
        assert!(looks_like_command_block("Try this:\n```\necho hello\n```"));
    }

    #[test]
    fn shell_prompt_detected() {
        assert!(looks_like_command_block("$ npm install"));
    }

    #[test]
    fn pipe_lines_detected() {
        assert!(looks_like_command_block(
            "cat foo | grep bar\necho x && echo y"
        ));
    }

    #[test]
    fn plain_text_not_command() {
        assert!(!looks_like_command_block("Everything is set up correctly."));
    }

    #[test]
    fn strip_fenced_removes_code() {
        let input = "before\n```\ncode\n```\nafter";
        let result = strip_fenced_code_blocks(input);
        assert!(result.contains("before"));
        assert!(result.contains("after"));
        assert!(!result.contains("code"));
    }

    #[test]
    fn question_inside_code_fence_not_detected() {
        let text = "Here's an example:\n```\nWhat is this?\n```\nDone.";
        assert!(!looks_like_question_or_need_user_input(text));
    }

    #[test]
    fn default_followup_contains_marker() {
        let f = default_followup();
        assert!(f.contains("[AISH_INTERNAL] retry_for_completion_v1"));
    }
}
