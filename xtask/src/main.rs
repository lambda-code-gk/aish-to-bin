//! Phase 8: xtask dist — dist/bin に aish と ai を配置（P8-6）
//!
//! ビルド: aish-cli（subcommand 統合 + 互換 ai）。成果物: dist/bin/aish, dist/bin/ai。

use std::process::Command;
use std::fs;
use std::io;

fn main() {
    if let Err(e) = run() {
        eprintln!("xtask: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let sub = args.next().and_then(|a| a.into_string().ok());
    match sub.as_deref() {
        Some("dist") => run_dist(args)?,
        _ => {
            eprintln!("Usage: cargo run -p xtask -- dist [--debug]");
            return Err("expected 'dist'".into());
        }
    }
    Ok(())
}

fn run_dist(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), String> {
    let release = !args.any(|a| a == "--debug" || a == "-d");
    let root = std::env::current_dir().map_err(|e| format!("current_dir: {}", e))?;
    let target_name = if release { "release" } else { "debug" };
    let target_dir = root.join("target").join(target_name);
    let dist_bin = root.join("dist").join("bin");

    println!("Building aish-cli ({})...", target_name);
    let status = Command::new("cargo")
        .args(["build", if release { "--release" } else { "" }, "-p", "aish-cli"].iter().filter(|s| !s.is_empty()))
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cargo build: {}", e))?;
    if !status.success() {
        return Err("cargo build -p aish-cli failed".into());
    }

    fs::create_dir_all(&dist_bin).map_err(|e| format!("create_dir_all {}: {}", dist_bin.display(), e))?;

    let mut copied = 0;
    for (bin, name) in [("aish", "aish"), ("ai", "ai")] {
        let src = target_dir.join(bin);
        let dst = dist_bin.join(name);
        if src.exists() {
            // 一時ファイルに書き込んでから rename で置換する。実行中のバイナリは
            // 開いた inode を保持するため、直接上書き(ETXTBSY)を避けられる。
            let tmp = dist_bin.join(format!("{}.new", name));
            fs::copy(&src, &tmp).map_err(|e: io::Error| format!("copy {} -> {}: {}", src.display(), tmp.display(), e))?;
            fs::rename(&tmp, &dst).map_err(|e: io::Error| format!("rename {} -> {}: {}", tmp.display(), dst.display(), e))?;
            println!("  {} -> {}", src.display(), dst.display());
            copied += 1;
        } else {
            eprintln!("  [warn] {} not found (expected at {})", bin, src.display());
        }
    }

    if copied == 0 {
        return Err("no binaries copied".into());
    }
    println!("Dist complete: {} binary(ies) in {}/", copied, dist_bin.display());
    Ok(())
}
