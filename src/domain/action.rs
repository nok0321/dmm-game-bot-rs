/// 順序ランナーが解釈する低レベル動作。
/// 現状は SequenceRunner の中で直接ステップ関数を呼んでおり、
/// この enum は将来 DSL 化する際の入口として保持している。
#[derive(Debug, Clone)]
pub enum Action {
    /// テンプレを `timeout_ms` 内に `poll_ms` 間隔で探索し、見つけたらクリック。
    ClickTemplate {
        template_name: String,
        timeout_ms: u64,
        poll_ms: u64,
    },
    /// テンプレ消失を確認 (デバウンス用)。
    WaitForTemplateGone {
        template_name: String,
        timeout_ms: u64,
        poll_ms: u64,
    },
    /// ROI 限定のポジティブ確認。マッチしなければ `on_miss` で停止。
    AssertTemplate {
        template_name: String,
        timeout_ms: u64,
        on_miss: GuardAction,
    },
    Sleep {
        ms: u64,
    },
    /// タイムアウトしたら正常スキップ扱い (Step 9 close 用)。
    OptionalClickTemplate {
        template_name: String,
        timeout_ms: u64,
        poll_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardAction {
    /// 即座に BotError::ReissekiGuardFailed で停止 (クリック発行は一切行わない)。
    Abort,
}
