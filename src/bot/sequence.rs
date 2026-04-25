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
use crate::vision::matcher::{Match, Matcher};
use crate::vision::template::{Template, TemplateLibrary};

/// ポーリング sleep のフェイルセーフ下限。Config::validate でも 100ms 未満は弾かれるが、
/// 将来の改変や直接呼び出しに備えてループ内側でも保険を張る (タイトループ防止)。
const POLL_SLEEP_FLOOR_MS: u64 = 50;

pub struct BotEngine {
    config: Config,
    window: GameWindow,
    capturer: Box<dyn Capturer + Send + Sync>,
    matcher: Matcher,
    input: Box<dyn InputSender>,
    templates: TemplateLibrary,
    dry_run: bool,
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
        let poll = self.config.loop_.poll.clone();

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
    fn do_assert_reisseki_zero(&self, timeout_ms: u64) -> Result<StepLog> {
        let started = Instant::now();
        let tpl = self.templates.require("reisseki_zero_guard")?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut best_score = 0f32;
        loop {
            let (matched, score) = self.capture_and_match(tpl)?;
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

        let mut best_score = 0f32;
        let mut last_match: Option<Match> = None;
        let mut stable_count: usize = 0;

        loop {
            let (matched, score) = self.capture_and_match(tpl)?;
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
                    last_match = Some(m.clone());

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
        let screen_x = rect.screen_x + cx;
        let screen_y = rect.screen_y + cy;
        let press_ms = random_press_duration_ms(
            self.config.input.click_press_duration_min_ms,
            self.config.input.click_press_duration_max_ms,
        );
        self.input.click_at(screen_x, screen_y, press_ms)
    }
}
