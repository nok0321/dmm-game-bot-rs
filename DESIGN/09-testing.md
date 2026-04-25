# 09. テスト戦略

## 9.1 現状 (ベータ版時点)

ユニットテストは 9 件、いずれも `src/config.rs::tests` モジュールに集約:

| テスト | 確認内容 |
|---|---|
| `dry_run_default_is_true` | **絶対不変条件**: `safety.dry_run` 既定値が true。回帰防止 |
| `validate_accepts_baseline_config` | 最低限の正常設定がバリデーションを通る |
| `validate_rejects_loose_reisseki_threshold` | 霊晶石ガード threshold < 0.80 で起動失敗 |
| `validate_rejects_zero_threshold` | 同上、threshold = 0.0 |
| `validate_rejects_missing_reisseki_roi` | 霊晶石ガード `roi == None` で起動失敗 |
| `validate_rejects_nan_roi` | 全テンプレで NaN ROI が拒否される |
| `validate_rejects_out_of_range_threshold` | `threshold > 1.0` で起動失敗 |
| `validate_rejects_zero_poll_interval` | ポーリング間隔 0ms で起動失敗 |
| `validate_rejects_missing_reisseki_template` | 霊晶石ガードテンプレ不在で起動失敗 |

**設計上の意図**:
ユニットテストは「霊晶石ガードを緩める方向の事故設定」のみを徹底ガードしている。
画像処理・ウィンドウ・入力エミュレーションは Win32 API 依存のため、ユニットテストは
最小限。代わりに次節の統合テストで補う方針 (未着手)。

実機 E2E:
- 2026-04-25 に実機 (Chrome + 「あやかしランブル」) で 2 サイクル継続成立を目視確認。
- ドライランモードでテンプレ検出ログを目視するフェーズが手動 E2E になっている。

## 9.2 未着手の統合テスト計画

### 9.2.1 フィクスチャ駆動オフライン回帰

`tests/fixtures/` に実機スクショ集を置き、`MockCapturer` + `MockInputSender`
でオフライン再生する:

```text
tests/fixtures/
  cycle_001/                       # 通常パス (close ありの周)
    step01_ap_plus.png
    step02_use_max.png
    step02_5_reisseki_zero.png
    step03_use_button.png
    step04_tap_indicator.png
    step05_toubatsu.png
    step06_toubatsu_start.png
    step07_next1.png
    step08_next2.png
    step09_close.png
  cycle_002/                       # close 出現しない正常スキップパス
    ...
    step09_no_close.png            # close_button が見えない通常画面
  cycle_guard_fail/                # 霊晶石ガード失敗パス
    step01_ap_plus.png
    step02_use_max.png
    step02_5_reisseki_selected.png # 0 と認識されない画像
```

期待する assert:
- `cycle_001`: 10 要素 (`Step::all()`) が順序通りに踏まれ、`MockInputSender` のクリック呼び出し回数が
  期待値 (8〜9 回、Close の有無で変動。`ReissekiGuard` はクリック発行なし) と一致。
- `cycle_002`: Step 9 が `OnMiss::Skip` で `StepLog.skipped == true`、その他は正常。
- `cycle_guard_fail`: `BotError::ReissekiGuardFailed` が返り、`MockInputSender` の
  クリック呼び出し回数が **正確にステップ 1, 2 ぶんで止まっている** こと。
  (= ガード失敗後にクリックが一切追加されないことの機械的保証)

### 9.2.2 Stability check の再現テスト

連続 N フレームで「位置とスコアが揺らぐ」スクショ列を作り、
`stability_count = 2` の場合に **クリックが発行されない** こと、
`stability_count = 1` (旧挙動) なら発行されることを確認。

### 9.2.3 ハード sleep 戦略の単発検証

`do_click_then_min_wait` を fake clock + fake sleep で呼び、
- クリック発行が 1 回だけ、
- その後の `sleep` 呼び出し時間が `min_wait_ms` と一致、
- ログに `"hard sleep ... before advancing"` が出る、
ことを確認する。

### 9.2.4 デイリー切替境界

`bot/cycle.rs::next_cutoff_after` の境界条件:
- `start = 04:59:59` → 同日 05:00:00
- `start = 05:00:00` → 翌日 05:00:00
- `start = 05:00:01` → 翌日 05:00:00

進行中サイクル完走規約 (8.4.1 既知ギャップ) を実装する場合は、
`run_loop` 内で `cutoff_after_step6` のような状態を持たせ、その境界の単体テストも追加する。

## 9.3 必要な抽象化作業 (テスト着手時の前提)

現行 `BotEngine` は `Box<dyn Capturer>` / `Box<dyn InputSender>` を持つので、
それぞれモック実装を差し込めばオフライン化できる。`Clock` は未抽象化
(`chrono::Utc::now()` 直呼び) なので、デイリー切替テストには `Clock` trait を導入する
リファクタが必要 (CHECKPOINT 「重要な決定事項」: テスト容易性は犠牲)。

## 9.4 ビルド検証

CI 整備は未着手。手動で:

```bash
cargo build --release   # crt-static 確認
cargo clippy            # 警告ゼロ目標 (CHECKPOINT 最終確認時警告なし)
cargo test              # 9 件のユニットテスト
```

を流して通ることをコミット前に確認する運用。
