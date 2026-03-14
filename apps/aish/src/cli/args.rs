use crate::domain::command::Command;
use clap::builder::ArgAction;
use clap::value_parser;
use clap_complete::Shell;
use common::error::Error;

/// CLI から受け取った生の設定（command は文字列のまま保持）
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub help: bool,
    pub session_dir: Option<String>,
    pub home_dir: Option<String>,
    /// -v / --verbose: 不具合調査用の冗長ログを出力する
    pub verbose: bool,
    /// コマンド名（None の場合は Shell）
    pub command_name: Option<String>,
    pub command_args: Vec<String>,
    /// aish init 用（サブコマンド init のときのみ有効）
    pub init_force: bool,
    pub init_dry_run: bool,
    pub init_defaults_dir: Option<String>,
    /// aish sessions rebuild-derived の --session で指定した id
    pub sessions_rebuild_derived_session_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            help: false,
            session_dir: None,
            home_dir: None,
            verbose: false,
            command_name: None,
            command_args: Vec::new(),
            init_force: false,
            init_dry_run: false,
            init_defaults_dir: None,
            sessions_rebuild_derived_session_id: None,
        }
    }
}

/// 解析結果: 通常の Config または補完スクリプト生成
#[derive(Debug, Clone)]
pub enum ParseOutcome {
    Config(Config),
    GenerateCompletion(Shell),
}

fn global_args(cmd: clap::Command) -> clap::Command {
    cmd.disable_help_flag(true)
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .help("Print help")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("session-dir")
                .short('s')
                .long("session-dir")
                .value_name("directory")
                .help("Specify a session directory (for resume)")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("home-dir")
                .short('d')
                .long("home-dir")
                .value_name("directory")
                .help("Specify a home directory (sets AISH_HOME for this process)")
                .num_args(1),
        )
        .arg(
            clap::Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Emit verbose debug logs (for troubleshooting)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("generate")
                .long("generate")
                .value_name("shell")
                .help("Generate shell completion script")
                .value_parser(value_parser!(Shell))
                .num_args(1),
        )
}

fn build_memory_subcommand() -> clap::Command {
    clap::Command::new("memory")
        .about("Memory list / get / remove (persistent knowledge used by ai)")
        .subcommand_required(true)
        .subcommand(clap::Command::new("list").about("List all memories (id, category, subject)"))
        .subcommand(
            clap::Command::new("get")
                .about("Get memory content by ID(s)")
                .arg(
                    clap::Arg::new("ids")
                        .num_args(1..)
                        .value_name("id")
                        .required(true),
                ),
        )
        .subcommand(
            clap::Command::new("remove")
                .about("Remove memory by ID(s)")
                .arg(
                    clap::Arg::new("ids")
                        .num_args(1..)
                        .value_name("id")
                        .required(true),
                ),
        )
}

fn build_history_subcommand() -> clap::Command {
    clap::Command::new("history")
        .about("List or get reviewed conversation history (requires -s/-d or AISH_SESSION)")
        .subcommand_required(true)
        .subcommand(
            clap::Command::new("ls")
                .about("List reviewed entries: <id> <datetime> <first line> (first line truncated at 20 chars)")
                .arg(
                    clap::Arg::new("all")
                        .long("all")
                        .help("Show all entries (ignore .history_send_from)")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    clap::Arg::new("assistant")
                        .short('a')
                        .long("assistant")
                        .help("Show only assistant role entries")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    clap::Arg::new("user")
                        .short('u')
                        .long("user")
                        .help("Show only user role entries")
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            clap::Command::new("get")
                .about("Get reviewed content by ID(s)")
                .arg(clap::Arg::new("ids").num_args(1..).value_name("id").required(true)),
        )
}

fn build_clap_command() -> clap::Command {
    let memory = build_memory_subcommand();
    let history = build_history_subcommand();

    global_args(
        clap::Command::new("aish")
            .about("CUI automation framework with LLM integration")
            .subcommand_required(false)
            .subcommand(
                clap::Command::new("shell")
                    .about("Start the interactive shell (default) or show shell status")
                    .subcommand_required(false)
                    .subcommand(
                        clap::Command::new("status")
                            .about("Show shell attachment and console log status"),
                    ),
            )
            .subcommand(
                clap::Command::new("plugins")
                    .about("External plugins (discovery is deny-by-default; enable explicitly in plugin.toml)")
                    .subcommand_required(false)
                    .subcommand(clap::Command::new("list").about("List discovered plugins (enabled/disabled)")),
            )
            .subcommand(
                clap::Command::new("tools")
                    .about("External tools provided by enabled plugins")
                    .subcommand_required(false)
                    .subcommand(clap::Command::new("list").about("List tools (canonical id: namespace.tool)")),
            )
            .subcommand(
                clap::Command::new("truncate_console_log")
                    .about("Truncate console buffer and log file (used by ai command)"),
            )
            .subcommand(
                clap::Command::new("rollout")
                    .about("Flush console buffer and rollover console log (SIGUSR1 equivalent)"),
            )
            .subcommand(
                clap::Command::new("mute")
                    .about("Rollout console log and stop recording console.txt"),
            )
            .subcommand(
                clap::Command::new("unmute")
                    .about("Resume recording console.txt"),
            )
            .subcommand(
                clap::Command::new("clear")
                    .about("Clear all part files in the session directory (delete conversation history)"),
            )
            .subcommand(memory)
            .subcommand(history)
            .subcommand(
                clap::Command::new("policy")
                    .about("Policy-related commands (explain: show resolved policy and examples)")
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("explain")
                            .about("Show resolved policy, rule order, and examples (calls ai --policy-explain)"),
                    ),
            )
            .subcommand(
                clap::Command::new("config")
                    .about("Config-related commands (explain: show resolved config and sources)")
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("explain")
                            .about("Show resolved config and sources (calls ai --config-explain)"),
                    ),
            )
            .subcommand(
                clap::Command::new("resume")
                    .about("Resume last or specified session")
                    .arg(
                        clap::Arg::new("id")
                            .value_name("id")
                            .help("Session id to resume (omit for latest)")
                            .num_args(0..=1),
                    ),
            )
            .subcommand(
                clap::Command::new("sessions")
                    .about("List sessions or rebuild derived artifacts")
                    .subcommand(
                        clap::Command::new("rebuild-derived")
                    .about("Rebuild index.sqlite and snapshots/summary.json from events.jsonl")
                            .arg(
                                clap::Arg::new("session")
                                    .long("session")
                                    .value_name("id")
                                    .help("Session id (omit to use -s/--session-dir or AISH_SESSION)")
                                    .num_args(1),
                            ),
                    ),
            )
            .subcommand(
                clap::Command::new("init")
                    .about("Copy default config from AISH_DEFAULTS_DIR or --defaults-dir to config directory")
                    .arg(
                        clap::Arg::new("force")
                            .long("force")
                            .help("Overwrite existing files")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(
                        clap::Arg::new("dry-run")
                            .long("dry-run")
                            .help("Only print what would be copied")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(
                        clap::Arg::new("defaults-dir")
                            .long("defaults-dir")
                            .value_name("directory")
                            .help("Template root (default: AISH_DEFAULTS_DIR env)")
                            .num_args(1),
                    ),
            )
            .subcommand(
                clap::Command::new("daemon")
                    .about("Single-writer daemon (aishd): start, ping, status, jobs, stop, cancel")
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("start")
                            .about("Run daemon in foreground (Ctrl-C to stop)")
                            .arg(
                                clap::Arg::new("detach")
                                    .long("detach")
                                    .help("Start daemon in background (detach) and return immediately")
                                    .action(ArgAction::SetTrue),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("ensure")
                            .about("Ensure daemon is running (start in background if needed)"),
                    )
                    .subcommand(clap::Command::new("ping").about("Check daemon liveness"))
                    .subcommand(clap::Command::new("status").about("Show daemon status (same as ping)"))
                    .subcommand(
                        clap::Command::new("jobs")
                            .about("List backend jobs and/or persisted lifecycle state")
                            .arg(
                                clap::Arg::new("active")
                                    .long("active")
                                    .help("Show only active jobs from the running daemon")
                                    .action(ArgAction::SetTrue),
                            )
                            .arg(
                                clap::Arg::new("persisted")
                                    .long("persisted")
                                    .help("Show only persisted lifecycle rows from the selected session")
                                    .action(ArgAction::SetTrue),
                            ),
                    )
                    .subcommand(clap::Command::new("stop").about("Ask the daemon to stop and clean up"))
                    .subcommand(
                        clap::Command::new("cancel")
                            .about("Cancel a running ai backend job")
                            .arg(
                                clap::Arg::new("job_id")
                                    .value_name("job_id")
                                    .required(true)
                                    .num_args(1),
                            ),
                    ),
            ),
    )
}

fn matches_to_config(matches: &clap::ArgMatches) -> Config {
    let help = matches.get_flag("help");
    let session_dir = matches.get_one::<String>("session-dir").cloned();
    let home_dir = matches.get_one::<String>("home-dir").cloned();
    let verbose = matches.get_flag("verbose");

    let (
        command_name,
        command_args,
        init_force,
        init_dry_run,
        init_defaults_dir,
        sessions_rebuild_derived_session_id,
    ) = match matches.subcommand() {
        None => (None, Vec::new(), false, false, None, None),
        Some(("shell", m)) => {
            let args = match m.subcommand() {
                Some(("status", _)) => vec!["status".to_string()],
                _ => vec![],
            };
            if args.is_empty() {
                (None, Vec::new(), false, false, None, None)
            } else {
                (Some("shell".to_string()), args, false, false, None, None)
            }
        }
        Some(("plugins", m)) => {
            let sub = m
                .subcommand()
                .map(|(n, _)| n.to_string())
                .unwrap_or_else(|| "list".to_string());
            (
                Some("plugins".to_string()),
                vec![sub],
                false,
                false,
                None,
                None,
            )
        }
        Some(("tools", m)) => {
            let sub = m
                .subcommand()
                .map(|(n, _)| n.to_string())
                .unwrap_or_else(|| "list".to_string());
            (
                Some("tools".to_string()),
                vec![sub],
                false,
                false,
                None,
                None,
            )
        }
        Some(("truncate_console_log", _)) => (
            Some("truncate_console_log".to_string()),
            vec![],
            false,
            false,
            None,
            None,
        ),
        Some(("rollout", _)) => (
            Some("rollout".to_string()),
            vec![],
            false,
            false,
            None,
            None,
        ),
        Some(("mute", _)) => (Some("mute".to_string()), vec![], false, false, None, None),
        Some(("unmute", _)) => (Some("unmute".to_string()), vec![], false, false, None, None),
        Some(("clear", _)) => (Some("clear".to_string()), vec![], false, false, None, None),
        Some(("resume", m)) => {
            let id = m.get_one::<String>("id").cloned();
            let args = id.into_iter().collect::<Vec<_>>();
            (Some("resume".to_string()), args, false, false, None, None)
        }
        Some(("init", m)) => (
            Some("init".to_string()),
            vec![],
            m.get_flag("force"),
            m.get_flag("dry-run"),
            m.get_one::<String>("defaults-dir").cloned(),
            None,
        ),
        Some(("memory", memory_m)) => {
            let (sub, args) = match memory_m.subcommand() {
                Some(("list", _)) => ("list", vec![]),
                Some(("get", m)) => (
                    "get",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                Some(("remove", m)) => (
                    "remove",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                _ => ("", vec![]),
            };
            let mut command_args = vec![sub.to_string()];
            command_args.extend(args);
            (
                Some("memory".to_string()),
                command_args,
                false,
                false,
                None,
                None,
            )
        }
        Some(("history", history_m)) => {
            let (sub, args) = match history_m.subcommand() {
                Some(("ls", m)) => {
                    let mut a = vec!["ls".to_string()];
                    if m.get_flag("all") {
                        a.push("--all".to_string());
                    }
                    if m.get_flag("assistant") {
                        a.push("-a".to_string());
                    }
                    if m.get_flag("user") {
                        a.push("-u".to_string());
                    }
                    ("ls", a)
                }
                Some(("get", m)) => (
                    "get",
                    m.get_many::<String>("ids")
                        .map(|i| i.cloned().collect())
                        .unwrap_or_default(),
                ),
                _ => ("", vec![]),
            };
            let mut command_args = vec![sub.to_string()];
            command_args.extend(args);
            (
                Some("history".to_string()),
                command_args,
                false,
                false,
                None,
                None,
            )
        }
        Some(("policy", policy_m)) => {
            let (sub, args) = match policy_m.subcommand() {
                Some(("explain", _)) => ("explain", vec![]),
                _ => ("", vec![]),
            };
            let mut command_args = if sub.is_empty() {
                vec![]
            } else {
                vec![sub.to_string()]
            };
            command_args.extend(args);
            (
                Some("policy".to_string()),
                command_args,
                false,
                false,
                None,
                None,
            )
        }
        Some(("sessions", sessions_m)) => {
            let (cmd_name, cmd_args, rebuild_session_id) = match sessions_m.subcommand() {
                Some(("rebuild-derived", m2)) => (
                    Some("sessions".to_string()),
                    vec!["rebuild-derived".to_string()],
                    m2.get_one::<String>("session").cloned(),
                ),
                _ => (Some("sessions".to_string()), vec![], None),
            };
            (cmd_name, cmd_args, false, false, None, rebuild_session_id)
        }
        Some(("daemon", daemon_m)) => {
            let args = match daemon_m.subcommand() {
                Some(("start", m)) => {
                    let mut args = vec!["start".to_string()];
                    if m.get_flag("detach") {
                        args.push("--detach".to_string());
                    }
                    args
                }
                Some(("ensure", _)) => vec!["ensure".to_string()],
                Some(("ping", _)) => vec!["ping".to_string()],
                Some(("status", _)) => vec!["status".to_string()],
                Some(("jobs", m)) => {
                    let mut args = vec!["jobs".to_string()];
                    if m.get_flag("active") {
                        args.push("--active".to_string());
                    }
                    if m.get_flag("persisted") {
                        args.push("--persisted".to_string());
                    }
                    args
                }
                Some(("stop", _)) => vec!["stop".to_string()],
                Some(("cancel", m)) => {
                    let mut args = vec!["cancel".to_string()];
                    if let Some(job_id) = m.get_one::<String>("job_id") {
                        args.push(job_id.clone());
                    }
                    args
                }
                _ => vec![],
            };
            (Some("daemon".to_string()), args, false, false, None, None)
        }
        Some((name, _)) => (Some(name.to_string()), vec![], false, false, None, None),
    };

    Config {
        help,
        session_dir,
        home_dir,
        verbose,
        command_name,
        command_args,
        init_force,
        init_dry_run,
        init_defaults_dir,
        sessions_rebuild_derived_session_id,
    }
}

/// コマンドラインを解析する。補完生成が要求された場合は ParseOutcome::GenerateCompletion を返す。
pub fn parse_args() -> Result<ParseOutcome, Error> {
    parse_args_from_os(std::env::args_os())
}

/// 外部から引数イテレータで解析する（bins/aish-cli の aish サブコマンド用）
pub fn parse_args_from_os(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Result<ParseOutcome, Error> {
    let args: Vec<std::ffi::OsString> = args.into_iter().map(|a| a.as_ref().to_owned()).collect();
    let cmd = build_clap_command();
    let matches = cmd
        .try_get_matches_from(args)
        .map_err(|e| Error::invalid_argument(e.to_string()))?;

    if let Some(&shell) = matches.get_one::<Shell>("generate") {
        return Ok(ParseOutcome::GenerateCompletion(shell));
    }

    Ok(ParseOutcome::Config(matches_to_config(&matches)))
}

/// 補完スクリプトを標準出力に出力する。
/// 注: clap_complete::generate は当コマンド構成でパニックするため、簡易フォールバックを常に使用する。
pub fn print_completion(shell: Shell) {
    emit_fallback_completion(shell);
}

fn emit_fallback_completion(shell: Shell) {
    let subcommands = "clear history init memory mute unmute policy resume rollout sessions shell truncate_console_log plugins tools";
    let global_opts = "-h --help -s --session-dir -d --home-dir -v --verbose --generate";
    let memory_subs = "list get remove";
    let history_subs = "ls get";
    let history_ls_opts = "--all -a --assistant -u --user";
    let init_opts = "--force --dry-run --defaults-dir";
    let generate_shells = "bash zsh fish";

    match shell {
        Shell::Bash => {
            println!(
                r#"# Fallback completion for aish (subcommands + options, dirs for -s/-d/--defaults-dir, history ls opts)
_aish() {{
  local cur="${{COMP_WORDS[COMP_CWORD]}}"
  local prev="${{COMP_WORDS[COMP_CWORD-1]}}"
  local words=("${{COMP_WORDS[@]}}")
  local cword=$COMP_CWORD

  if [[ "$prev" == "--generate" ]]; then
    COMPREPLY=($(compgen -W "{generate_shells}" -- "$cur"))
  elif [[ "$prev" == "-s" || "$prev" == "--session-dir" || "$prev" == "-d" || "$prev" == "--home-dir" ]]; then
    compopt -o filenames 2>/dev/null
    COMPREPLY=($(compgen -d -S / -- "$cur"))
  elif [[ "$prev" == "--defaults-dir" ]]; then
    compopt -o filenames 2>/dev/null
    COMPREPLY=($(compgen -d -S / -- "$cur"))
  elif (( cword == 1 )); then
    COMPREPLY=($(compgen -W "{subcommands} {global_opts}" -- "$cur"))
  elif (( cword == 2 )); then
    case "${{words[1]}}" in
      memory)  COMPREPLY=($(compgen -W "{memory_subs}" -- "$cur")) ;;
      history) COMPREPLY=($(compgen -W "{history_subs}" -- "$cur")) ;;
      plugins) COMPREPLY=($(compgen -W "list" -- "$cur")) ;;
      tools)   COMPREPLY=($(compgen -W "list" -- "$cur")) ;;
      resume)  COMPREPLY=($(compgen -W "$(aish sessions 2>/dev/null)" -- "$cur")) ;;
      init)    COMPREPLY=($(compgen -W "{init_opts}" -- "$cur")) ;;
      *)       COMPREPLY=() ;;
    esac
  elif (( cword >= 3 )); then
    if [[ "${{words[1]}}" == "memory" ]]; then
      if [[ "${{words[2]}}" == "get" || "${{words[2]}}" == "remove" ]]; then
        COMPREPLY=($(compgen -W "$(aish memory list 2>/dev/null | awk '{{print $1}}')" -- "$cur"))
      fi
    elif [[ "${{words[1]}}" == "history" ]]; then
      if [[ "${{words[2]}}" == "ls" ]]; then
        COMPREPLY=($(compgen -W "{history_ls_opts}" -- "$cur"))
      elif [[ "${{words[2]}}" == "get" ]]; then
        COMPREPLY=($(compgen -W "$(aish history ls 2>/dev/null | cut -f1)" -- "$cur"))
      fi
    elif [[ "${{words[1]}}" == "init" ]]; then
      if [[ "$prev" != "--defaults-dir" ]]; then
        COMPREPLY=($(compgen -W "{init_opts}" -- "$cur"))
      fi
    fi
  fi
}}
complete -F _aish aish
"#,
                subcommands = subcommands,
                global_opts = global_opts,
                memory_subs = memory_subs,
                history_subs = history_subs,
                history_ls_opts = history_ls_opts,
                init_opts = init_opts,
                generate_shells = generate_shells
            );
        }
        Shell::Zsh => {
            let subcommands_zsh = subcommands
                .split_whitespace()
                .chain(global_opts.split_whitespace())
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                r#"# Fallback completion for aish (subcommands + options, dirs for -s/-d/--defaults-dir, history ls opts)
#compdef aish
local cur="${{words[CURRENT]}}"
local prev="${{words[CURRENT-1]}}"
local -a reply
if [[ "$prev" == --generate ]]; then
  reply=(bash zsh fish)
elif [[ "$prev" == -s || "$prev" == --session-dir || "$prev" == -d || "$prev" == --home-dir || "$prev" == --defaults-dir ]]; then
  _files -/
elif (( CURRENT == 2 )); then
  reply=({subcommands_zsh})
elif (( CURRENT == 3 )); then
  case "${{words[2]}}" in
    memory)  reply=(list get remove) ;;
    history) reply=(ls get) ;;
    resume)  reply=($(aish sessions 2>/dev/null)) ;;
    init)    reply=(--force --dry-run --defaults-dir) ;;
    *)       reply=() ;;
  esac
elif (( CURRENT >= 4 )); then
  if [[ "${{words[2]}}" == memory && ( "${{words[3]}}" == get || "${{words[3]}}" == remove ) ]]; then
    reply=($(aish memory list 2>/dev/null | awk '{{print $1}}'))
  elif [[ "${{words[2]}}" == history && "${{words[3]}}" == get ]]; then
    reply=($(aish history ls 2>/dev/null | cut -f1))
  elif [[ "${{words[2]}}" == history && "${{words[3]}}" == ls ]]; then
    reply=(--all -a --assistant -u --user)
  elif [[ "${{words[2]}}" == init ]]; then
    [[ "$prev" != --defaults-dir ]] && reply=(--force --dry-run --defaults-dir)
  else
    reply=()
  fi
else
  reply=()
fi
[[ -n $reply ]] && _describe 'aish' reply
"#,
                subcommands_zsh = subcommands_zsh
            );
        }
        Shell::Fish => {
            println!(
                r#"# Fallback completion for aish (subcommands + options, dirs for -s/-d/--defaults-dir, history ls opts)
complete -c aish -l help -s h -d "Print help"
complete -c aish -l session-dir -s s -d "Session directory" -r -a "(__fish_complete_directories)"
complete -c aish -l home-dir -s d -d "Home directory" -r -a "(__fish_complete_directories)"
complete -c aish -l verbose -s v -d "Verbose debug logs"
complete -c aish -l generate -d "Generate completion script" -r -a "bash zsh fish"
complete -c aish -l force -d "Overwrite existing files" -n "__fish_seen_subcommand_from init"
complete -c aish -l dry-run -d "Only print what would be copied" -n "__fish_seen_subcommand_from init"
complete -c aish -l defaults-dir -d "Template root" -r -a "(__fish_complete_directories)" -n "__fish_seen_subcommand_from init"
complete -c aish -a "clear" -d "Clear part files in session"
complete -c aish -a "history" -d "List or get conversation history"
complete -c aish -a "init" -d "Copy default config"
complete -c aish -a "memory" -d "Memory list / get / remove"
complete -c aish -a "mute" -d "Stop recording console.txt"
complete -c aish -a "unmute" -d "Resume recording console.txt"
complete -c aish -a "resume" -d "Resume session"
complete -c aish -a "rollout" -d "Flush and rollover console log"
complete -c aish -a "sessions" -d "List sessions"
complete -c aish -a "shell" -d "Start interactive shell (default)"
complete -c aish -a "truncate_console_log" -d "Truncate console buffer and log"
complete -c aish -a "(aish sessions 2>/dev/null)" -n "__fish_seen_subcommand_from resume"
complete -c aish -a "(aish memory list 2>/dev/null | awk '{{print $1}}')" -n "__fish_seen_subcommand_from memory; and (__fish_seen_subcommand_from get or __fish_seen_subcommand_from remove)"
complete -c aish -a "list get remove" -n "__fish_seen_subcommand_from memory; and not __fish_seen_subcommand_from get; and not __fish_seen_subcommand_from remove"
complete -c aish -a "(aish history ls 2>/dev/null | cut -f1)" -n "__fish_seen_subcommand_from history; and __fish_seen_subcommand_from get"
complete -c aish -a "ls get" -n "__fish_seen_subcommand_from history; and not __fish_seen_subcommand_from get; and not __fish_seen_subcommand_from ls"
complete -c aish -a "--all -a --assistant -u --user" -n "__fish_seen_subcommand_from history; and __fish_seen_subcommand_from ls"
complete -c aish -a "--force --dry-run --defaults-dir" -n "__fish_seen_subcommand_from init"
"#
            );
        }
        _ => {}
    }
}

/// Config を Command に変換する
pub fn config_to_command(config: &Config) -> Command {
    if config.help {
        return Command::Help;
    }
    match &config.command_name {
        Some(name) if name == "init" => Command::Init {
            force: config.init_force,
            dry_run: config.init_dry_run,
            defaults_dir: config.init_defaults_dir.clone(),
        },
        Some(name)
            if name == "sessions"
                && config.command_args.first().map(|s| s.as_str()) == Some("rebuild-derived") =>
        {
            Command::SessionsRebuildDerived {
                session_id: config.sessions_rebuild_derived_session_id.clone(),
            }
        }
        Some(name) => Command::parse_with_args(name, &config.command_args),
        None => Command::Shell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_flag() {
        let config = Config::default();
        assert!(!config.help);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(!config.help);
        assert_eq!(config.session_dir, None);
        assert_eq!(config.home_dir, None);
        assert!(!config.verbose);
        assert_eq!(config.command_name, None);
        assert_eq!(config.command_args.len(), 0);
    }

    #[test]
    fn test_config_to_command_default_is_shell() {
        let config = Config::default();
        assert_eq!(config_to_command(&config), Command::Shell);
    }

    #[test]
    fn test_config_to_command_help() {
        let config = Config {
            help: true,
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Help);
    }

    #[test]
    fn test_config_to_command_with_command_name() {
        let config = Config {
            command_name: Some("truncate_console_log".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::TruncateConsoleLog);
    }

    #[test]
    fn test_config_to_command_with_mute() {
        let config = Config {
            command_name: Some("mute".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Mute);
    }

    #[test]
    fn test_config_to_command_with_shell_status() {
        let config = Config {
            command_name: Some("shell".to_string()),
            command_args: vec!["status".to_string()],
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::ShellStatus);
    }

    #[test]
    fn test_config_to_command_with_unmute() {
        let config = Config {
            command_name: Some("unmute".to_string()),
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::Unmute);
    }

    #[test]
    fn test_config_to_command_with_daemon_stop() {
        let config = Config {
            command_name: Some("daemon".to_string()),
            command_args: vec!["stop".to_string()],
            ..Default::default()
        };
        assert_eq!(config_to_command(&config), Command::DaemonStop);
    }

    #[test]
    fn test_config_to_command_with_daemon_jobs() {
        let config = Config {
            command_name: Some("daemon".to_string()),
            command_args: vec!["jobs".to_string()],
            ..Default::default()
        };
        assert_eq!(
            config_to_command(&config),
            Command::DaemonJobs {
                active_only: false,
                persisted_only: false,
            }
        );
    }

    #[test]
    fn test_config_to_command_with_daemon_jobs_filters() {
        let config = Config {
            command_name: Some("daemon".to_string()),
            command_args: vec![
                "jobs".to_string(),
                "--active".to_string(),
                "--persisted".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(
            config_to_command(&config),
            Command::DaemonJobs {
                active_only: true,
                persisted_only: true,
            }
        );
    }

    #[test]
    fn test_config_to_command_with_daemon_cancel() {
        let config = Config {
            command_name: Some("daemon".to_string()),
            command_args: vec!["cancel".to_string(), "job-1".to_string()],
            ..Config::default()
        };
        assert_eq!(
            config_to_command(&config),
            Command::DaemonCancel {
                job_id: "job-1".to_string()
            }
        );
    }
}
