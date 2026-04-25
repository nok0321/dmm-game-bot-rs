# 03. ランタイムフロー

## 3.1 BotEngine の責務

`src/bot/sequence.rs` の `BotEngine` がオーケストレータ本体。
保有する依存 (Box<dyn> による所有):

```rust
pub struct BotEngine {
    config: Config,
    window: GameWindow,
    capturer: Box<dyn Capturer + Send + Sync>,  // Capturer trait は super-trait を持たない → 明示
    matcher: Matcher,
    input: Box<dyn InputSender>,                 // InputSender: Send + Sync (trait 側で要求済み)
    templates: TemplateLibrary,
    dry_run: bool,
}
```

**読み取り専用アクセサ** (テスト・将来拡張用):
`BotEngine::dry_run() -> bool`、`config() -> &Config`、`templates() -> &TemplateLibrary`。
`run_loop` / `run_one_cycle` / `detect_once` / `capture_rgba` は通常運用で使う。

`BotEngine::new` で:

1. テンプレ読み込み (設定不備をウィンドウ非存在より先に弾く)
2. ウィンドウ検索 (タイトルパターン部分一致)
3. 設定の `capture.method` から `Capturer` を選択
4. `dry_run_override` (CLI `--live` / `--dry-run`) を最優先、なければ `safety.dry_run`
5. `dry_run==true` なら `DryRunSender`、else `SendInputSender`
6. ログに `engine ready: dry_run=..., capture_method=..., templates=N` を出力

## 3.2 メインループ (`run_loop`)

```text
parse_cutoff_hh_mm(stop.daily_cutoff_jst)        // "05:00" → NaiveTime
start = now_jst()
next_cut = next_cutoff_after(start, cutoff_time)  // 当日 or 翌日の 05:00 JST
max_cycles = CLI override ?? config.loop.max_cycles  // 0 = 無限

loop:
  if max_cycles > 0 && count >= max_cycles → 正常終了
  if now_jst() >= next_cut → "daily cutoff reached" 正常終了
  run_one_cycle()
    Ok(report) → ログ出力、count++
    Err(ReissekiGuardFailed) → ログ + 即 return Err  // 全サイクル中断
    Err(その他) → ログ + 即 return Err
```

**現行実装の制約**:
v1.1 設計書には「ステップ 6 以降を踏んだ周は完走させてから停止」という
進行中サイクル完走規約があったが、現行 `run_loop` はサイクル先頭でしか
カットオフ判定をしないため、結果的に「現在進行中のサイクルは完走させる」
動作になっている (デイリーまたぎを途中で打ち切らない)。
意図的か成り行きかは曖昧 — `08-safety-and-errors.md` の既知ギャップを参照。

## 3.3 1 サイクル (`run_one_cycle`)

```text
started_at = now_jst()
window.focus()                                   // 失敗は無視 (ベストエフォート)

for step in Step::all():                          // 10 要素固定列
  log = match step:
    ApPlus, UseMax, UseButton, TapIndicator, Toubatsu
        → do_step(step, OnMiss::Fail, default_timeout, default_interval)
    ReissekiGuard
        → do_assert_reisseki_zero(default_timeout)  // クリック発行なし
    ToubatsuStart
        → do_click_then_min_wait(step, default_timeout, default_interval, post_battle_min_wait_ms)
    Next1
        → do_click_then_min_wait(step, post_battle_timeout, post_battle_interval, next1_settle_wait_ms)
    Next2
        → do_click_then_min_wait(step, default_timeout, default_interval, next2_settle_wait_ms)
    Close
        → do_step(step, OnMiss::Skip, close_button_timeout, default_interval)
  log.emit("step {:?} done: elapsed=...ms, score=..., skipped=...")
  steps.push(log)

return CycleReport{ started_at, completed_at: now_jst(), steps, success: true, error: None }
```

## 3.4 ステップ別パラメータ

| Step | テンプレ | timeout (config キー) | interval (config キー) | 後続ハード sleep | OnMiss |
|---|---|---|---|---|---|
| 1. ApPlus | `ap_plus_button` | `default_timeout_ms` (60s) | `default_interval_ms` (1.5s) | - | Fail (`TemplateWaitTimeout`) |
| 2. UseMax | `ap_recovered_use_max` | 同上 | 同上 | - | Fail |
| 2.5 ReissekiGuard | `reisseki_zero_guard` | `default_timeout_ms` (60s) | `default_interval_ms` の `max(POLL_SLEEP_FLOOR_MS=50ms)` | - | **Abort** (`ReissekiGuardFailed`、クリック発行なし) |
| 3. UseButton | `use_button` | 60s | 1.5s | - | Fail |
| 4. TapIndicator | `tap_indicator` | 60s | 1.5s | - | Fail |
| 5. Toubatsu | `toubatsu_button` | 60s | 1.5s | - | Fail |
| 6. ToubatsuStart | `toubatsu_start` | 60s | 1.5s | `post_battle_min_wait_ms` (既定 25min、`config/default.toml` では ≒ 27.5min) | Fail |
| 7. Next1 | `next_button` | `post_battle_timeout_ms` (45min) | `post_battle_interval_ms` (7s) | `next1_settle_wait_ms` (3s) | Fail |
| 8. Next2 | `next_button` | 60s | 1.5s | `next2_settle_wait_ms` (2s) | Fail |
| 9. Close | `close_button` | `close_button_timeout_ms` (コード既定 30s、`default.toml` では 5s) | 1.5s | - | **Skip** (`StepLog.skipped=true`) |

`config/default.toml` の値はベータ運用での実機調整値。Rust 側の
`fn default_*_ms()` は別途のフォールバック値で、設定ファイル欠損時の
保険として機能する (Step 9 のように両者が一致しないケースもある)。

## 3.5 do_step (汎用ステップ実行)

```rust
fn do_step(&self, step, on_miss, timeout_ms, poll_ms) -> Result<StepLog>:
  let started = Instant::now();
  let (matched, best_score) = try_click_template(step.template_name(), timeout_ms, poll_ms)?;
  match (matched, on_miss):
    (Some(m), _) → StepLog{step, elapsed, matched_score: Some(max(m.score, best_score)), skipped: false}
    (None, Skip) → ログ "step {step} skipped" → StepLog{..., matched_score: None, skipped: true}
    (None, Fail) → Err(TemplateWaitTimeout{template, elapsed_ms: timeout_ms, best_score})
```

`OnMiss` enum (sequence.rs 内 private):
- `Fail` - タイムアウトでサイクル終了 (致命)
- `Skip` - `skipped: true` でログを残し次ステップへ進む (Close 専用)

## 3.6 try_click_template (Stability check)

ポーリング探索の中核。フェードイン中の半透明ボタンを誤クリックしないよう、
**「位置とスコアが連続 N 回安定したら初めてクリック」** という stability check を実装。

```rust
fn try_click_template(name, timeout_ms, poll_ms) -> Result<(Option<Match>, f32)>:
  let stability_required = config.input.stability_count.max(1);  // 1=旧挙動
  let pos_tol = config.input.stability_position_tol_px;            // 既定 6px
  let score_tol = config.input.stability_score_tol;                // 既定 0.03

  let mut best_score = 0.0;
  let mut last_match: Option<Match> = None;
  let mut stable_count = 0;
  let deadline = Instant::now() + timeout_ms;

  loop:
    (matched, score) = capture_and_match(template);
    if score > best_score: best_score = score;

    match matched:
      Some(m):
        consistent = match last_match:
          Some(prev) → |m.cx-prev.cx|≤pos_tol ∧ |m.cy-prev.cy|≤pos_tol ∧ |m.score-prev.score|≤score_tol
          None → true
        if consistent: stable_count += 1
        else:           stable_count = 1   // リセット (debug ログを出す)
        last_match = Some(m);

        if stable_count >= stability_required:
          info "matched {name} (score=...) at client (x, y) [stable n/N] — issuing click"
          sleep(random(pre_click_min..=max_ms))   // 既定 150〜300ms
          click_match(m)                            // ジッタ + 押下時間ランダム + SendInput
          sleep(random(post_click_min..=max_ms))  // 既定 400〜800ms
          return Ok((Some(m), best_score))
        else:
          debug "{name} matched (score=...) — pending stability n/N"
      None:
        if last_match.is_some():
          debug "{name} disappeared — clearing stability buffer"
          last_match = None; stable_count = 0;

    if Instant::now() >= deadline:
      warn "{name} not found (or never stabilized) within {timeout}ms (best={best_score})"
      return Ok((None, best_score))

    // pending stability 中は別パラメータで高速ポーリング (滞留短縮)
    interval = if last_match.is_some() then config.input.stability_poll_ms else poll_ms;
    sleep(max(interval, POLL_SLEEP_FLOOR_MS=50ms))
```

**安全装置 (タイトループ防止)**:
全 sleep は `max(interval, POLL_SLEEP_FLOOR_MS=50ms)` で下限を保証。
`Config::validate` が起動時にも 100ms 未満を弾くが、ループ内側でも保険を張る。

## 3.7 do_click_then_min_wait (ハード sleep 戦略)

`ToubatsuStart` / `Next1` / `Next2` 専用。クリック発行後、戦闘演出または
画面遷移アニメ完了が見込める時間まで **一切画面を見ない**。

```rust
fn do_click_then_min_wait(step, click_timeout_ms, click_poll_ms, min_wait_ms) -> Result<StepLog>:
  let log = do_step(step, OnMiss::Fail, click_timeout_ms, click_poll_ms)?;
  if min_wait_ms > 0:
    info "{step}: hard sleep {min}min ({ms}ms) before advancing — battle in progress"
    std::thread::sleep(Duration::from_millis(min_wait_ms));
  Ok(log)
```

**意図**:
- ToubatsuStart 後: 戦闘 30 分中の偽マッチ (toubatsu_start / next_button / close_button) を踏まない。
- Next1 後: リザルト系の連続画面で同じ位置に同じ next_button が出るため、
  消失検出 (debounce) では成立しない → ハード sleep で UI 遷移完了まで待つ。
- Next2 後: 「報酬獲得!!」モーダル表示アニメ完了まで close 探索を遅延。
  アニメ途中の背景静的偽マッチが stability check を通過する事例があった。

ログ文言は現状すべて `"battle in progress"` 固定 (Next1/Next2 でも出る)。
ステップ別に出し分けるかは要検討 (`CHECKPOINT.md` 残タスク)。

## 3.8 do_assert_reisseki_zero (霊晶石ガード)

**最優先停止条件。このパスは絶対にクリックを発行しない。**

```rust
fn do_assert_reisseki_zero(timeout_ms) -> Result<StepLog>:
  let tpl = templates.require("reisseki_zero_guard")?;  // 起動時バリデーションで存在保証
  let deadline = Instant::now() + timeout_ms;
  let mut best_score = 0.0;
  loop:
    (matched, score) = capture_and_match(tpl);
    if score > best_score: best_score = score;
    if let Some(m) = matched:
      info "reisseki guard PASS (score=..., threshold=...)"
      return Ok(StepLog{ step: ReissekiGuard, matched_score: Some(m.score), skipped: false })
    if Instant::now() >= deadline:
      error "REISSEKI GUARD FAILED (best=... < threshold=...) — refusing to click 'use'"
      return Err(BotError::ReissekiGuardFailed { best_score })
    sleep(max(default_interval_ms, POLL_SLEEP_FLOOR_MS))
```

`do_step` を経由せず別メソッドにしているのは、**「クリック発行」という
副作用ロジックそのものが存在しないこと** をコード上で機械的に保証するため。
`run_loop` 側で `BotError::ReissekiGuardFailed` を専用にハンドル
(全サイクル中断、ログレベル `error`) する。

詳細は [`08-safety-and-errors.md`](08-safety-and-errors.md) を参照。

## 3.9 click_match (実クリックの組み立て)

```rust
fn click_match(m: &Match) -> Result<()>:
  let rect = window.client_rect()?;                          // 毎クリックで再取得 (ウィンドウ移動対応)
  let radius = config.input.click_jitter_radius_px;         // 既定 3px
  let (cx, cy) = jitter_click_point((m.center_x, m.center_y), radius);  // 一様分布 (humanize::jitter_click_point)
  let screen_x = rect.screen_x + cx;
  let screen_y = rect.screen_y + cy;
  let press_ms = random(click_press_duration_min..=max_ms); // 既定 60〜120ms
  input.click_at(screen_x, screen_y, press_ms)
```

`InputSender::click_at` の実装は `05-platform-windows.md` を参照。

## 3.10 サブコマンド経由の単発実行

`run` 以外のサブコマンドは `BotEngine::new` でセットアップ後、ループに入らず
1 回だけ実行して終わる:

- **`detect-once`**: `engine.detect_once()` で全テンプレを 1 度ずつ ROI 限定マッチし、
  `(template, matched, best_score, center)` の表を `stdout` に出す。
  テンプレ調整時の確認用。
- **`capture --output PATH`**: `engine.capture_rgba()` で 1 フレーム取得し PNG 保存。
  テンプレ切り出し用の元画像を作る用途。
