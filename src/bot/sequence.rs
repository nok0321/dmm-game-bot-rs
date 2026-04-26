use std::cell::RefCell;
use std::time::{Duration, Instant};

use image::{DynamicImage, GrayImage};

use crate::bot::cycle::{next_cutoff_after, now_jst, parse_cutoff_hh_mm, CycleReport};
use crate::bot::humanize::{jitter_click_point, random_delay, random_press_duration_ms};
use crate::config::Config;
use crate::domain::step::{Step, StepLog};
use crate::error::{BotError, Result};
use crate::platform::capture::{build_capturer, Capturer};
use crate::platform::input::{DryRunSender, InputSender, SendInputSender};
use crate::platform::window::GameWindow;
use crate::vision::coord_cache::{small_roi, CachedCenter, CoordCache};
use crate::vision::coords::client_to_screen;
use crate::vision::matcher::{Match, Matcher};
use crate::vision::template::{Template, TemplateLibrary};

/// ポーリング sleep のフェイルセーフ下限。Config::validate でも 100ms 未満は弾かれるが、
/// 将来の改変や直接呼び出しに備えてループ内側でも保険を張る (タイトループ防止)。
const POLL_SLEEP_FLOOR_MS: u64 = 50;

/// 連続キャプチャ失敗カウンタを 1 進め、`threshold` に達したかを返す。
/// `threshold` 到達なら呼び出し側はエラーを伝播 (致命扱い)、未満なら継続 (リトライ)。
///
/// 純粋関数として分離してあるのは、ループ全体を Windows API ごとモック化するのは
/// 重すぎるため、判定ロジックだけは単体テストで挙動を保証したいから。
/// 詳細は [`tests::capture_retry_*`] を参照。
fn should_propagate_capture_failure(consecutive_failures: &mut u32, threshold: u32) -> bool {
    *consecutive_failures = consecutive_failures.saturating_add(1);
    *consecutive_failures >= threshold.max(1)
}

pub struct BotEngine {
    config: Config,
    window: GameWindow,
    capturer: Box<dyn Capturer + Send + Sync>,
    matcher: Matcher,
    input: Box<dyn InputSender>,
    templates: TemplateLibrary,
    dry_run: bool,
    /// 静的位置テンプレ用の座標キャッシュ (DESIGN/11-coord-cache.md)。
    /// `try_click_template` から内側可変アクセスする。
    /// `do_assert_reisseki_zero` は経路上 **絶対に参照しない** (霊晶石ガード保護)。
    coord_cache: RefCell<CoordCache>,
}

#[derive(Debug, Clone)]
pub struct DetectionRow {
    pub template: String,
    pub matched: bool,
    pub score: f32,
    pub center: Option<(u32, u32)>,
}

/// `do_step` の「テンプレが時間内に見えなかった」場合の扱い。
#[derive(Debug, Clone, Copy)]
enum OnMiss {
    /// `BotError::TemplateWaitTimeout` でサイクルを終了する (致命)。
    Fail,
    /// 正常スキップ扱い (`StepLog.skipped = true`) で次ステップへ進む。
    Skip,
}

impl BotEngine {
    pub fn new(config: Config, dry_run_override: Option<bool>) -> Result<Self> {
        // テンプレ読み込みを最初に行う (設定不備をウィンドウ非存在より先に検出するため)。
        let templates =
            TemplateLibrary::load_from_dir(&config.templates_dir, &config.templates)?;
        let window = GameWindow::find_by_title_substring(&config.window.title_pattern)?;
        let capturer = build_capturer(config.capture.method);
        let dry_run = dry_run_override.unwrap_or(config.safety.dry_run);
        let input: Box<dyn InputSender> = if dry_run {
            Box::new(DryRunSender)
        } else {
            Box::new(SendInputSender::new())
        };
        let matcher = Matcher::new();

        tracing::info!(
            "engine ready: dry_run={}, capture_method={:?}, templates={}",
            dry_run,
            config.capture.method,
            templates.names().len()
        );

        Ok(Self {
            config,
            window,
            capturer,
            matcher,
            input,
            templates,
            dry_run,
            coord_cache: RefCell::new(CoordCache::new()),
        })
    }

    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn templates(&self) -> &TemplateLibrary {
        &self.templates
    }

    fn capture_gray(&self) -> Result<GrayImage> {
        let rgba = self.capturer.capture(&self.window)?;
        Ok(DynamicImage::ImageRgba8(rgba).to_luma8())
    }

    pub fn capture_rgba(&self) -> Result<image::RgbaImage> {
        self.capturer.capture(&self.window)
    }

    /// `gray` フレーム上で `tpl` を ROI 込みで照合する (キャプチャは行わない)。
    fn match_in(&self, gray: &GrayImage, tpl: &Template) -> (Option<Match>, f32) {
        let roi = tpl.resolve_roi(gray.width(), gray.height());
        self.matcher.find_in_rect(gray, tpl, roi)
    }

    /// 1 フレームキャプチャしてテンプレ照合まで行う。ポーリング各イテレーションで使う。
    fn capture_and_match(&self, tpl: &Template) -> Result<(Option<Match>, f32)> {
        let gray = self.capture_gray()?;
        Ok(self.match_in(&gray, tpl))
    }

    pub fn detect_once(&self) -> Result<Vec<DetectionRow>> {
        let gray = self.capture_gray()?;
        let mut names = self.templates.names();
        names.sort_unstable();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let tpl = self.templates.require(name)?;
            let (matched, best) = self.match_in(&gray, tpl);
            out.push(DetectionRow {
                template: name.to_string(),
                matched: matched.is_some(),
                score: best,
                center: matched.map(|m| (m.center_x, m.center_y)),
            });
        }
        Ok(out)
    }

    pub fn run_loop(&self, cli_max_cycles: Option<u32>) -> Result<()> {
        let cutoff_time = parse_cutoff_hh_mm(&self.config.stop.daily_cutoff_jst)?;
        let start = now_jst();
        let next_cut = next_cutoff_after(start, cutoff_time);
        let max_cycles = cli_max_cycles.unwrap_or(self.config.loop_.max_cycles);
        tracing::info!(
            "loop start: dry_run={}, daily_cutoff={}, max_cycles={} (0=infinite)",
            self.dry_run,
            next_cut,
            max_cycles
        );

        let mut count = 0u32;
        loop {
            if max_cycles > 0 && count >= max_cycles {
                tracing::info!("max cycles {} reached — exiting", max_cycles);
                return Ok(());
            }
            let now = now_jst();
            if now >= next_cut {
                tracing::info!("daily cutoff reached at {} — exiting", now);
                return Ok(());
            }

            tracing::info!("---- cycle {} start ----", count + 1);
            match self.run_one_cycle() {
                Ok(report) => {
                    tracing::info!(
                        "---- cycle {} OK (steps={}, took {}s) ----",
                        count + 1,
                        report.steps.len(),
                        (report.completed_at - report.started_at).num_seconds()
                    );
                }
                Err(e @ BotError::ReissekiGuardFailed { .. }) => {
                    tracing::error!("REISSEKI GUARD FAILURE — aborting all cycles: {}", e);
                    return Err(e);
                }
                Err(e) => {
                    tracing::error!("cycle {} failed: {}", count + 1, e);
                    return Err(e);
                }
            }
            count += 1;
        }
    }

    pub fn run_one_cycle(&self) -> Result<CycleReport> {
        let started_at = now_jst();
        let mut steps: Vec<StepLog> = Vec::new();
        let poll = &self.config.loop_.poll;

        let _ = self.window.focus();

        for step in Step::all().iter().copied() {
            let log = match step {
                Step::ApPlus
                | Step::UseMax
                | Step::UseButton
                | Step::TapIndicator
                | Step::Toubatsu => self.do_step(
                    step,
                    OnMiss::Fail,
                    poll.default_timeout_ms,
                    poll.default_interval_ms,
                )?,
                Step::ReissekiGuard => self.do_assert_reisseki_zero(poll.default_timeout_ms)?,
                Step::ToubatsuStart => self.do_click_then_min_wait(
                    step,
                    poll.default_timeout_ms,
                    poll.default_interval_ms,
                    poll.post_battle_min_wait_ms,
                )?,
                Step::Next1 => self.do_click_then_min_wait(
                    step,
                    poll.post_battle_timeout_ms,
                    poll.post_battle_interval_ms,
                    poll.next1_settle_wait_ms,
                )?,
                Step::Next2 => self.do_click_then_min_wait(
                    step,
                    poll.default_timeout_ms,
                    poll.default_interval_ms,
                    poll.next2_settle_wait_ms,
                )?,
                Step::Close => self.do_step(
                    step,
                    OnMiss::Skip,
                    poll.close_button_timeout_ms,
                    poll.default_interval_ms,
                )?,
            };
            tracing::info!(
                "step {:?} done: elapsed={}ms, score={:?}, skipped={}",
                log.step,
                log.elapsed.as_millis(),
                log.matched_score,
                log.skipped,
            );
            steps.push(log);
        }

        let completed_at = now_jst();

        // サイクル末で座標キャッシュの累積状況を 1 行で出す (DESIGN/11 §11.8)。
        if self.config.loop_.coord_cache.enabled {
            let cache = self.coord_cache.borrow();
            let s = cache.stats();
            tracing::info!(
                "coord cache: hits={} misses={} (recovered={} still_missing={}) invalidations={} entries={}",
                s.hits,
                s.small_roi_misses,
                s.fallback_succeeded,
                s.fallback_failed,
                s.invalidations,
                cache.entries_len(),
            );
        }

        Ok(CycleReport {
            started_at,
            completed_at,
            steps,
            success: true,
            error: None,
        })
    }

    /// テンプレを timeout 内にポーリング探索 → 安定マッチでクリック発行 → StepLog を返す。
    /// `on_miss` が `Fail` ならテンプレを見ずに timeout した時点で `TemplateWaitTimeout`、
    /// `Skip` なら `skipped: true` の StepLog を返して次ステップへ進む。
    fn do_step(
        &self,
        step: Step,
        on_miss: OnMiss,
        timeout_ms: u64,
        poll_ms: u64,
    ) -> Result<StepLog> {
        let started = Instant::now();
        let (matched, best_score) =
            self.try_click_template(step.template_name(), timeout_ms, poll_ms)?;
        match (matched, on_miss) {
            (Some(m), _) => Ok(StepLog {
                step,
                elapsed: started.elapsed(),
                matched_score: Some(m.score.max(best_score)),
                skipped: false,
            }),
            (None, OnMiss::Skip) => {
                tracing::info!(
                    "step {:?} skipped — template not seen within {}ms (best={:.4}); treating as normal",
                    step,
                    timeout_ms,
                    best_score
                );
                Ok(StepLog {
                    step,
                    elapsed: started.elapsed(),
                    matched_score: None,
                    skipped: true,
                })
            }
            (None, OnMiss::Fail) => Err(BotError::TemplateWaitTimeout {
                template: step.template_name().to_string(),
                elapsed_ms: timeout_ms,
                best_score,
            }),
        }
    }

    /// クリック発行 → ハードに `min_wait_ms` だけ sleep して次ステップへ。
    /// ToubatsuStart 専用: 戦闘演出中は `toubatsu_start` も `next_button` も
    /// 偽マッチで >0.85 になり得るため、debounce や次ステップ即時開始は使わず
    /// 戦闘完了見込み時間まで一切画面を見ない (見ないことが安全保証)。
    fn do_click_then_min_wait(
        &self,
        step: Step,
        click_timeout_ms: u64,
        click_poll_ms: u64,
        min_wait_ms: u64,
    ) -> Result<StepLog> {
        let log = self.do_step(step, OnMiss::Fail, click_timeout_ms, click_poll_ms)?;
        if min_wait_ms > 0 {
            tracing::info!(
                "{:?}: hard sleep {}min ({}ms) before advancing — battle in progress",
                step,
                min_wait_ms / 60_000,
                min_wait_ms
            );
            std::thread::sleep(Duration::from_millis(min_wait_ms));
        }
        Ok(log)
    }

    /// 霊晶石ガード: ROI 限定でゼロ状態テンプレを探す。
    /// 見えなければ `BotError::ReissekiGuardFailed` を返してサイクルを停止する。
    /// **このパスは絶対にクリックを発行しない** (課金通貨の誤消費防止)。
    ///
    /// 連続キャプチャ失敗の retry も入っているが、retry 分岐は単に continue する
    /// だけでクリック発行パスは一切増えていない。capture が連続失敗し閾値を
    /// 超えれば `BotError::CaptureFailed` を伝播 (これも当然クリック発行なし)。
    /// 不変条件: 「ガード未確認の状態でクリックは絶対に発行されない」。
    fn do_assert_reisseki_zero(&self, timeout_ms: u64) -> Result<StepLog> {
        let started = Instant::now();
        let tpl = self.templates.require("reisseki_zero_guard")?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let capture_retry_threshold = self.config.loop_.poll.capture_retry_threshold;
        let mut best_score = 0f32;
        let mut consecutive_capture_failures: u32 = 0;
        loop {
            let (matched, score) = match self.capture_and_match(tpl) {
                Ok(res) => {
                    consecutive_capture_failures = 0;
                    res
                }
                Err(err) => {
                    if should_propagate_capture_failure(
                        &mut consecutive_capture_failures,
                        capture_retry_threshold,
                    ) {
                        // 致命扱い。ReissekiGuardFailed ではなく CaptureFailed を伝播するが、
                        // どちらにせよ「ガード未確認」のためクリックは発行されない。
                        return Err(err);
                    }
                    tracing::warn!(
                        "reisseki guard capture failed ({}/{}): {} — retrying (NOT clicking 'use')",
                        consecutive_capture_failures,
                        capture_retry_threshold,
                        err
                    );
                    if Instant::now() >= deadline {
                        tracing::error!(
                            "REISSEKI GUARD FAILED — capture flapping (best={:.4} < threshold={:.4}) — refusing to click 'use'",
                            best_score,
                            tpl.threshold
                        );
                        return Err(BotError::ReissekiGuardFailed { best_score });
                    }
                    std::thread::sleep(Duration::from_millis(
                        self.config
                            .loop_
                            .poll
                            .default_interval_ms
                            .max(POLL_SLEEP_FLOOR_MS),
                    ));
                    continue;
                }
            };
            if score > best_score {
                best_score = score;
            }
            if let Some(m) = matched {
                tracing::info!(
                    "reisseki guard PASS (score={:.4}, threshold={:.4})",
                    m.score,
                    tpl.threshold
                );
                return Ok(StepLog {
                    step: Step::ReissekiGuard,
                    elapsed: started.elapsed(),
                    matched_score: Some(m.score),
                    skipped: false,
                });
            }
            if Instant::now() >= deadline {
                tracing::error!(
                    "REISSEKI GUARD FAILED (best={:.4} < threshold={:.4}) — refusing to click 'use'",
                    best_score,
                    tpl.threshold
                );
                return Err(BotError::ReissekiGuardFailed { best_score });
            }
            std::thread::sleep(Duration::from_millis(
                self.config
                    .loop_
                    .poll
                    .default_interval_ms
                    .max(POLL_SLEEP_FLOOR_MS),
            ));
        }
    }

    /// テンプレを timeout 内にポーリング探索する。
    /// 連続 `stability_count` 回「位置 ±tol_px / score ±tol」内のマッチが続いたら
    /// クリックを発行する (フェードイン中の半透明ボタンへの誤クリック対策)。
    /// 見つからなかったら `(None, best_score)` を返す。
    ///
    /// 座標キャッシュ機構 (DESIGN/11) が有効かつテンプレがホワイトリスト
    /// (`CACHEABLE_TEMPLATES`) に含まれる場合、各イテレーションで
    /// 「キャッシュ位置周辺の小 ROI で先に NCC → 失敗時に通常 ROI へフォールバック」
    /// する。霊晶石ガード経路 (`do_assert_reisseki_zero`) は本関数を呼ばないため、
    /// キャッシュは構造的に到達不能。
    ///
    /// `BotError::CaptureFailed` は連続 `capture_retry_threshold` 回までは捕捉して
    /// 警告ログを出してリトライする (PrintWindow の一過性失敗で 30 分超のサイクルを
    /// 捨てないため)。閾値到達でのみ伝播。
    fn try_click_template(
        &self,
        name: &str,
        timeout_ms: u64,
        poll_ms: u64,
    ) -> Result<(Option<Match>, f32)> {
        let tpl = self.templates.require(name)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let stability_required = self.config.input.stability_count.max(1) as usize;
        let pos_tol = self.config.input.stability_position_tol_px;
        let score_tol = self.config.input.stability_score_tol;
        let capture_retry_threshold = self.config.loop_.poll.capture_retry_threshold;

        let cache_enabled = self.config.loop_.coord_cache.enabled;
        let cache_pad = self.config.loop_.coord_cache.search_pad_px;

        let mut best_score = 0f32;
        let mut last_match: Option<Match> = None;
        let mut stable_count: usize = 0;
        let mut consecutive_capture_failures: u32 = 0;

        loop {
            // capture_gray() の一過性失敗は ROB-5 の retry 機構で吸収。
            // CoordCache 統合 (DESIGN/11) によりキャプチャと NCC が分離されたため、
            // retry 対象は capture_gray() のみ (matcher.find_in_rect は失敗を返さない)。
            let gray = match self.capture_gray() {
                Ok(g) => {
                    consecutive_capture_failures = 0;
                    g
                }
                Err(err) => {
                    if should_propagate_capture_failure(
                        &mut consecutive_capture_failures,
                        capture_retry_threshold,
                    ) {
                        return Err(err);
                    }
                    tracing::warn!(
                        "{} capture failed ({}/{}): {} — retrying",
                        name,
                        consecutive_capture_failures,
                        capture_retry_threshold,
                        err
                    );
                    if Instant::now() >= deadline {
                        tracing::warn!(
                            "{} not found and capture flapping within {}ms (best={:.4})",
                            name,
                            timeout_ms,
                            best_score
                        );
                        return Ok((None, best_score));
                    }
                    std::thread::sleep(Duration::from_millis(poll_ms.max(POLL_SLEEP_FLOOR_MS)));
                    continue;
                }
            };
            let client_w = gray.width();
            let client_h = gray.height();

            // クライアント領域変動でキャッシュ全破棄 (DESIGN/11 §11.4)。
            if cache_enabled {
                self.coord_cache.borrow_mut().observe(client_w, client_h);
            }

            let roi_full = tpl.resolve_roi(client_w, client_h);

            // キャッシュ参照: ホワイトリスト外なら lookup が None を返すので安全。
            let cached = if cache_enabled {
                self.coord_cache.borrow().lookup(name)
            } else {
                None
            };

            // 1 段階目: 小 ROI (キャッシュ有り) または通常 ROI (無し) で探索。
            let mut cache_hit_path = cached.is_some();
            let (mut matched, mut score) = if let Some(cc) = cached {
                let small = small_roi(cc, tpl.width, tpl.height, cache_pad, client_w, client_h);
                tracing::debug!(
                    "{} cache lookup: small ROI ({},{}) {}x{} (cached_center=({},{}))",
                    name, small.x, small.y, small.w, small.h, cc.center_x, cc.center_y
                );
                self.matcher.find_in_rect(&gray, tpl, small)
            } else {
                self.matcher.find_in_rect(&gray, tpl, roi_full)
            };

            // 2 段階目: 小 ROI で空振りなら通常 ROI へフォールバック。
            if matched.is_none() && cache_hit_path {
                self.coord_cache.borrow_mut().note_small_roi_miss();
                tracing::info!(
                    "{} small ROI miss (best={:.4} < threshold={:.4}) — falling back to full ROI",
                    name, score, tpl.threshold
                );
                let (m2, s2) = self.matcher.find_in_rect(&gray, tpl, roi_full);
                if m2.is_some() {
                    self.coord_cache.borrow_mut().note_fallback_succeeded();
                    self.coord_cache.borrow_mut().evict(name);
                    tracing::info!(
                        "{} fallback recovered (score={:.4}) — refreshing cache on stable click",
                        name, s2
                    );
                } else {
                    self.coord_cache.borrow_mut().note_fallback_failed();
                    // フォールバックでも見つからなかった = 画面遷移途中の可能性が高い。
                    // 次の poll で再試行されるため致命ではないが、繰り返し出る場合は
                    // 遷移待ちの sleep 値や ROI 設定を見直す指標になる。
                    tracing::warn!(
                        "{} fallback also missed (best={:.4} < threshold={:.4}) — likely transition not settled, will retry",
                        name, s2, tpl.threshold
                    );
                }
                matched = m2;
                score = s2;
                cache_hit_path = false;
            }

            if score > best_score {
                best_score = score;
            }

            match matched {
                Some(m) => {
                    let consistent = match &last_match {
                        Some(prev) => {
                            let dx = (m.center_x as i32 - prev.center_x as i32).unsigned_abs();
                            let dy = (m.center_y as i32 - prev.center_y as i32).unsigned_abs();
                            let ds = (m.score - prev.score).abs();
                            dx <= pos_tol && dy <= pos_tol && ds <= score_tol
                        }
                        None => true,
                    };

                    if consistent {
                        stable_count += 1;
                    } else {
                        if let Some(prev) = &last_match {
                            tracing::debug!(
                                "{} match unstable: prev=(x={}, y={}, s={:.4}) now=(x={}, y={}, s={:.4}) — resetting",
                                name,
                                prev.center_x,
                                prev.center_y,
                                prev.score,
                                m.center_x,
                                m.center_y,
                                m.score
                            );
                        }
                        stable_count = 1;
                    }
                    last_match = Some(m);

                    if stable_count >= stability_required {
                        tracing::info!(
                            "matched {} (score={:.4}) at client ({}, {}) [stable {}/{}] — issuing click",
                            name,
                            m.score,
                            m.center_x,
                            m.center_y,
                            stable_count,
                            stability_required
                        );

                        // クリック発行直前にキャッシュ更新 (ホワイトリスト外は record 内で no-op)。
                        if cache_enabled {
                            self.coord_cache.borrow_mut().record(
                                name,
                                CachedCenter {
                                    center_x: m.center_x,
                                    center_y: m.center_y,
                                    last_score: m.score,
                                },
                            );
                            if cache_hit_path {
                                self.coord_cache.borrow_mut().note_hit();
                                tracing::info!(
                                    "{} cache hit: clicked at ({},{}) score={:.4}",
                                    name, m.center_x, m.center_y, m.score
                                );
                            }
                        }

                        std::thread::sleep(random_delay(
                            self.config.input.pre_click_min_ms,
                            self.config.input.pre_click_max_ms,
                        ));
                        self.click_match(&m)?;
                        std::thread::sleep(random_delay(
                            self.config.input.post_click_min_ms,
                            self.config.input.post_click_max_ms,
                        ));
                        return Ok((Some(m), best_score));
                    }
                    tracing::debug!(
                        "{} matched (score={:.4}) — pending stability {}/{}",
                        name,
                        m.score,
                        stable_count,
                        stability_required
                    );
                }
                None => {
                    if last_match.is_some() {
                        tracing::debug!("{} disappeared — clearing stability buffer", name);
                        last_match = None;
                        stable_count = 0;
                    }
                }
            }

            if Instant::now() >= deadline {
                tracing::warn!(
                    "{} not found (or never stabilized) within {}ms (best={:.4})",
                    name,
                    timeout_ms,
                    best_score
                );
                return Ok((None, best_score));
            }
            // pending stability 中は別パラメータで高速ポーリング (matched 表示の滞留を短縮)。
            let interval_ms = if last_match.is_some() {
                self.config.input.stability_poll_ms
            } else {
                poll_ms
            };
            std::thread::sleep(Duration::from_millis(interval_ms.max(POLL_SLEEP_FLOOR_MS)));
        }
    }

    fn click_match(&self, m: &Match) -> Result<()> {
        let rect = self.window.client_rect()?;
        let radius = self.config.input.click_jitter_radius_px;
        let (cx, cy) = jitter_click_point((m.center_x as i32, m.center_y as i32), radius);
        let (screen_x, screen_y) = client_to_screen(rect.screen_x, rect.screen_y, cx, cy);
        let press_ms = random_press_duration_ms(
            self.config.input.click_press_duration_min_ms,
            self.config.input.click_press_duration_max_ms,
        );
        self.input.click_at(screen_x, screen_y, press_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// threshold=3 の挙動: 1〜2 回目は false (継続)、3 回目で true (伝播)。
    /// ROB-5 の核となる挙動 (一過性失敗 2 回までは耐える) を機械的に保証する。
    #[test]
    fn capture_retry_holds_until_threshold() {
        let mut failures = 0u32;
        // 1 回目失敗: 継続
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        assert_eq!(failures, 1);
        // 2 回目失敗: 継続
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        assert_eq!(failures, 2);
        // 3 回目失敗: 伝播
        assert!(should_propagate_capture_failure(&mut failures, 3));
        assert_eq!(failures, 3);
    }

    /// 連続成功でカウンタがリセットされた前提なら、再び閾値まで耐える。
    /// (リセットは呼び出し側が `consecutive_capture_failures = 0` でやる)。
    #[test]
    fn capture_retry_resets_after_success_simulated() {
        let mut failures = 0u32;
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        // 呼び出し側が成功時にやるリセットをここで明示
        failures = 0;
        // 以降は再び 3 回耐えられる
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        assert!(!should_propagate_capture_failure(&mut failures, 3));
        assert!(should_propagate_capture_failure(&mut failures, 3));
    }

    /// threshold=1 (リトライ無効) は 1 回目で即伝播 (旧挙動と等価)。
    #[test]
    fn capture_retry_threshold_one_is_legacy_behavior() {
        let mut failures = 0u32;
        assert!(should_propagate_capture_failure(&mut failures, 1));
        assert_eq!(failures, 1);
    }

    /// threshold=0 は 1 として扱う (サニティ: 「0 で無限リトライ」のような曖昧さを避ける)。
    /// `threshold.max(1)` で正規化することで、設定ファイルでうっかり 0 を入れても
    /// 旧挙動 (即伝播) に倒れる。
    #[test]
    fn capture_retry_threshold_zero_normalizes_to_one() {
        let mut failures = 0u32;
        assert!(should_propagate_capture_failure(&mut failures, 0));
        assert_eq!(failures, 1);
    }

    /// 連続失敗カウンタが overflow しない (saturating_add の確認)。
    /// 実運用ではポーリング deadline でループは止まるので到達しないが、
    /// 永久ループの一過性事故防止として仕様化しておく。
    #[test]
    fn capture_retry_counter_saturates_at_u32_max() {
        let mut failures = u32::MAX;
        // saturating_add で u32::MAX のまま据え置き → 閾値超過で伝播
        assert!(should_propagate_capture_failure(&mut failures, 3));
        assert_eq!(failures, u32::MAX);
    }
}
