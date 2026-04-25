# Checkpoint: dmm-game-bot 周回完走確認まで
Updated: 2026-04-26T08:30+09:00 (JST)
Session: (continuation) 座標キャッシュ機構 (DESIGN/11) 実装

## 目標

`templates/dmm-game-bot_architecture.md` の設計に従い、DMM ブラウザゲーム
(主に「天地狂乱」/「あやかしランブル」) の AP 回復〜討伐周回シーケンスを自動化する
Windows 用 Rust CLI ツールを完成させる。最終形は単一 exe バイナリで配布可能、
霊晶石ガード等の安全装置を備えた状態で実機運用に耐える品質を目指す。

## 完了済み

### 基盤 (前セッション)
- [x] Cargo プロジェクト初期化（純 Rust 構成、windows-rs 0.58 / image 0.25 / imageproc 0.25 / clap 4 / tracing 等）
- [x] 全レイヤ実装（error / config / domain / vision / platform / bot / cli）
- [x] `config/default.toml` に 9 種テンプレ + ROI + 各種 timeout/threshold を記述
- [x] リリースビルド成功（`target/release/dmm-game-bot.exe` ≒ 6.2MB 単一バイナリ、LTO + strip + crt-static）
- [x] PrintWindow キャプチャ動作確認
- [x] 安全装置（霊晶石ガード / JST デイリー停止 / `--dry-run` / `--live`）
- [x] Stability check 機能（フェードインアニメ中の半透明ボタン誤クリック対策）

### このセッションでの追加・修正
- [x] **2 サイクル以上の周回継続を実機で確認** (2026-04-25 20:00 頃)
  - サイクル 1 → サイクル 2 開始 (ApPlus 再検出) を目視確認
  - 戦闘 (約 30 分) → リザルト (Next1 → Next2) → 報酬モーダル (Close) → ホーム復帰の全フローが
    機械的に成立した
- [x] **ToubatsuStart 後のハード待機**（旧 Issue #1 解決）
  - `loop.poll.post_battle_min_wait_ms` (デフォルト 25 分) を新設
  - 戦闘中は一切画面を見ないことで、戦闘演出中の偽マッチ (toubatsu_start / next_button / close_button) を
    踏まずに済む。debounce より圧倒的に堅い
  - `bot/sequence.rs::do_click_then_min_wait` を新設し ToubatsuStart / Next1 / Next2 で再利用
- [x] **Next1 / Next2 / Close の偽マッチ排除**（旧 Issue #2 解決）
  - `next_button` threshold 0.85 → 0.96、ROI を画面下端寄りに絞る (y_pct 0.55 → 0.65)
  - `close_button` threshold 0.85 → 0.93、ROI 下端 5% を除外 (静的偽マッチ (791, 668) score 0.9053 を排除)
  - `toubatsu_start` threshold 0.85 → 0.92 (右下隅の 0.9157 偽マッチを排除)
  - 通常テンプレ (ap_plus / use_max / use_button / toubatsu_button) も 0.85 → 0.93 に底上げ
- [x] **画面遷移待ちの導入**（旧 debounce 方式の根本欠陥を修正）
  - `next1_settle_wait_ms` (3 秒): リザルト系の連続画面で同じ位置に同じ next_button が出るため
    debounce では消えるのを検出できないので、ハード sleep に切り替え
  - `next2_settle_wait_ms` (2 秒): 「報酬獲得!!」モーダル表示アニメ完了まで Close 探索を遅延
  - 旧 `do_click_then_debounce` / `wait_template_gone` / `BotError::TemplateGoneTimeout` は完全削除
- [x] **Stability poll の高速化**
  - `input.stability_poll_ms` (デフォルト 50ms) を新設し、pending stability 中だけ高速ポーリング
  - 「matched ... pending stability 1/2」状態の滞留が ~1.5秒 → ~50ms に短縮
  - 初回マッチ前のポーリングは従来通り (CPU 負荷据え置き)
- [x] **観測性改善**
  - ログタイムスタンプを **JST (+09:00)** に変更（独自 `JstTime` impl で `tracing_subscriber::FormatTime`）
  - CLI フラグ `--post-battle-min-wait-ms <N>` を追加（動作確認用に 25 分待機を一時的に短縮可能）

### このセッション (2026-04-26) での追加・修正

- [x] **座標キャッシュ機構を実装** (DESIGN/11-coord-cache.md)
  - `src/vision/coord_cache.rs` を新設
    (`CoordCache` / `CachedCenter` / `CoordCacheStats` / `small_roi` /
    `CACHEABLE_TEMPLATES`)
  - キャッシュ対象 (ホワイトリスト固定): `ap_plus_button` / `use_button` /
    `toubatsu_button` / `toubatsu_start`
  - 絶対対象外: `reisseki_zero_guard` (霊晶石ガード保護)、
    `next_button` / `close_button` / `tap_indicator` / `ap_recovered_use_max`
  - 失効条件: クライアント領域 (W×H) 変動で全エントリ破棄
  - `BotEngine` に `coord_cache: RefCell<CoordCache>` 追加、
    `try_click_template` で「小 ROI 先行 → 失敗時に通常 ROI フォールバック」
  - `do_assert_reisseki_zero` は構造的にキャッシュ非到達 (経路完全分離維持)
  - 観測性: サイクル末サマリ + 個別ヒット/ミス/フォールバック/失効ログ
- [x] **事前整地** (robust-review S-Critical 解消)
  - `vision/coords.rs::roi_to_rect`: `saturating_sub` 統一 + NaN 防御
  - `vision/matcher.rs::find_in_rect`: 0 寸法 ROI / template ガード追加
  - `vision/coords.rs::full_rect` を `vision/matcher.rs::Rect::full` へ集約
  - `vision/matcher.rs::Match` を `Copy` 化、`bot/sequence.rs` の `m.clone()` 除去
  - `bot/sequence.rs::click_match` で `coords::client_to_screen` を活用
- [x] **設計書整合**
  - DESIGN/11-coord-cache.md 新設 (12 節構成、不変条件・テスト計画含む)
  - DESIGN/02 / 04 / 06 / 09 / 10 / README に CoordCache 相互参照を追加
  - spec-audit Critical 2 件 (AUDIT-1: full_rect 旧 API 残置 / AUDIT-2:
    予告章未追加) を解消
- [x] **テスト件数**: 9 → **20 件** (coord_cache 8 + config 3 追加、全 PASS)
- [x] **ビルド状態**: `cargo clippy --workspace --all-targets` 警告 0、
  `cargo build --release` 成功 (`target/release/dmm-game-bot.exe` 6,240,256 B)

## 進行中の課題

### Issue #1: 〜 #3: **解決済み** (上記参照)

すべて 2 サイクル目検証で解消を確認:
- Issue #1 (ToubatsuStart 後 debounce 失敗) → ハード sleep 化で根本解決
- Issue #2 (next_button / close_button 偽マッチ) → 閾値 + ROI 引き締めで解決
- Issue #3 (1 サイクル完走未確認) → 2 サイクル以上の継続を実機で確認

### このセッションで残した別タスク (spawn_task 経由で chip 化)

robust-review / spec-audit が出した周辺 Finding のうち、CoordCache と直交する
ものは「別タスク」chip として待機中 (進行可否はユーザー判断):

1. **capture.rs::extract_pixels の i32 オーバーフロー** (ROB-4) — 防御的修正
2. **try_click_template の一過性 capture failure リトライ** (ROB-5) —
   1 回の PrintWindow 失敗で 30 分サイクルが捨てられないようカウンタ化
3. **Config::validate に pre/post_click min<=max 検証** (ROB-7) — 起動時に弾く
4. **templates ファイル名のパストラバーサル拒否** (SEC-1) —
   `..` / 絶対パスを `Config::validate` で弾く
5. **DESIGN/*.md 横断の表記揺れ** (AUDIT-4..9) — 25/27.5 分、Step 9/10、
   テンプレ 9/10、debounce 残置理由、霊晶石ガード用語表

### 残タスク（軽微）

- [ ] `config/default.toml` の `post_battle_min_wait_ms` コメント "25 分" は実値 1650000ms (≈27.5 分) に未追従
- [ ] `do_click_then_min_wait` の info ログ文言が "battle in progress" 固定（Next1/Next2 でも出る）。
  ステップ別に文言を出し分けるかどうかは要検討
- [ ] `loop.poll.debounce_*_ms` は現状未使用だが config 上に残置 (将来同テンプレ消失検出が要るステップ用)

## 未着手（新セッションで実施したい改善）

### 優先度: 中

1. **座標キャッシュ機構**: ✅ **2026-04-26 実装完了** (DESIGN/11-coord-cache.md 参照)
   - 実機で 2 サイクル `--live` 検証はまだ未実施。次セッションで `cache hits` の
     カウントが期待通り (cycle1: 0、cycle2 以降: 4 種テンプレ各 ≥1) に出ることを確認。

2. **detect-once の overlay 機能**
   - 検出結果を画面上に矩形オーバーレイで PNG 保存する `--save-overlay PATH` オプション
   - ROI キャリブレーション時に視覚的に確認できるように

3. **テスト追加**
   - 現状ユニットテスト 9 件のみ (config 検証系)
   - tests/fixtures/ に実機スクショを置いて MockCapturer + MockInputSender でオフライン回帰

### 優先度: 低

4. **ROI キャリブレーション CLI**: `dmm-game-bot calibrate <template>` で対話的に ROI 切り出し
5. **設計書 §4.5.2 「ステップ 6 以降は完走」未実装**: cycle.rs で進行中フラグを持たせて cutoff 判定を遅延
6. **Action enum を活かした DSL 化**: 現状未使用。設定駆動でステップ列を組み替えたいなら復活
7. **simplify / code-review / robust-review の自走**: 蓄積された変更を一通り再レビュー

## 重要な決定事項

- **windows-rs 0.58 採用**: 0.62 が最新だが API 探索を 0.58 ベースで完結したためそのまま使用
- **xcap / OpenCV 不採用**: 純 Rust + windows-rs のみ（配布の単純化）
- **Clock 抽象化を省略**: `chrono::Utc::now()` 直呼び (テスト容易性は犠牲)
- **PrintWindow を第一優先、BitBlt フォールバック**
- **dry_run 既定 true**: 設定ファイルで `safety.dry_run = true`。`--live` 明示で初めて実クリック
- **debounce 方式を放棄しハード sleep に統一** (このセッション)
  - debounce は「テンプレが消える」前提だったが、リザルト系の連続画面で同じ位置に
    類似ボタンが出る場合に成立しない。シーケンス全体で hard sleep + threshold 厳格化に統一
- **ログは JST 固定**: システムロケール非依存。`bot::cycle::jst_offset()` を `cli::JstTime` でも再利用

## 環境状態

- プラットフォーム: Windows 11 + bash (Git Bash 想定)
- Rust: 1.94.0 (msvc target, x86_64-pc-windows-msvc, crt-static)
- ブランチ: **`feat/coord-cache`** (本セッション) / `main` (前回までの履歴)
  リモート: https://github.com/nok0321/dmm-game-bot-rs.git に push 済み
- ビルド状態: **成功**（`target/release/dmm-game-bot.exe` 6,240,256 bytes、
  modify time 2026-04-26 08:25）
- clippy: 警告なし（`cargo clippy --workspace --all-targets`）
- テスト: **20 件** (config 11 件 + coord_cache 8 件 + その他 1 件、全 PASS)
- 実機検証: 2 サイクル継続成立（2026-04-25 20:00、`--live`）。
  座標キャッシュ機構の実機検証は **未実施** (次セッションのタスク)

## ファイル構成

```
.
├── .cargo/config.toml              # crt-static
├── Cargo.toml / Cargo.lock         # 依存定義
├── CHECKPOINT.md                   # 本ファイル
├── config/
│   └── default.toml                # 9 種テンプレ + ROI + パラメータ
├── templates/                      # 9 種テンプレ PNG（既存）
│   ├── ap_plus_button.png ... (etc)
│   ├── dmm-game-bot_architecture.md  # 設計書 v1.1
│   └── README.md                   # テンプレ画像の説明
├── src/
│   ├── main.rs                     # 薄い bin
│   ├── lib.rs
│   ├── cli.rs                      # CLI + JstTime (tracing 用 JST フォーマッタ)
│   ├── error.rs                    # BotError (TemplateGoneTimeout は削除済み)
│   ├── config.rs                   # PollConfig + CoordCacheConfig (DESIGN/11) 追加、
│   │                               #  Config::validate で search_pad_px 範囲検査
│   ├── domain/{mod,step,action}.rs
│   ├── vision/{mod,template,matcher,coords,coord_cache}.rs  # coord_cache.rs 新設 (DESIGN/11)
│   ├── platform/{mod,window,capture,input,dpi}.rs
│   └── bot/
│       ├── mod.rs
│       ├── sequence.rs             # do_click_then_min_wait, try_click_template に
│       │                           #  CoordCache 統合 (RefCell<CoordCache>)
│       ├── cycle.rs                # JST + cutoff 計算 (jst_offset 公開)
│       └── humanize.rs
├── DESIGN/
│   ├── 01〜10-*.md                 # 既存設計書
│   └── 11-coord-cache.md           # 座標キャッシュ機構 (本セッション追加)
└── target/release/dmm-game-bot.exe # 6.2MB 単一バイナリ
```

## 次のセッションへの申し送り

### まずやること

1. CHECKPOINT.md を読んで状況把握
2. 座標キャッシュ機構の **実機検証** (`--max-cycles 2 --live`):
   - サイクル 1 末: `coord cache: hits=0 small_roi_miss_fallback=0 (...) entries=4`
   - サイクル 2 末: `coord cache: hits=4 small_roi_miss_fallback=0 (...) entries=4`
   - 期待値が出なければ DEBUG ログ (`-vv`) で `cache lookup`・`small ROI miss` を確認
3. 余裕があれば次の課題に着手 (CHECKPOINT 「未着手」優先度を参照)

### 触っていい / 触らないでほしい不変条件

- **触らないでほしい**:
  - 霊晶石ガードのロジック（`bot/sequence.rs::do_assert_reisseki_zero`）
    threshold 引き上げ / ROI 厳格化はアリだが、「失敗時にクリックしない」コードパスは絶対に変えない
  - `safety.dry_run = true` 既定。`--live` 明示で初めて実クリック
  - JST 固定ログ（システムロケールに依存させない）
  - 座標キャッシュ `CACHEABLE_TEMPLATES` (`vision/coord_cache.rs`) に
    `reisseki_zero_guard` / `next_button` / `close_button` / `tap_indicator` /
    `ap_recovered_use_max` を **絶対に追加しない**
    (DESIGN/11 §11.9 不変条件、テストで機械的に保護)
- **触っていい**:
  - 各テンプレの threshold / ROI（実機での best score 観測に応じて）
  - sleep 系の値（next1_settle_wait_ms / next2_settle_wait_ms / post_battle_min_wait_ms 等）
  - stability_count / stability_position_tol_px / stability_score_tol（誤クリック対策の調整）

### 実行確認コマンド

```bash
# ビルド
cargo build --release

# 検出のみ（テンプレロード+ROI 確認用）
target/release/dmm-game-bot.exe -v detect-once

# ドライラン 1 周
target/release/dmm-game-bot.exe --max-cycles 1 run

# 実クリック 1 周
target/release/dmm-game-bot.exe -v --live --max-cycles 1 run

# 動作確認用に戦闘待機を 30 秒に短縮して 1 周
target/release/dmm-game-bot.exe -v --live --max-cycles 1 --post-battle-min-wait-ms 30000 run

# 実クリック 2 周（周回継続確認）
target/release/dmm-game-bot.exe -v --live --max-cycles 2 run
```

### git ログ

`main` ブランチ:
- `5fe9ed0` feat: 初期ベータ版コミット (DMM ブラウザゲーム自動化ボット)

`feat/coord-cache` ブランチ (本セッション):
- 座標キャッシュ機構 (DESIGN/11) を 1 コミットで作成し PR 経由でマージ予定。
