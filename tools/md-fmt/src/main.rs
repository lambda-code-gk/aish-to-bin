//! 標準入力から逐次入力される Markdown を整形して標準出力に出すフィルタ。
//! ストリーミング入力（例: ai コマンドの出力）を随時処理し、できるだけ随時出力する。

use std::io::{self, BufRead, Write};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// ANSI エスケープ（決め打ち）
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";

/// コードブロック内かどうか
enum BlockState {
    Normal,
    CodeBlock,
}

fn main() -> io::Result<()> {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut state = BlockState::Normal;
    let mut code_buf: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim_end();

        match &state {
            BlockState::Normal => {
                if fence_start(trimmed).is_some() {
                    // コードブロック開始 → バッファに貯める
                    state = BlockState::CodeBlock;
                    code_buf.clear();
                    code_buf.push(trimmed.to_string());
                } else {
                    // 通常行をその場で整形して出力
                    format_line(trimmed, &mut stdout)?;
                    stdout.flush()?;
                }
            }
            BlockState::CodeBlock => {
                code_buf.push(trimmed.to_string());
                if is_fence(trimmed, &code_buf[0]) {
                    // コードブロック終了 → バッファを整形して一括出力
                    output_code_block(&code_buf, &ps, &ts, &mut stdout)?;
                    stdout.flush()?;
                    state = BlockState::Normal;
                    code_buf.clear();
                }
            }
        }
    }

    // 入力終了時、コードブロックが閉じていない場合はそのまま出力
    if !code_buf.is_empty() {
        output_code_block(&code_buf, &ps, &ts, &mut stdout)?;
        stdout.flush()?;
    }

    Ok(())
}

/// 行がフェンス（``` または ```lang）ならそのフェンス文字列を返す
fn fence_start(line: &str) -> Option<&str> {
    let s = line.trim_start();
    if s.starts_with("```") {
        let rest = s[3..].trim_start();
        let len = 3 + rest.find(|c: char| c.is_whitespace() || c == '\n').unwrap_or(rest.len());
        Some(&s[..len.min(s.len())])
    } else {
        None
    }
}

fn fence_len(line: &str) -> usize {
    line.trim_start()
        .chars()
        .take_while(|&c| c == '`')
        .count()
}

fn is_fence(line: &str, _fence: &str) -> bool {
    let s = line.trim_start();
    s.starts_with('`') && fence_len(line) >= 3
}

/// 開始フェンス行（``` または ```lang）から言語トークンを抽出する
fn extract_lang(fence_line: &str) -> Option<&str> {
    let s = fence_line.trim_start().strip_prefix("```")?;
    let rest = s.trim_start();
    if rest.is_empty() {
        return None;
    }
    let lang = rest
        .split_whitespace()
        .next()
        .filter(|t| !t.is_empty())?;
    Some(lang)
}

/// 言語トークンから syntect の Syntax を解決する。```bash 等でシェル用が取れるようフォールバックする
fn resolve_syntax<'a>(ps: &'a SyntaxSet, lang: &str) -> Option<&'a syntect::parsing::SyntaxReference> {
    ps.find_syntax_by_token(lang).or_else(|| {
        // シェル系は拡張子が "sh" のことが多い
        let lower = lang.to_lowercase();
        if matches!(
            lower.as_str(),
            "bash" | "zsh" | "sh" | "shell" | "shellscript"
        ) {
            ps.find_syntax_by_token("sh")
        } else {
            None
        }
    })
}

/// コードブロックの行たちを整形して出力。言語指定があれば syntect でシンタックスハイライト
fn output_code_block(
    lines: &[String],
    ps: &SyntaxSet,
    ts: &ThemeSet,
    w: &mut impl Write,
) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let lang = extract_lang(lines[0].trim_end());
    let theme = ts
        .themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.values().next());

    let (syntax, do_highlight) = match (lang, theme) {
        (Some(l), Some(_th)) => match resolve_syntax(ps, l) {
            Some(syn) => (syn, true),
            None => (ps.find_syntax_plain_text(), false),
        },
        _ => (ps.find_syntax_plain_text(), false),
    };

    let theme = match theme {
        Some(t) => t,
        None => {
            for line in lines {
                writeln!(w, "{DIM}{}{RESET}", line)?;
            }
            return Ok(());
        }
    };

    // 開始フェンス行を DIM で出力
    writeln!(w, "{DIM}{}{RESET}", lines[0])?;

    if lines.len() <= 1 {
        return Ok(());
    }

    let (content_lines, has_closing) = if lines.len() >= 2 && is_fence(lines.last().unwrap(), "") {
        (&lines[1..lines.len() - 1], true)
    } else {
        (&lines[1..], false)
    };

    if do_highlight && !content_lines.is_empty() {
        let mut highlighter = HighlightLines::new(syntax, theme);
        let content = content_lines.join("\n") + "\n";
        for line in LinesWithEndings::from(&content) {
            match highlighter.highlight_line(line, ps) {
                Ok(ranges) => {
                    write!(w, "{}", as_24_bit_terminal_escaped(&ranges[..], false))?;
                }
                Err(_) => {
                    write!(w, "{}", line)?;
                }
            }
            write!(w, "\x1b[0m")?;
        }
    } else {
        for line in content_lines {
            writeln!(w, "{DIM}{}{RESET}", line)?;
        }
    }

    if has_closing {
        writeln!(w, "{DIM}{}{RESET}", lines.last().unwrap())?;
    }

    Ok(())
}

/// 1行分の Markdown を整形して書き出す（見出し・強調・インラインコード・リスト）
fn format_line(line: &str, w: &mut impl Write) -> io::Result<()> {
    let s = line.trim_start();

    // 見出し
    if s.starts_with("### ") {
        writeln!(w, "{GREEN}{BOLD}{}{RESET}", &s[4..])?;
        return Ok(());
    }
    if s.starts_with("## ") {
        writeln!(w, "{YELLOW}{BOLD}{}{RESET}", &s[3..])?;
        return Ok(());
    }
    if s.starts_with("# ") {
        writeln!(w, "{YELLOW}{BOLD}{}{RESET}", &s[2..])?;
        return Ok(());
    }

    // リスト（- または * または N.）
    let list_marker = s.starts_with("- ")
        || s.starts_with("* ")
        || (s.len() >= 3
            && s.chars().next().map_or(false, |c| c.is_ascii_digit())
            && s.get(1..3) == Some(". "));
    if list_marker {
        let rest = if s.starts_with("- ") {
            &s[2..]
        } else if s.starts_with("* ") {
            &s[2..]
        } else if let Some(i) = s.find(". ") {
            &s[i + 2..]
        } else {
            s
        };
        write!(w, "  {DIM}•{RESET} ")?;
        format_inline(rest, w)?;
        return Ok(());
    }

    // 空行
    if s.is_empty() {
        writeln!(w)?;
        return Ok(());
    }

    // 通常行: インライン要素のみ整形
    format_inline(s, w)?;
    Ok(())
}

/// インライン要素（**bold** と `code`）を処理して出力
fn format_inline(s: &str, w: &mut impl Write) -> io::Result<()> {
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        // **bold**
        if i + 2 <= bytes.len() && &bytes[i..i + 2] == b"**" {
            i += 2;
            if let Some(end) = find_subslice(bytes, i, b"**") {
                let _ = std::str::from_utf8(&bytes[i..end]).map(|t| write!(w, "{BOLD}{}{RESET}", t));
                i = end + 2;
                continue;
            }
            write!(w, "**")?;
            continue;
        }
        // `code`
        if bytes[i] == b'`' {
            i += 1;
            if let Some(end) = find_byte(bytes, i, b'`') {
                let _ = std::str::from_utf8(&bytes[i..end]).map(|t| write!(w, "{CYAN}{}{RESET}", t));
                i = end + 1;
                continue;
            }
            write!(w, "`")?;
            continue;
        }
        // 通常文字（UTF-8 の先頭）
        let start = i;
        i += 1;
        while i < bytes.len() && (bytes[i] & 0xC0) == 0x80 {
            i += 1;
        }
        let _ = std::str::from_utf8(&bytes[start..i]).map(|t| write!(w, "{}", t));
    }
    writeln!(w)?;
    Ok(())
}

fn find_subslice(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| start + p)
}

fn find_byte(haystack: &[u8], start: usize, b: u8) -> Option<usize> {
    haystack[start..].iter().position(|&x| x == b).map(|p| start + p)
}
