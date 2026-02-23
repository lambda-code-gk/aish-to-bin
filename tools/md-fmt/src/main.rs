//! 標準入力から逐次入力される Markdown を整形して標準出力に出すフィルタ。
//! ストリーミング入力（例: ai コマンドの出力）を随時処理し、できるだけ随時出力する。

use std::io::{self, BufRead, Write};
use unicode_width::UnicodeWidthStr;
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

/// テーブル枠用（太字＋白以外で境界を判別しやすくする）
const TABLE_FRAME: &str = "\x1b[1;36m"; // BOLD + CYAN

/// ブロック状態（コードブロック / テーブルはバッファしてから出力）
enum BlockState {
    Normal,
    CodeBlock,
    Table,
}

fn main() -> io::Result<()> {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut state = BlockState::Normal;
    let mut code_buf: Vec<String> = Vec::new();
    let mut table_buf: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim_end();

        match &state {
            BlockState::Normal => {
                if fence_start(trimmed).is_some() {
                    state = BlockState::CodeBlock;
                    code_buf.clear();
                    code_buf.push(trimmed.to_string());
                } else if is_table_row(trimmed) {
                    state = BlockState::Table;
                    table_buf.clear();
                    table_buf.push(trimmed.to_string());
                } else {
                    format_line(trimmed, &mut stdout)?;
                    stdout.flush()?;
                }
            }
            BlockState::CodeBlock => {
                code_buf.push(trimmed.to_string());
                if is_fence(trimmed, &code_buf[0]) {
                    output_code_block(&code_buf, &ps, &ts, &mut stdout)?;
                    stdout.flush()?;
                    state = BlockState::Normal;
                    code_buf.clear();
                }
            }
            BlockState::Table => {
                if trimmed.is_empty() || !is_table_row(trimmed) {
                    output_table(&table_buf, &mut stdout)?;
                    stdout.flush()?;
                    state = BlockState::Normal;
                    table_buf.clear();
                    format_line(trimmed, &mut stdout)?;
                    stdout.flush()?;
                } else {
                    table_buf.push(trimmed.to_string());
                }
            }
        }
    }

    if !code_buf.is_empty() {
        output_code_block(&code_buf, &ps, &ts, &mut stdout)?;
        stdout.flush()?;
    }
    if !table_buf.is_empty() {
        output_table(&table_buf, &mut stdout)?;
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
    let (syntax, do_highlight) = match lang {
        Some(l) => match resolve_syntax(ps, l) {
            Some(syn) => (syn, true),
            None => (ps.find_syntax_plain_text(), false),
        },
        None => (ps.find_syntax_plain_text(), false),
    };

    // シンタックスハイライト有効時は通常の明るさのテーマ、それ以外は暗めのテーマ
    let theme = if do_highlight {
        ts.themes
            .get("Solarized (light)")
            .or_else(|| ts.themes.get("InspiredGitHub"))
            .or_else(|| ts.themes.get("base16-ocean.light"))
            .or_else(|| ts.themes.values().next())
    } else {
        ts.themes
            .get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
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

/// 行が GFM テーブル行か（先頭が | で、もう一つ | がある）
fn is_table_row(line: &str) -> bool {
    let s = line.trim_start();
    s.starts_with('|') && s.len() > 1 && s[1..].contains('|')
}

/// テーブル行をセルに分割（前後の | で区切られた部分を trim）
fn parse_table_cells(line: &str) -> Vec<String> {
    let parts: Vec<String> = line.split('|').map(|s| s.trim().to_string()).collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    parts[1..parts.len() - 1].to_vec()
}

/// 区切り行か（各セルが - または : のみで構成）
fn is_separator_row(cells: &[String]) -> bool {
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|c| {
        let t = c.trim();
        !t.is_empty() && t.chars().all(|x| x == '-' || x == ':')
    })
}

/// セルの表示幅（ターミナル列数。全角=2・半角=1。ANSI は考慮しない）
fn cell_width(s: &str) -> usize {
    s.width()
}

/// テーブル行バッファを列幅揃えして出力（枠は BOLD+CYAN、ヘッダは BOLD+YELLOW、データ行に行番号）
fn output_table(lines: &[String], w: &mut impl Write) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let rows: Vec<Vec<String>> = lines.iter().map(|s| parse_table_cells(s)).collect();
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        for line in lines {
            writeln!(w, "{}", line)?;
        }
        return Ok(());
    }
    let mut widths = vec![0usize; ncols];
    for row in &rows {
        if !is_separator_row(row) {
            for (j, cell) in row.iter().take(ncols).enumerate() {
                widths[j] = widths[j].max(cell_width(cell));
            }
        }
    }
    let separator_idx = rows.iter().position(|r| is_separator_row(r));
    let data_start = separator_idx.map(|idx| idx + 1).unwrap_or(1);
    let n_data = rows
        .iter()
        .enumerate()
        .filter(|(i, r)| *i >= data_start && !is_separator_row(r))
        .count();
    let row_num_width = if n_data <= 0 {
        0
    } else {
        format!("{}", n_data).len()
    };

    for (i, row) in rows.iter().enumerate() {
        let is_header = i == 0 && !is_separator_row(row);
        let is_sep = is_separator_row(row);
        let is_data = i >= data_start && !is_sep;

        if is_data {
            let row_num = i - data_start + 1;
            write!(w, " {DIM}{:>width$}. {RESET}", row_num, width = row_num_width)?;
        } else if row_num_width > 0 {
            write!(w, "{}", " ".repeat(row_num_width + 3))?;
        }

        write!(w, "{TABLE_FRAME}|{RESET}")?;
        if is_sep {
            for j in 0..ncols {
                let col_w = widths.get(j).copied().unwrap_or(0).max(1);
                write!(w, " {TABLE_FRAME}{}{RESET} {TABLE_FRAME}|{RESET}", "-".repeat(col_w))?;
            }
        } else if is_header {
            for j in 0..ncols {
                let cell = row.get(j).map(|s| s.as_str()).unwrap_or("");
                let col_w = widths.get(j).copied().unwrap_or(0);
                let len = cell_width(cell);
                let pad_len = col_w.saturating_sub(len);
                write!(w, " {YELLOW}{BOLD}{}{RESET}{} {TABLE_FRAME}|{RESET}", cell, " ".repeat(pad_len))?;
            }
        } else {
            for j in 0..ncols {
                let cell = row.get(j).map(|s| s.as_str()).unwrap_or("");
                let col_w = widths.get(j).copied().unwrap_or(0);
                let len = cell_width(cell);
                let pad_len = col_w.saturating_sub(len);
                write!(w, " {}{} {TABLE_FRAME}|{RESET}", cell, " ".repeat(pad_len))?;
            }
        }
        writeln!(w)?;
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
