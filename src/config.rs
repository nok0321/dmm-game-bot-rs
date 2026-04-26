use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{BotError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub window: WindowConfig,
    #[serde(default)]
    pub capture: CaptureConfig,
    #[serde(rename = "loop", default)]
    pub loop_: LoopConfig,
    #[serde(default)]
    pub stop: StopConfig,
    #[serde(default)]
    pub input: InputConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub templates: HashMap<String, TemplateConfig>,
    /// テンプレート画像のディレクトリ (CLI で上書き可能)。
    #[serde(default = "default_templates_dir")]
    pub templates_dir: PathBuf,
}

fn default_templates_dir() -> PathBuf {
    PathBuf::from("templates")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub title_pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    #[serde(default = "default_capture_method")]
    pub method: CaptureMethod,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            method: default_capture_method(),
        }
    }
}

fn default_capture_method() -> CaptureMethod {
    CaptureMethod::PrintWindow
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    PrintWindow,
    Bitblt,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoopConfig {
    #[serde(default)]
    pub max_cycles: u32,
    #[serde(default)]
    pub poll: PollConfig,
    #[serde(default)]
    pub coord_cache: CoordCacheConfig,
}

/// 座標キャッシュ機構の設定。詳細は `DESIGN/11-coord-cache.md` を参照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordCacheConfig {
    /// false で機構を完全 bypass (デバッグ・回帰検証用)。既定 true。
    #[serde(default = "default_coord_cache_enabled")]
    pub enabled: bool,
    /// キャッシュ中心 ± この値 px をテンプレ寸法に加えた範囲を小 ROI とする。
    #[serde(default = "default_coord_cache_search_pad_px")]
    pub search_pad_px: u32,
    /// キャッシュヒット時に stability check を緩和するか (既定 false; 安全側)。
    /// 現バージョンではフラグだけ用意し、ロジックは将来拡張用。
    #[serde(default = "default_coord_cache_relax_stability_on_hit")]
    pub relax_stability_on_hit: bool,
}

impl Default for CoordCacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_coord_cache_enabled(),
            search_pad_px: default_coord_cache_search_pad_px(),
            relax_stability_on_hit: default_coord_cache_relax_stability_on_hit(),
        }
    }
}

fn default_coord_cache_enabled() -> bool { true }
fn default_coord_cache_search_pad_px() -> u32 { 24 }
fn default_coord_cache_relax_stability_on_hit() -> bool { false }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollConfig {
    #[serde(default = "default_default_interval_ms")]
    pub default_interval_ms: u64,
    #[serde(default = "default_default_timeout_ms")]
    pub default_timeout_ms: u64,
    #[serde(default = "default_post_battle_interval_ms")]
    pub post_battle_interval_ms: u64,
    #[serde(default = "default_post_battle_timeout_ms")]
    pub post_battle_timeout_ms: u64,
    /// ToubatsuStart クリック直後にハードに待機する時間。
    /// 戦闘は最低 30 分かかるため、それ未満で next_button を探すと
    /// 戦闘演出中の偽マッチを拾ってサイクルを暴走させるリスクがある。
    /// 0 にすると旧挙動 (即時 next_button 探索開始) に戻る。
    #[serde(default = "default_post_battle_min_wait_ms")]
    pub post_battle_min_wait_ms: u64,
    /// Next1 クリック直後の画面遷移待ち。
    /// Next1 と Next2 はどちらも next_button テンプレを使い、リザルト系の
    /// 連続画面で同じ位置に同じボタンが出るため、debounce では消えるのを
    /// 検出できない。ハード sleep で UI が遷移する時間を確保する。
    /// 0 にすると Next1 直後すぐ Next2 を探し始める。
    #[serde(default = "default_next1_settle_wait_ms")]
    pub next1_settle_wait_ms: u64,
    /// Next2 クリック直後の「報酬獲得!!」モーダル表示アニメ待ち。
    /// アニメ途中で close_button 探索を始めると、本物の close ボタンが
    /// 高スコアで現れる前に背景の偽マッチ (静的特徴) が stability check を
    /// 通ってしまう実機事例があるため、ここで明示的に待つ。
    #[serde(default = "default_next2_settle_wait_ms")]
    pub next2_settle_wait_ms: u64,
    #[serde(default = "default_close_button_timeout_ms")]
    pub close_button_timeout_ms: u64,
    #[serde(default = "default_debounce_interval_ms")]
    pub debounce_interval_ms: u64,
    #[serde(default = "default_debounce_timeout_ms")]
    pub debounce_timeout_ms: u64,
    /// `BotError::CaptureFailed` が連続で発生した場合に伝播する閾値。
    /// PrintWindow はフォーカス切替・GPU 一時不在で一過性に失敗することがあり、
    /// 1 回の失敗で 30 分超のサイクル待機を捨てるリスクを下げる。
    /// `failure_count >= threshold` になった時点で初めてエラーを伝播する。
    /// 0 または 1 を設定すると 1 回目の失敗で即伝播 (旧挙動)。
    #[serde(default = "default_capture_retry_threshold")]
    pub capture_retry_threshold: u32,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            default_interval_ms: default_default_interval_ms(),
            default_timeout_ms: default_default_timeout_ms(),
            post_battle_interval_ms: default_post_battle_interval_ms(),
            post_battle_timeout_ms: default_post_battle_timeout_ms(),
            post_battle_min_wait_ms: default_post_battle_min_wait_ms(),
            next1_settle_wait_ms: default_next1_settle_wait_ms(),
            next2_settle_wait_ms: default_next2_settle_wait_ms(),
            close_button_timeout_ms: default_close_button_timeout_ms(),
            debounce_interval_ms: default_debounce_interval_ms(),
            debounce_timeout_ms: default_debounce_timeout_ms(),
            capture_retry_threshold: default_capture_retry_threshold(),
        }
    }
}

fn default_default_interval_ms() -> u64 { 1500 }
fn default_default_timeout_ms() -> u64 { 60_000 }
fn default_post_battle_interval_ms() -> u64 { 7_000 }
fn default_post_battle_timeout_ms() -> u64 { 2_700_000 } // 45 min
fn default_post_battle_min_wait_ms() -> u64 { 1_500_000 } // 25 min
fn default_next1_settle_wait_ms() -> u64 { 3_000 } // 3 秒 (リザルト画面遷移待ち)
fn default_next2_settle_wait_ms() -> u64 { 2_000 } // 2 秒 (モーダル表示アニメ待ち)
fn default_close_button_timeout_ms() -> u64 { 30_000 }
fn default_debounce_interval_ms() -> u64 { 1500 }
fn default_debounce_timeout_ms() -> u64 { 60_000 }
/// 連続キャプチャ失敗のリトライ閾値。
/// 一過性の PrintWindow 失敗で 30 分超のサイクルを捨てないため、3 回までは耐える。
fn default_capture_retry_threshold() -> u32 { 3 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopConfig {
    /// "HH:MM" 形式 (JST)。
    #[serde(default = "default_daily_cutoff_jst")]
    pub daily_cutoff_jst: String,
}

impl Default for StopConfig {
    fn default() -> Self {
        Self {
            daily_cutoff_jst: default_daily_cutoff_jst(),
        }
    }
}

fn default_daily_cutoff_jst() -> String { "05:00".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    #[serde(default = "default_click_jitter_radius_px")]
    pub click_jitter_radius_px: u32,
    #[serde(default = "default_click_press_duration_min_ms")]
    pub click_press_duration_min_ms: u64,
    #[serde(default = "default_click_press_duration_max_ms")]
    pub click_press_duration_max_ms: u64,
    /// テンプレマッチ → クリック発行 の間に挟む遅延 (アニメーション完了待ち)。
    #[serde(default = "default_pre_click_min_ms")]
    pub pre_click_min_ms: u64,
    #[serde(default = "default_pre_click_max_ms")]
    pub pre_click_max_ms: u64,
    /// クリック発行 → 次の画面遷移検出開始 の間に挟む遅延。
    #[serde(default = "default_post_click_min_ms")]
    pub post_click_min_ms: u64,
    #[serde(default = "default_post_click_max_ms")]
    pub post_click_max_ms: u64,
    /// 連続マッチで「位置とスコアが安定」と判定するために必要な回数。
    /// 1 = 初回マッチで即クリック (旧挙動)。2 以上で stability check が効く。
    /// フェードイン中の半透明ボタンへの誤クリック対策。
    #[serde(default = "default_stability_count")]
    pub stability_count: u32,
    /// 連続マッチが「同じ位置」とみなされる中心座標の許容差 (ピクセル)。
    #[serde(default = "default_stability_position_tol_px")]
    pub stability_position_tol_px: u32,
    /// 連続マッチが「同じスコア」とみなされる NCC スコアの許容差。
    #[serde(default = "default_stability_score_tol")]
    pub stability_score_tol: f32,
    /// 一度マッチを観測した後、次のフレーム取得までの間隔 (ミリ秒)。
    /// 「pending stability N/M」状態に滞留する時間を短くしてクリック反応を速める。
    /// 初回マッチ前のポーリング間隔は `loop.poll.*_interval_ms` が使われる。
    #[serde(default = "default_stability_poll_ms")]
    pub stability_poll_ms: u64,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            click_jitter_radius_px: default_click_jitter_radius_px(),
            click_press_duration_min_ms: default_click_press_duration_min_ms(),
            click_press_duration_max_ms: default_click_press_duration_max_ms(),
            pre_click_min_ms: default_pre_click_min_ms(),
            pre_click_max_ms: default_pre_click_max_ms(),
            post_click_min_ms: default_post_click_min_ms(),
            post_click_max_ms: default_post_click_max_ms(),
            stability_count: default_stability_count(),
            stability_position_tol_px: default_stability_position_tol_px(),
            stability_score_tol: default_stability_score_tol(),
            stability_poll_ms: default_stability_poll_ms(),
        }
    }
}

fn default_click_jitter_radius_px() -> u32 { 3 }
fn default_click_press_duration_min_ms() -> u64 { 60 }
fn default_click_press_duration_max_ms() -> u64 { 120 }
fn default_pre_click_min_ms() -> u64 { 150 }
fn default_pre_click_max_ms() -> u64 { 300 }
fn default_post_click_min_ms() -> u64 { 400 }
fn default_post_click_max_ms() -> u64 { 800 }
fn default_stability_count() -> u32 { 2 }
fn default_stability_position_tol_px() -> u32 { 6 }
fn default_stability_score_tol() -> f32 { 0.03 }
fn default_stability_poll_ms() -> u64 { 50 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            dry_run: default_dry_run(),
        }
    }
}

fn default_dry_run() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub file: String,
    #[serde(default = "default_threshold")]
    pub threshold: f32,
    #[serde(default)]
    pub roi: Option<RoiPct>,
}

fn default_threshold() -> f32 { 0.90 }

/// クライアント領域全体に対する比率 (0.0〜1.0)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RoiPct {
    pub x_pct: f32,
    pub y_pct: f32,
    pub w_pct: f32,
    pub h_pct: f32,
}

/// 霊晶石ガードの threshold 最小値。
/// これより下げて起動することは禁止 (絶対不変条件: 厳格化方向のみ可)。
const REISSEKI_GUARD_MIN_THRESHOLD: f32 = 0.80;

/// ポーリング間隔の最小値 (ミリ秒)。
/// これより下げるとタイトループ化し、CPU 占有 / GDI 枯渇 / 誤フレーム連射による
/// 霊晶石ガードの誤 PASS 確率上昇を招く。
const MIN_POLL_INTERVAL_MS: u64 = 100;

/// stability check 中の高速ポーリング下限 (ミリ秒)。
/// 通常ポーリングと別枠。`bot::sequence::POLL_SLEEP_FLOOR_MS` と整合させる。
const MIN_STABILITY_POLL_MS: u64 = 50;

/// 座標キャッシュ機構の小 ROI パディング下限。0 ではキャッシュ位置のわずかな
/// ズレを許容できないので 1 を強制。
const COORD_CACHE_MIN_PAD_PX: u32 = 1;
/// 同上限。これ以上は通常 ROI と差がなくなりキャッシュの意味が薄れる。
const COORD_CACHE_MAX_PAD_PX: u32 = 256;

impl Config {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| BotError::Config(format!("read {}: {}", path.display(), e)))?;
        let mut cfg: Config = toml::from_str(&text)?;
        // 設定ファイル相対パスでテンプレートディレクトリが指定されていれば
        // 設定ファイルの親ディレクトリ基準で解決する。
        if cfg.templates_dir.is_relative() {
            if let Some(parent) = path.parent() {
                let candidate = parent.join(&cfg.templates_dir);
                if candidate.exists() {
                    cfg.templates_dir = candidate;
                }
            }
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// 設定値の境界検査。霊晶石ガードを「緩める方向」の事故設定 (threshold=0 等) を
    /// 起動時に弾く。`load_from_file` から自動的に呼ばれる。
    pub fn validate(&self) -> Result<()> {
        // --- テンプレ全般 ---
        for (name, t) in &self.templates {
            if !t.threshold.is_finite() || !(0.0..=1.0).contains(&t.threshold) {
                return Err(BotError::Config(format!(
                    "template '{}': threshold {} not finite or out of [0.0, 1.0]",
                    name, t.threshold
                )));
            }
            if let Some(r) = &t.roi {
                for (label, v) in [
                    ("x_pct", r.x_pct),
                    ("y_pct", r.y_pct),
                    ("w_pct", r.w_pct),
                    ("h_pct", r.h_pct),
                ] {
                    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                        return Err(BotError::Config(format!(
                            "template '{}' roi.{} = {} not finite or out of [0.0, 1.0]",
                            name, label, v
                        )));
                    }
                }
                if r.w_pct <= 0.0 || r.h_pct <= 0.0 {
                    return Err(BotError::Config(format!(
                        "template '{}' roi has zero or negative size (w_pct={}, h_pct={})",
                        name, r.w_pct, r.h_pct
                    )));
                }
            }
        }

        // --- 霊晶石ガード専用 (絶対不変条件: 緩める方向の事故を機械的に阻止) ---
        match self.templates.get("reisseki_zero_guard") {
            None => {
                return Err(BotError::Config(
                    "reisseki_zero_guard template is required for safety guard".into(),
                ));
            }
            Some(t) => {
                if t.threshold < REISSEKI_GUARD_MIN_THRESHOLD {
                    return Err(BotError::Config(format!(
                        "reisseki_zero_guard.threshold = {} is too lax; \
                         minimum {} (recommended 0.90+) — refusing to start",
                        t.threshold, REISSEKI_GUARD_MIN_THRESHOLD
                    )));
                }
                if t.roi.is_none() {
                    return Err(BotError::Config(
                        "reisseki_zero_guard requires explicit roi (refusing fullscreen search)"
                            .into(),
                    ));
                }
            }
        }

        // --- ポーリング間隔 (タイトループ防止) ---
        let p = &self.loop_.poll;
        for (label, v) in [
            ("default_interval_ms", p.default_interval_ms),
            ("post_battle_interval_ms", p.post_battle_interval_ms),
            ("debounce_interval_ms", p.debounce_interval_ms),
        ] {
            if v < MIN_POLL_INTERVAL_MS {
                return Err(BotError::Config(format!(
                    "loop.poll.{} = {} is too small (minimum {}ms)",
                    label, v, MIN_POLL_INTERVAL_MS
                )));
            }
        }

        // stability_poll_ms は通常ポーリングと別枠で 50ms まで許可する。
        if self.input.stability_poll_ms < MIN_STABILITY_POLL_MS {
            return Err(BotError::Config(format!(
                "input.stability_poll_ms = {} is too small (minimum {}ms)",
                self.input.stability_poll_ms, MIN_STABILITY_POLL_MS
            )));
        }

        // 座標キャッシュ search_pad_px の範囲検査 (DESIGN/11 §11.7)。
        let pad = self.loop_.coord_cache.search_pad_px;
        if !(COORD_CACHE_MIN_PAD_PX..=COORD_CACHE_MAX_PAD_PX).contains(&pad) {
            return Err(BotError::Config(format!(
                "loop.coord_cache.search_pad_px = {} out of range [{}, {}]",
                pad, COORD_CACHE_MIN_PAD_PX, COORD_CACHE_MAX_PAD_PX
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_template(threshold: f32, roi: Option<RoiPct>) -> TemplateConfig {
        TemplateConfig {
            file: "x.png".into(),
            threshold,
            roi,
        }
    }

    fn base_config() -> Config {
        let mut templates = HashMap::new();
        templates.insert(
            "reisseki_zero_guard".into(),
            base_template(
                0.90,
                Some(RoiPct {
                    x_pct: 0.66,
                    y_pct: 0.20,
                    w_pct: 0.30,
                    h_pct: 0.65,
                }),
            ),
        );
        Config {
            window: WindowConfig {
                title_pattern: "x".into(),
            },
            capture: CaptureConfig::default(),
            loop_: LoopConfig::default(),
            stop: StopConfig::default(),
            input: InputConfig::default(),
            safety: SafetyConfig::default(),
            templates,
            templates_dir: PathBuf::from("templates"),
        }
    }

    #[test]
    fn dry_run_default_is_true() {
        // 絶対不変条件: dry_run 既定 true を回帰防止。
        assert!(default_dry_run());
        assert!(SafetyConfig::default().dry_run);
    }

    #[test]
    fn validate_accepts_baseline_config() {
        assert!(base_config().validate().is_ok());
    }

    #[test]
    fn validate_rejects_loose_reisseki_threshold() {
        let mut cfg = base_config();
        cfg.templates.get_mut("reisseki_zero_guard").unwrap().threshold = 0.5;
        let err = cfg.validate().unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("reisseki_zero_guard.threshold"));
    }

    #[test]
    fn validate_rejects_zero_threshold() {
        let mut cfg = base_config();
        cfg.templates.get_mut("reisseki_zero_guard").unwrap().threshold = 0.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_missing_reisseki_roi() {
        let mut cfg = base_config();
        cfg.templates.get_mut("reisseki_zero_guard").unwrap().roi = None;
        let err = cfg.validate().unwrap_err();
        assert!(format!("{}", err).contains("requires explicit roi"));
    }

    #[test]
    fn validate_rejects_nan_roi() {
        let mut cfg = base_config();
        cfg.templates.get_mut("reisseki_zero_guard").unwrap().roi = Some(RoiPct {
            x_pct: f32::NAN,
            y_pct: 0.0,
            w_pct: 1.0,
            h_pct: 1.0,
        });
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_out_of_range_threshold() {
        let mut cfg = base_config();
        cfg.templates.get_mut("reisseki_zero_guard").unwrap().threshold = 1.5;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_poll_interval() {
        let mut cfg = base_config();
        cfg.loop_.poll.default_interval_ms = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_missing_reisseki_template() {
        let mut cfg = base_config();
        cfg.templates.clear();
        assert!(cfg.validate().is_err());
    }

    // === 座標キャッシュ関連 (DESIGN/11) ===

    #[test]
    fn validate_rejects_zero_search_pad() {
        let mut cfg = base_config();
        cfg.loop_.coord_cache.search_pad_px = 0;
        let err = cfg.validate().unwrap_err();
        assert!(format!("{}", err).contains("coord_cache.search_pad_px"));
    }

    #[test]
    fn validate_rejects_huge_search_pad() {
        let mut cfg = base_config();
        cfg.loop_.coord_cache.search_pad_px = 257;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn coord_cache_default_round_trip() {
        // [loop.coord_cache] 省略時の既定値が想定通りであることを回帰防止。
        let cc = CoordCacheConfig::default();
        assert!(cc.enabled);
        assert_eq!(cc.search_pad_px, 24);
        // 絶対不変条件: stability check 緩和は既定 false (DESIGN/11 §11.9)。
        assert!(!cc.relax_stability_on_hit);
        // バリデーション通過確認。
        let mut cfg = base_config();
        cfg.loop_.coord_cache = cc;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn capture_retry_threshold_default_is_three() {
        // 既定 3 を回帰防止 (ROB-5: 一過性 capture failure で 30 分超のサイクルを
        // 捨てないため、最低 3 回は耐える挙動を維持する)。
        assert_eq!(default_capture_retry_threshold(), 3);
        assert_eq!(PollConfig::default().capture_retry_threshold, 3);
    }
}
