//! Phase 8: single binary aish with subcommands. Wiring only; delegates to ai and aish crates.

use std::ffi::OsString;

/// メイン処理。bin エントリ（ai / aish）から呼ばれる。
pub fn run() -> Result<i32, common::error::Error> {
    use clap::builder::ArgAction;

    let mut args: Vec<OsString> = std::env::args_os().collect();
    if invoked_as_ai() && args.len() >= 1 {
        args.insert(1, OsString::from("ai"));
    }
    let args_for_ai = args.clone();

    let cmd = clap::Command::new("aish")
        .about("CUI automation framework with LLM integration")
        .disable_help_flag(true)
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("session-dir")
                .short('s')
                .long("session-dir")
                .value_name("directory")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("home-dir")
                .short('d')
                .long("home-dir")
                .value_name("directory")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("generate")
                .long("generate")
                .num_args(1),
        )
        .subcommand_required(false)
        .subcommand(
            clap::Command::new("ai")
                .about("Run ai (LLM query, task, policy-explain, etc.)")
                .trailing_var_arg(true)
                .arg(clap::Arg::new("ai_rest").num_args(0..).allow_hyphen_values(true)),
        )
        .subcommand(clap::Command::new("shell").about("Start the interactive shell (default)"))
        .subcommand(
            clap::Command::new("plugins")
                .about("External plugins (aish plugins list)")
                .trailing_var_arg(true)
                .arg(clap::Arg::new("plugins_rest").num_args(0..).allow_hyphen_values(true)),
        )
        .subcommand(
            clap::Command::new("tools")
                .about("External tools (aish tools list)")
                .trailing_var_arg(true)
                .arg(clap::Arg::new("tools_rest").num_args(0..).allow_hyphen_values(true)),
        )
        .subcommand(clap::Command::new("sessions").about("List sessions or rebuild-derived"))
        .subcommand(clap::Command::new("policy").about("Policy explain (ai --policy-explain)"))
        .subcommand(clap::Command::new("config").about("Config explain (ai --config-explain)"))
        .subcommand(clap::Command::new("clear").about("Clear part files in session"))
        .subcommand(clap::Command::new("init").about("Copy default config"))
        .subcommand(clap::Command::new("memory").about("Memory list/get/remove"))
        .subcommand(clap::Command::new("history").about("History ls/get"))
        .subcommand(clap::Command::new("resume").about("Resume session"))
        .subcommand(clap::Command::new("rollout").about("Rollover console log"))
        .subcommand(clap::Command::new("mute").about("Stop recording console"))
        .subcommand(clap::Command::new("unmute").about("Resume recording console"))
        .subcommand(clap::Command::new("truncate_console_log").about("Truncate console buffer"))
        .subcommand(
            clap::Command::new("daemon")
                .about("Single-writer daemon (aishd): start, ping, status")
                .subcommand_required(true)
                .subcommand(clap::Command::new("start").about("Run daemon in foreground"))
                .subcommand(clap::Command::new("ping").about("Check daemon liveness"))
                .subcommand(clap::Command::new("status").about("Show daemon status")),
        );

    let matches = cmd.get_matches_from(args);

    if matches.get_flag("help") {
        print_help();
        return Ok(0);
    }

    if let Some(shell) = matches.get_one::<String>("generate") {
        let argv = vec![
            OsString::from("aish"),
            OsString::from("--generate"),
            OsString::from(shell),
        ];
        return aish::run_with_args(argv);
    }

    let global_args = build_global_argv(&matches);
    match matches.subcommand() {
        Some(("ai", _)) => {
            let mut argv = vec![OsString::from("ai")];
            argv.extend(args_for_ai.into_iter().skip(2));
            return ai::run_with_args(argv);
        }
        Some((sub_name, _)) => {
            let mut argv = vec![OsString::from("aish")];
            argv.extend(global_args);
            argv.push(OsString::from(sub_name));
            let raw: Vec<OsString> = std::env::args_os().collect();
            let sub_os = OsString::from(sub_name);
            let mut iter = raw.iter().skip(1);
            while let Some(a) = iter.next() {
                if a == &sub_os {
                    for x in iter {
                        argv.push(x.clone());
                    }
                    break;
                }
            }
            aish::run_with_args(argv)
        }
        None => {
            if matches.get_flag("help") {
                print_help();
                Ok(0)
            } else {
                // No subcommand: run as "aish" (shell) for backward compatibility
                let mut argv = vec![OsString::from("aish")];
                argv.extend(global_args);
                aish::run_with_args(argv)
            }
        }
    }
}

/// argv[0] のベース名が "ai" なら true（互換入口: ai として起動されたら aish ai にフォワード）
fn invoked_as_ai() -> bool {
    std::env::args_os()
        .next()
        .and_then(|a| a.into_string().ok())
        .and_then(|s| std::path::Path::new(&s).file_stem().map(|st| st == "ai"))
        .unwrap_or(false)
}

fn build_global_argv(matches: &clap::ArgMatches) -> Vec<OsString> {
    let mut v = Vec::new();
    if let Some(d) = matches.get_one::<String>("session-dir") {
        v.push(OsString::from("-s"));
        v.push(OsString::from(d));
    }
    if let Some(d) = matches.get_one::<String>("home-dir") {
        v.push(OsString::from("-d"));
        v.push(OsString::from(d));
    }
    if matches.get_flag("verbose") {
        v.push(OsString::from("-v"));
    }
    v
}

fn print_help() {
    println!("Usage: aish [options] <subcommand> [args...]");
    println!("  -h, --help            Show this help");
    println!("  -s, --session-dir     Session directory (resume)");
    println!("  -d, --home-dir        Home directory (AISH_HOME)");
    println!("  -v, --verbose         Verbose logs");
    println!("  --generate <shell>    Generate shell completion script (bash, zsh, fish)");
    println!();
    println!("Subcommands:");
    println!("  ai                    Run ai (query, task, --policy-explain, etc.)");
    println!("  shell                 Start interactive shell");
    println!("  plugins               List external plugins (aish plugins list)");
    println!("  tools                 List external tools (aish tools list)");
    println!("  sessions              List sessions or rebuild-derived");
    println!("  policy                Policy explain");
    println!("  config                Config explain");
    println!("  clear, init, memory, history, resume, rollout, mute, unmute, truncate_console_log");
    println!("  daemon start|ping|status   Single-writer daemon (aishd)");
}
