//! エントリ: バイナリ名 "aish" 用。aish_cli::run() に委譲するだけ（同一ソース重複ビルド警告回避）。

fn main() {
    let exit_code = match aish_cli::run() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aish: {}", e);
            e.exit_code()
        }
    };
    std::process::exit(exit_code);
}
