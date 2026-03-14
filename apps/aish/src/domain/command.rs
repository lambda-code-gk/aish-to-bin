//! aish コマンドの enum（Command Pattern）
//!
//! 引数解析の結果を enum に落とし、match でディスパッチする。

/// aish のサブコマンド
///
/// コマンドなし = 対話シェル起動。それ以外は文字列から解析。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// ヘルプ表示
    Help,

    /// 対話シェルを起動（コマンド未指定時）
    Shell,

    /// shell attachment / console log 周辺の状態表示
    ShellStatus,

    /// 実装済み: コンソールバッファ・ログのロールオーバー
    TruncateConsoleLog,

    /// 実装済み: セッションのクリア
    Clear,

    /// コンソールログのロールアウト（SIGUSR1 相当）
    Rollout,

    /// コンソールログの記録を停止（rollout 後に mute）
    Mute,

    /// コンソールログの記録を再開
    Unmute,

    /// セッション再開（resume [<id>]）
    Resume { id: Option<String> },

    /// セッション一覧
    Sessions,
    /// セッションの派生物を再生成（sessions rebuild-derived [--session <id>]）
    SessionsRebuildDerived { session_id: Option<String> },

    /// 初期設定の展開（init [--force] [--dry-run] [--defaults-dir DIR]）
    Init {
        force: bool,
        dry_run: bool,
        defaults_dir: Option<String>,
    },

    /// メモリ一覧（memory list）
    MemoryList,
    /// メモリ取得（memory get id [id...]）
    MemoryGet { ids: Vec<String> },
    /// メモリ削除（memory remove id [id...]）
    MemoryRemove { ids: Vec<String> },

    /// reviewed 履歴一覧（history ls [--all][-a][-u]）
    HistoryLs {
        all: bool,
        user_only: bool,
        assistant_only: bool,
    },
    /// reviewed 履歴取得（history get <id> ...）
    HistoryGet { ids: Vec<String> },

    /// policy explain（ai --policy-explain を実行）
    PolicyExplain,
    /// config explain（ai --config-explain を実行）
    ConfigExplain,

    /// 外部プラグイン一覧（plugins list）
    PluginsList,
    /// 外部ツール一覧（tools list）
    ToolsList,

    /// 単一ライタ daemon: 起動（foreground または detach）
    DaemonStart {
        /// true のとき detach で起動（background）
        detach: bool,
    },
    /// 単一ライタ daemon: ping
    DaemonPing,
    /// 単一ライタ daemon: status（ping ベース）
    DaemonStatus,
    /// 単一ライタ daemon: jobs（active / persisted lifecycle 一覧）
    DaemonJobs {
        active_only: bool,
        persisted_only: bool,
    },
    /// 単一ライタ daemon: stop
    DaemonStop,
    /// 単一ライタ daemon: cancel ai job
    DaemonCancel { job_id: String },

    /// 単一ライタ daemon: ensure（起動していなければ detach で起動して待つ）
    DaemonEnsure,

    /// 未知のコマンド（エラー用）
    Unknown(String),
}

impl Command {
    /// コマンド名と引数から Command に解析する（resume / memory は args を使用）
    pub fn parse_with_args(name: &str, args: &[String]) -> Self {
        if name == "resume" {
            let id = args.first().cloned();
            return Command::Resume { id };
        }
        if name == "shell" {
            return match args.first().map(|s| s.as_str()) {
                Some("status") => Command::ShellStatus,
                _ => Command::Shell,
            };
        }
        if name == "memory" {
            match args.first().map(|s| s.as_str()) {
                Some("list") => return Command::MemoryList,
                Some("get") => {
                    return Command::MemoryGet {
                        ids: args[1..].to_vec(),
                    }
                }
                Some("remove") => {
                    return Command::MemoryRemove {
                        ids: args[1..].to_vec(),
                    }
                }
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("memory {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "policy" {
            match args.first().map(|s| s.as_str()) {
                Some("explain") => return Command::PolicyExplain,
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("policy {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "config" {
            match args.first().map(|s| s.as_str()) {
                Some("explain") => return Command::ConfigExplain,
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("config {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "plugins" {
            match args.first().map(|s| s.as_str()) {
                Some("list") | None => return Command::PluginsList,
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("plugins {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "daemon" {
            match args.first().map(|s| s.as_str()) {
                Some("start") => {
                    let detach = args.iter().any(|arg| arg == "--detach");
                    return Command::DaemonStart { detach };
                }
                Some("ensure") => return Command::DaemonEnsure,
                Some("ping") => return Command::DaemonPing,
                Some("status") => return Command::DaemonStatus,
                Some("jobs") => {
                    let active_only = args.iter().any(|arg| arg == "--active");
                    let persisted_only = args.iter().any(|arg| arg == "--persisted");
                    return Command::DaemonJobs {
                        active_only,
                        persisted_only,
                    };
                }
                Some("stop") => return Command::DaemonStop,
                Some("cancel") => {
                    return Command::DaemonCancel {
                        job_id: args.get(1).cloned().unwrap_or_default(),
                    }
                }
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("daemon {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "tools" {
            match args.first().map(|s| s.as_str()) {
                Some("list") | None => return Command::ToolsList,
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("tools {}", sub).trim_end().to_string());
                }
            }
        }
        if name == "history" {
            match args.first().map(|s| s.as_str()) {
                Some("ls") => {
                    let mut all = false;
                    let mut user_only = false;
                    let mut assistant_only = false;
                    for arg in args.iter().skip(1) {
                        match arg.as_str() {
                            "--all" => all = true,
                            "-a" => assistant_only = true,
                            "-u" => user_only = true,
                            _ => {}
                        }
                    }
                    return Command::HistoryLs {
                        all,
                        user_only,
                        assistant_only,
                    };
                }
                Some("get") => {
                    return Command::HistoryGet {
                        ids: args[1..].to_vec(),
                    }
                }
                _ => {
                    let sub = args.first().cloned().unwrap_or_else(|| "".to_string());
                    return Command::Unknown(format!("history {}", sub).trim_end().to_string());
                }
            }
        }
        Self::parse(name)
    }

    /// 文字列を Command に解析する（サブコマンドなし）。init は parse_with_args で解析する。
    pub fn parse(s: &str) -> Self {
        match s {
            "truncate_console_log" => Command::TruncateConsoleLog,
            "clear" => Command::Clear,
            "rollout" => Command::Rollout,
            "mute" => Command::Mute,
            "unmute" => Command::Unmute,
            "shell" => Command::Shell,
            "resume" => Command::Resume { id: None },
            "sessions" => Command::Sessions,
            "init" => Command::Init {
                force: false,
                dry_run: false,
                defaults_dir: None,
            },
            "plugins" => Command::PluginsList,
            "tools" => Command::ToolsList,
            _ => Command::Unknown(s.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_truncate_console_log() {
        let cmd = Command::parse("truncate_console_log");
        assert_eq!(cmd, Command::TruncateConsoleLog);
    }

    #[test]
    fn test_parse_rollout() {
        let cmd = Command::parse("rollout");
        assert_eq!(cmd, Command::Rollout);
    }

    #[test]
    fn test_parse_mute() {
        let cmd = Command::parse("mute");
        assert_eq!(cmd, Command::Mute);
    }

    #[test]
    fn test_parse_unmute() {
        let cmd = Command::parse("unmute");
        assert_eq!(cmd, Command::Unmute);
    }

    #[test]
    fn test_parse_resume() {
        let cmd = Command::parse("resume");
        assert_eq!(cmd, Command::Resume { id: None });
    }

    #[test]
    fn test_parse_with_args_shell_status() {
        let cmd = Command::parse_with_args("shell", &["status".to_string()]);
        assert_eq!(cmd, Command::ShellStatus);
    }

    #[test]
    fn test_parse_sessions() {
        let cmd = Command::parse("sessions");
        assert_eq!(cmd, Command::Sessions);
    }

    #[test]
    fn test_parse_unknown() {
        let cmd = Command::parse("unknown_cmd");
        assert!(matches!(cmd, Command::Unknown(s) if s == "unknown_cmd"));
    }

    #[test]
    fn test_parse_with_args_memory_list() {
        let cmd = Command::parse_with_args("memory", &["list".to_string()]);
        assert_eq!(cmd, Command::MemoryList);
    }

    #[test]
    fn test_parse_with_args_memory_get() {
        let cmd = Command::parse_with_args(
            "memory",
            &["get".to_string(), "id1".to_string(), "id2".to_string()],
        );
        assert!(
            matches!(&cmd, Command::MemoryGet { ids } if ids == &["id1".to_string(), "id2".to_string()])
        );
    }

    #[test]
    fn test_parse_with_args_memory_remove() {
        let cmd = Command::parse_with_args("memory", &["remove".to_string(), "abc".to_string()]);
        assert!(matches!(&cmd, Command::MemoryRemove { ids } if ids == &["abc".to_string()]));
    }

    #[test]
    fn test_parse_with_args_history_ls() {
        let cmd = Command::parse_with_args("history", &["ls".to_string()]);
        assert!(matches!(
            &cmd,
            Command::HistoryLs {
                all: false,
                user_only: false,
                assistant_only: false
            }
        ));
    }

    #[test]
    fn test_parse_with_args_history_ls_all_assistant() {
        let cmd = Command::parse_with_args(
            "history",
            &["ls".to_string(), "--all".to_string(), "-a".to_string()],
        );
        assert!(matches!(
            &cmd,
            Command::HistoryLs {
                all: true,
                user_only: false,
                assistant_only: true
            }
        ));
    }

    #[test]
    fn test_parse_with_args_history_ls_user() {
        let cmd = Command::parse_with_args("history", &["ls".to_string(), "-u".to_string()]);
        assert!(matches!(
            &cmd,
            Command::HistoryLs {
                all: false,
                user_only: true,
                assistant_only: false
            }
        ));
    }

    #[test]
    fn test_parse_with_args_history_get() {
        let cmd = Command::parse_with_args(
            "history",
            &["get".to_string(), "001".to_string(), "002".to_string()],
        );
        assert!(
            matches!(&cmd, Command::HistoryGet { ids } if ids == &["001".to_string(), "002".to_string()])
        );
    }

    #[test]
    fn test_parse_with_args_policy_explain() {
        let cmd = Command::parse_with_args("policy", &["explain".to_string()]);
        assert_eq!(cmd, Command::PolicyExplain);
    }

    #[test]
    fn test_parse_with_args_daemon_stop() {
        let cmd = Command::parse_with_args("daemon", &["stop".to_string()]);
        assert_eq!(cmd, Command::DaemonStop);
    }

    #[test]
    fn test_parse_with_args_daemon_jobs() {
        let cmd = Command::parse_with_args("daemon", &["jobs".to_string()]);
        assert_eq!(
            cmd,
            Command::DaemonJobs {
                active_only: false,
                persisted_only: false,
            }
        );
    }

    #[test]
    fn test_parse_with_args_daemon_jobs_filters() {
        let cmd = Command::parse_with_args(
            "daemon",
            &[
                "jobs".to_string(),
                "--active".to_string(),
                "--persisted".to_string(),
            ],
        );
        assert_eq!(
            cmd,
            Command::DaemonJobs {
                active_only: true,
                persisted_only: true,
            }
        );
    }

    #[test]
    fn test_parse_with_args_daemon_cancel() {
        let cmd = Command::parse_with_args("daemon", &["cancel".to_string(), "job-1".to_string()]);
        assert_eq!(
            cmd,
            Command::DaemonCancel {
                job_id: "job-1".to_string()
            }
        );
    }

    #[test]
    fn test_parse_with_args_daemon_start_default_is_foreground() {
        let cmd = Command::parse_with_args("daemon", &["start".to_string()]);
        assert!(matches!(
            cmd,
            Command::DaemonStart { detach } if !detach
        ));
    }

    #[test]
    fn test_parse_with_args_daemon_start_with_detach_flag() {
        let cmd =
            Command::parse_with_args("daemon", &["start".to_string(), "--detach".to_string()]);
        assert!(matches!(
            cmd,
            Command::DaemonStart { detach } if detach
        ));
    }

    #[test]
    fn test_parse_with_args_daemon_ensure() {
        let cmd = Command::parse_with_args("daemon", &["ensure".to_string()]);
        assert_eq!(cmd, Command::DaemonEnsure);
    }
}
