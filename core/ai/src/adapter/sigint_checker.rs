//! Ctrl+C（SIGINT）で割り込みフラグを立てる InterruptChecker 実装
//!
//! コンストラクタで ctrlc ハンドラを登録し、is_interrupted() でフラグを読む。

use crate::ports::outbound::InterruptChecker;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

static SIGINT_COUNT: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
static SIGINT_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C を受けたらカウンタをインクリメントする実装
pub struct SigintChecker {
    count: Arc<AtomicUsize>,
}

impl SigintChecker {
    /// 新しいチェッカーを作成し、SIGINT ハンドラを登録する。
    ///
    /// - カウンタはグローバルに共有される（複数回 new されても同じカウンタを参照）
    /// - ハンドラ登録は一度だけ行われ、その結果を共有する
    pub fn new() -> Result<Self, ctrlc::Error> {
        let count = SIGINT_COUNT
            .get_or_init(|| Arc::new(AtomicUsize::new(0)))
            .clone();

        // handler は一度だけ登録する。並行 new() 時も compare_exchange で調整。
        if SIGINT_INSTALLED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let count_for_handler = count.clone();
            if let Err(e) = ctrlc::set_handler(move || {
                let prev = count_for_handler.fetch_add(1, Ordering::SeqCst);

                if prev == 0 {
                    // 1回目：即フィードバック
                    let _ = writeln!(
                        io::stderr(),
                        "\n[Ctrl-C] Interrupt received. Gracefully stopping... (press Ctrl-C again to force quit)"
                    );
                    let _ = io::stderr().flush();
                } else {
                    // 2回目以降：即強制終了
                    let _ = writeln!(io::stderr(), "\n[Ctrl-C] Force quitting.");
                    let _ = io::stderr().flush();
                    std::process::exit(130);
                }
            }) {
                // 登録に失敗した場合はフラグを戻し、従来通り Err を返す
                SIGINT_INSTALLED.store(false, Ordering::SeqCst);
                return Err(e);
            }
        }

        Ok(Self { count })
    }
}

impl InterruptChecker for SigintChecker {
    fn is_interrupted(&self) -> bool {
        self.count.load(Ordering::Relaxed) >= 1
    }
}

/// 割り込みを検知しないスタブ（ハンドラ登録に失敗した場合などに使用）
pub struct NoopInterruptChecker;

impl NoopInterruptChecker {
    pub fn new() -> Self {
        Self
    }
}

impl InterruptChecker for NoopInterruptChecker {
    fn is_interrupted(&self) -> bool {
        false
    }
}
