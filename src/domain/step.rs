use std::time::Duration;

/// 順序主導 9 ステップ。
/// `ReissekiGuard` は ROI 限定アサートのみ (クリックはしない)。
/// `Close` はタイムアウトを正常スキップとして扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    ApPlus,
    UseMax,
    ReissekiGuard,
    UseButton,
    TapIndicator,
    Toubatsu,
    ToubatsuStart,
    Next1,
    Next2,
    Close,
}

impl Step {
    pub fn name(&self) -> &'static str {
        match self {
            Step::ApPlus => "ap_plus",
            Step::UseMax => "use_max",
            Step::ReissekiGuard => "reisseki_guard",
            Step::UseButton => "use_button",
            Step::TapIndicator => "tap_indicator",
            Step::Toubatsu => "toubatsu",
            Step::ToubatsuStart => "toubatsu_start",
            Step::Next1 => "next1",
            Step::Next2 => "next2",
            Step::Close => "close",
        }
    }

    /// このステップが探索するテンプレ名 (TemplateLibrary のキー)。
    pub fn template_name(&self) -> &'static str {
        match self {
            Step::ApPlus => "ap_plus_button",
            Step::UseMax => "ap_recovered_use_max",
            Step::ReissekiGuard => "reisseki_zero_guard",
            Step::UseButton => "use_button",
            Step::TapIndicator => "tap_indicator",
            Step::Toubatsu => "toubatsu_button",
            Step::ToubatsuStart => "toubatsu_start",
            Step::Next1 | Step::Next2 => "next_button",
            Step::Close => "close_button",
        }
    }

    pub fn all() -> &'static [Step] {
        &[
            Step::ApPlus,
            Step::UseMax,
            Step::ReissekiGuard,
            Step::UseButton,
            Step::TapIndicator,
            Step::Toubatsu,
            Step::ToubatsuStart,
            Step::Next1,
            Step::Next2,
            Step::Close,
        ]
    }
}

/// 1 ステップの実行ログ。
#[derive(Debug, Clone)]
pub struct StepLog {
    pub step: Step,
    pub elapsed: Duration,
    pub matched_score: Option<f32>,
    pub skipped: bool,
}
