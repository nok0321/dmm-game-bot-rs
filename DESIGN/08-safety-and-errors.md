# 08. 安全装置とエラー設計

## 8.1 BotError 型階層

`src/error.rs` の `enum BotError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error("window not found: {0}")]
    WindowNotFound(String),

    #[error("capture failed: {0}")]
    CaptureFailed(String),

    #[error("template wait timeout: {template} for {elapsed_ms}ms (best score: {best_score:.4})")]
    TemplateWaitTimeout { template: String, elapsed_ms: u64, best_score: f32 },

    #[error("reisseki guard failed: zero-state template did not match — refusing to click 'use' (best score: {best_score:.4})")]
    ReissekiGuardFailed { best_score: f32 },

    #[error("input send failed: {0}")]
    InputFailed(String),

    #[error("template not found in library: {0}")]
    TemplateNotFound(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("other: {0}")]
    Other(String),
}
```

`pub type Result<T> = std::result::Result<T, BotError>` で全モジュール共通。

`run_loop` での扱い分け:

| エラー | レベル | 後続動作 |
|---|---|---|
| `ReissekiGuardFailed` | `error!()` ログ + `"REISSEKI GUARD FAILURE — aborting all cycles"` | **即 return** (全サイクル中断) |
| その他 (`TemplateWaitTimeout` 含む) | `error!()` ログ | 即 return (1 サイクル分で停止) |

## 8.2 霊晶石ガード (最優先停止条件)

### 8.2.1 リスク

「最大選択」は AP 不足分を `英気の霊薬(小→中→大) → 霊晶石` の順で
自動充填する仕様。霊薬枯渇フェーズで「最大選択 → 使用」を押すと
**課金通貨である霊晶石 (1 個 = 185 AP 回復) が自動消費される**。
実機で 13.5 万個保有しているような場合、誤動作 1 回でも被害が大きい
高リスクポイント。

### 8.2.2 対策の本質

- ステップ 2 (「最大選択」クリック) の **直後**、ステップ 3 (「使用」クリック) の **前**
  に、霊晶石スロット (右端 4 列目) の数量カウンタ部分 ROI に対して
  `reisseki_zero_guard.png` のテンプレマッチを行う。
- マッチした (NCC ≥ threshold) → 霊晶石は 0 と確認 → ステップ 3 に進む。
- マッチしなかった → `BotError::ReissekiGuardFailed` で **即停止**。
  「使用」ボタンは **絶対に押さない**。

### 8.2.3 機械的に保証されている不変条件

| 不変条件 | 保証箇所 |
|---|---|
| `reisseki_zero_guard` テンプレが起動時に必ず存在する | `Config::validate` |
| ROI が必ず指定されている (画面全体探索を禁止) | `Config::validate` |
| `threshold >= 0.80` (緩める方向の事故防止) | `Config::validate` |
| ガード失敗時にクリック発行が一切起こらない | `do_assert_reisseki_zero` メソッドに **クリック発行ロジックが存在しない** |
| ガード失敗が全サイクルを止める | `run_loop` が `ReissekiGuardFailed` をパターンマッチで専用ハンドル |
| 既定 `safety.dry_run = true` | `default_dry_run()` + ユニットテスト `dry_run_default_is_true` |

### 8.2.4 触ってはいけないコード

- `bot/sequence.rs::do_assert_reisseki_zero` の **失敗時パス**:
  `Err(BotError::ReissekiGuardFailed { best_score })` 直前にクリック発行や
  「最後にもう 1 回試す」のような分岐を入れない。
- 同 `do_assert_reisseki_zero` の **キャプチャ失敗リトライ分岐**:
  `should_propagate_capture_failure` で false が返った後の処理は
  「ログ出力 → deadline 確認 → sleep → continue」だけ。
  ここにクリック発行や「最後の手段の click」を絶対に追加しない。
  retry 中はガード未確認状態が維持されるので、これを破らない限り
  「ガード PASS なしにクリックしない」不変条件は機械的に保たれる。
- `Config::validate` 内の `REISSEKI_GUARD_MIN_THRESHOLD = 0.80` を下げない。
- `safety.dry_run` 既定値 `true`。
- `JST 固定ログ` (システムロケール依存にしない)。

## 8.3 ドライラン

`safety.dry_run = true` (既定) または CLI `--dry-run` で:

- `BotEngine::new` が `Box<dyn InputSender> = Box::new(DryRunSender)` を選択。
- `DryRunSender::click_at` は **`warn!()` でログを出すだけで何もしない**。
- 検出ロジック (キャプチャ、マッチング、stability check、ハード sleep) は
  すべて本番と同じ動きをする。

CLI `--live` を明示すると `dry_run_override = Some(false)` になり、
`SendInputSender` が選ばれる。**`--dry-run` と `--live` は clap で排他**。

## 8.4 デイリー切替停止 (05:00 JST)

```rust
// run_loop 冒頭
let cutoff_time = parse_cutoff_hh_mm(&config.stop.daily_cutoff_jst)?;  // "05:00"
let start = now_jst();
let next_cut = next_cutoff_after(start, cutoff_time);  // 当日 or 翌日

loop {
    if max_cycles > 0 && count >= max_cycles { ... return Ok(()); }
    let now = now_jst();
    if now >= next_cut {
        info!("daily cutoff reached at {} — exiting", now);
        return Ok(());
    }
    run_one_cycle()?;
    count += 1;
}
```

`next_cutoff_after(start, cutoff)`:
- 当日の cutoff 時刻が `start` 以降なら当日。
- すでに過ぎていれば翌日 (`+ ChronoDuration::days(1)`)。
- `FixedOffset::from_local_datetime` の曖昧解は `Single` のみ返り、`None` でも安全側に
  `start` を返す (FixedOffset では理論上発生しない)。

### 8.4.1 既知ギャップ: 進行中サイクル完走

v1.1 設計書には **「ステップ 6 (討伐開始) を踏んだ後にデイリー切替が来た場合は
完走させてから停止」** という規約が書かれていたが、現行実装はサイクル先頭でしか
カットオフ判定をしていない。

結果として:
- カットオフ直前にサイクルを開始した → サイクル先頭の判定では `now < next_cut` のため通過 →
  そのサイクルは中断されず最後まで走り切る (45 分かかっても完走する)
- 次のサイクルに入ろうとした時点で `now >= next_cut` を踏んで終了

**つまり実害としては「規約通りに動いている」状態だが、設計書の意図とは異なる
偶然のセーフティネット**。明示的に `cycle.rs` で進行中フラグを持たせる改修は
未着手 (`CHECKPOINT.md` の優先度: 低タスクに記載)。

## 8.5 タイトループ防止

| 場所 | 下限 | 仕掛け |
|---|---|---|
| `Config::validate` | 100ms | `default_/post_battle_/debounce_interval_ms` のいずれかが下回ると起動失敗 |
| `Config::validate` | 50ms | `stability_poll_ms` の下限 |
| `bot/sequence.rs::POLL_SLEEP_FLOOR_MS` | 50ms | ループ内の `sleep(max(interval, 50ms))` で **二重保険** |

`Config::validate` を改変・スキップしても、`POLL_SLEEP_FLOOR_MS` がループ内側で
50ms 未満を弾く。逆もまた然りで、ループ内の `max` を消されても
`Config::validate` が起動を許さない。

## 8.5.1 一過性キャプチャ失敗のリトライ (ROB-5)

`PrintWindowCapturer` / `BitBltCapturer` はフォーカス切替・GPU 一時不在で
`BotError::CaptureFailed` を返すことがある。1 回の失敗で 30 分超のサイクル
(特に Next1: 戦闘 25 分待機後) を捨てるのは費用対効果が悪いため、
連続失敗カウンタが `loop.poll.capture_retry_threshold` (既定 3) に達するまでは
`warn!` ログを出して continue する。

| 配置 | 挙動 | 不変条件 |
|---|---|---|
| `try_click_template` | retry 中はクリック発行しない (deadline 超過時は `(None, best_score)` を返してスキップ・致命を呼び出し側が判断) | 旧来の `OnMiss::Fail` / `Skip` 動作は維持 |
| `do_assert_reisseki_zero` | retry 中も継続的に `reisseki_zero_guard` を探すだけ。**クリック発行ロジックは存在しない (機械的保証)**。連続失敗閾値超過は `BotError::CaptureFailed` 伝播、deadline 超過は `BotError::ReissekiGuardFailed` 伝播 | 「ガード未確認状態でのクリック発行」は依然として絶対に発生しない |
| 判定関数 | `should_propagate_capture_failure(failures, threshold)` 純粋関数。`threshold.max(1)` で 0 を 1 として正規化。`saturating_add` で overflow セーフ | `#[cfg(test)]` でユニットテスト 5 件 |
| カウンタリセット | キャプチャ成功で `consecutive_capture_failures = 0` | 一過性失敗の累積を断つ |

設定値 `capture_retry_threshold = 1` は旧挙動 (1 回目で即伝播) と等価。
`= 0` も `max(1)` 正規化により旧挙動。`= 2` 以上で初めてリトライ効果が出る。

## 8.6 安全装置一覧

| 装置 | 動作 | コード位置 |
|---|---|---|
| ドライラン既定 | `safety.dry_run = true` | `config.rs::default_dry_run` |
| CLI `--dry-run` / `--live` 排他 | clap `conflicts_with` | `cli.rs::Cli` |
| 霊晶石ガード | ROI 限定 positive check、失敗時クリック発行なし | `sequence.rs::do_assert_reisseki_zero` |
| 霊晶石ガード threshold 下限 | `>= 0.80` を起動時強制 | `config.rs::REISSEKI_GUARD_MIN_THRESHOLD` |
| 霊晶石ガード ROI 必須 | `roi: None` を起動時拒否 | `config.rs::Config::validate` |
| Stability check | 連続 N 回安定マッチ後にだけクリック | `sequence.rs::try_click_template` |
| キャプチャ失敗リトライ | 連続 N 回未満は warn ログで継続、N 回到達で初めて伝播 (ROB-5) | `sequence.rs::should_propagate_capture_failure` + `try_click_template` / `do_assert_reisseki_zero` |
| ハード sleep (Toubatsu/Next1/Next2) | 戦闘演出・遷移アニメ中は画面を見ない | `sequence.rs::do_click_then_min_wait` |
| Step 9 タイムアウトの正常スキップ | `OnMiss::Skip` で次サイクルへ | `sequence.rs::do_step` |
| デイリー切替 (05:00 JST) | サイクル先頭で時刻判定 | `sequence.rs::run_loop` + `cycle.rs::next_cutoff_after` |
| サイクル数上限 | `--max-cycles N` で段階的検証 | `sequence.rs::run_loop` |
| ポーリング下限 (Config) | < 100ms で起動失敗 | `config.rs::MIN_POLL_INTERVAL_MS` |
| ポーリング下限 (実装) | `max(interval, 50ms)` でループ内下限 | `sequence.rs::POLL_SLEEP_FLOOR_MS` |
| Ctrl+C ハンドラ | OS 標準。進行中のクリックは完了まで待つ | (実装なし、std 任せ) |

## 8.7 ログ運用

ログレベル運用方針:
- `INFO`: サイクル開始/終了、ステップ完了、霊晶石ガード PASS、CLI 上書き発生
- `WARN`: stability check 内の不一致 (debug)、テンプレ未検出での `OnMiss::Skip`
  通知 (実際は info で出ている)、ドライラン時のクリック抑止
- `ERROR`: `ReissekiGuardFailed`、`TemplateWaitTimeout`、その他サイクル致命
- `DEBUG`: ROI 解決後の探索矩形、stability ペンディング (`pending stability n/N`)、
  テンプレ消失検知
- `TRACE`: 現状未使用 (将来用)

`-v` (DEBUG) で `pending stability` の挙動が見え、安定マッチ閾値の調整に役立つ。
