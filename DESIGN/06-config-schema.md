# 06. 設定スキーマ

設定は TOML 形式 (`config/default.toml`)。
パースは `serde` + `toml` で `Config` 構造体に流し込む。
`Config::load_from_file` がロード後に `Config::validate` を自動呼び出し、
**「緩める方向の事故設定」を起動時に弾く**。

## 6.1 トップレベル

```toml
templates_dir = "../templates"   # 設定ファイル相対で解決される (相対パスのみ親ディレクトリ基準で resolve)
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `templates_dir` | `PathBuf` | `"templates"` | テンプレ画像置き場。CLI `--templates-dir` で上書き |

`Config::load_from_file` で:
1. `templates_dir` が相対パスかつ `<config_parent>/<templates_dir>` が存在 →
   絶対パスへ resolve (リポジトリ内のレイアウトに追従)。
2. それ以外 (絶対パス or 既存しない) → そのまま採用。

## 6.2 [window]

```toml
[window]
title_pattern = "あやかしランブル"
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `title_pattern` | `String` | (必須) | ウィンドウタイトルにこの文字列が含まれるものを探す (部分一致) |

CLI `--window-title <STR>` で上書き可。

## 6.3 [capture]

```toml
[capture]
method = "print_window"   # "print_window" | "bitblt"
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `method` | enum | `"print_window"` | キャプチャ方式。`bitblt` はフォールバック用 |

## 6.4 [loop]

```toml
[loop]
max_cycles = 0   # 0 = 無限ループ
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `max_cycles` | `u32` | `0` | 0=無限。CLI `--max-cycles` で上書き |

## 6.5 [loop.poll] (PollConfig)

```toml
[loop.poll]
default_interval_ms = 1500
default_timeout_ms = 60000
post_battle_interval_ms = 7000
post_battle_timeout_ms = 2700000     # 45 min
post_battle_min_wait_ms = 1650000    # 27.5 min (default.toml; コード既定は 25 min)
next1_settle_wait_ms = 3000
next2_settle_wait_ms = 2000
close_button_timeout_ms = 5000       # default.toml; コード既定は 30 s
debounce_interval_ms = 1500          # 予備 (現状未使用)
debounce_timeout_ms = 60000          # 予備 (現状未使用)
```

**注意 (`default.toml` ⇄ コード既定の意図的な乖離)**:
`post_battle_min_wait_ms` と `close_button_timeout_ms` は `default.toml` の値が
コード側のフォールバック既定 (`fn default_*_ms()`) と **意図的に異なる**。
下表は **コード既定** を示す (`default.toml` 値は前段の TOML サンプルを参照)。
`default.toml` 値はベータ運用での実機調整値で、起動時は TOML 側が優先。

| キー | コード既定 | 用途 |
|---|---|---|
| `default_interval_ms` | 1500 | Step 1〜5, 8, 9 の通常ポーリング間隔 |
| `default_timeout_ms` | 60_000 | 通常ステップの未検出タイムアウト |
| `post_battle_interval_ms` | 7_000 | Step 7 (Next1) のロング待機ポーリング |
| `post_battle_timeout_ms` | 2_700_000 | Step 7 探索の最大時間 = 45 分 |
| `post_battle_min_wait_ms` | 1_500_000 | ToubatsuStart 後のハード sleep (25 分。CLI `--post-battle-min-wait-ms` で上書き) |
| `next1_settle_wait_ms` | 3_000 | Next1 後の画面遷移待ち |
| `next2_settle_wait_ms` | 2_000 | Next2 後の「報酬獲得!!」モーダルアニメ待ち |
| `close_button_timeout_ms` | 30_000 | Close 探索上限 (タイムアウトは正常スキップ) |
| `debounce_interval_ms` | 1_500 | 現状未使用 (将来再導入用に残置) |
| `debounce_timeout_ms` | 60_000 | 同上 |

**バリデーション**:
`default_interval_ms` / `post_battle_interval_ms` / `debounce_interval_ms` のいずれかが
`MIN_POLL_INTERVAL_MS = 100ms` 未満なら `BotError::Config`。タイトループ防止。

## 6.5.1 [loop.coord_cache] (CoordCacheConfig)

```toml
[loop.coord_cache]
enabled = true
search_pad_px = 24
relax_stability_on_hit = false
```

| キー | 型 | コード既定 | 用途 |
|---|---|---|---|
| `enabled` | `bool` | `true` | 座標キャッシュ機構を有効化。`false` で完全 bypass (デバッグ用) |
| `search_pad_px` | `u32` | `24` | キャッシュ中心 ± この値 px をテンプレ寸法に加えた範囲を小 ROI とする |
| `relax_stability_on_hit` | `bool` | `false` | キャッシュヒット時に stability check を緩和するか (既定 false; 安全側) |

**バリデーション**:
- `search_pad_px == 0` → `BotError::Config` (小 ROI が template 寸法と同寸でズレを許容できない)
- `search_pad_px > 256` → `BotError::Config` (大 ROI と差がなくなりキャッシュ意味が薄れる)
- `relax_stability_on_hit` 既定値 `false` をユニットテストで回帰防止
  (DESIGN/11 §11.9 不変条件サマリ参照)

機構の詳細・対象テンプレのホワイトリスト・観測性ログは
[`11-coord-cache.md`](11-coord-cache.md) を参照。

## 6.6 [stop]

```toml
[stop]
daily_cutoff_jst = "05:00"
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `daily_cutoff_jst` | `"HH:MM"` | `"05:00"` | この時刻 (JST) を超えたサイクルは開始しない |

`bot::cycle::parse_cutoff_hh_mm` でパース。不正値は `BotError::Config`。

## 6.7 [input] (InputConfig)

```toml
[input]
click_jitter_radius_px = 3
click_press_duration_min_ms = 60
click_press_duration_max_ms = 120
pre_click_min_ms = 150
pre_click_max_ms = 300
post_click_min_ms = 400
post_click_max_ms = 800
stability_count = 2
stability_position_tol_px = 6
stability_score_tol = 0.03
stability_poll_ms = 50
```

| キー | コード既定 | 用途 |
|---|---|---|
| `click_jitter_radius_px` | 3 | クリック中心からの一様ジッタ半径 (`humanize::jitter_click_point`) |
| `click_press_duration_min_ms` | 60 | LEFTDOWN→LEFTUP の押下時間下限 |
| `click_press_duration_max_ms` | 120 | 同上上限 |
| `pre_click_min_ms` | 150 | 安定マッチ後 → クリック発行 までの遅延下限 |
| `pre_click_max_ms` | 300 | 同上上限 |
| `post_click_min_ms` | 400 | クリック発行 → 次の検出開始 までの遅延下限 |
| `post_click_max_ms` | 800 | 同上上限 |
| `stability_count` | 2 | 連続マッチで「安定」と判定するために必要な回数。1=旧挙動 |
| `stability_position_tol_px` | 6 | 同位置とみなす中心座標の許容差 |
| `stability_score_tol` | 0.03 | 同スコアとみなす NCC 差の許容差 |
| `stability_poll_ms` | 50 | pending stability 中だけ使う高速ポーリング間隔 |

**バリデーション**:
`stability_poll_ms < MIN_STABILITY_POLL_MS = 50ms` は弾く。
通常ポーリングと別枠で 50ms まで許可するのは、「matched ... pending stability N/M」
状態の滞留時間を短縮するため。

## 6.8 [safety]

```toml
[safety]
dry_run = true   # 既定 true
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `dry_run` | `bool` | `true` | クリック送信を抑止。CLI `--live` で false 上書き、`--dry-run` で true 強制 |

**回帰防止テスト**: `default_dry_run()` と `SafetyConfig::default().dry_run` が
ともに true であることを `tests::dry_run_default_is_true` で機械的に保証。

## 6.9 [templates.<name>] (TemplateConfig)

```toml
[templates.<name>]
file = "<filename>.png"
threshold = 0.93
roi = { x_pct = 0.0, y_pct = 0.0, w_pct = 1.0, h_pct = 1.0 }   # optional
```

| キー | 型 | 既定 | 説明 |
|---|---|---|---|
| `file` | `String` | (必須) | `templates_dir` 配下の PNG ファイル名 |
| `threshold` | `f32` | 0.90 | NCC 採用閾値 (0.0〜1.0) |
| `roi` | `Option<RoiPct>` | None (画面全体) | クライアント領域に対する比率 |

**バリデーション (全テンプレ共通)**:
- `file` のパスコンポーネントが `Component::Normal` / `Component::CurDir` 以外
  （`..`、ドライブレター `C:\`、絶対パス `/...`、UNC `\\server\share` 等）を含む → 失敗
  - 理由: `templates_dir.join(&file)` で任意ファイルが読み込まれる
    パス・トラバーサルを防止 (共有 TOML のサプライチェーン懸念)
- `threshold` が NaN/Inf または `[0.0, 1.0]` 外 → `BotError::Config`
- `roi.{x_pct, y_pct, w_pct, h_pct}` が NaN/Inf または `[0.0, 1.0]` 外 → 失敗
- `roi.w_pct <= 0.0 || roi.h_pct <= 0.0` → 失敗

**`reisseki_zero_guard` 専用バリデーション (絶対不変条件)**:
- 設定不在 → 失敗 (`"reisseki_zero_guard template is required for safety guard"`)
- `threshold < REISSEKI_GUARD_MIN_THRESHOLD = 0.80` → 失敗
- `roi == None` → 失敗 (`"reisseki_zero_guard requires explicit roi (refusing fullscreen search)"`)

**現行 default.toml の登録順 (9 種)**:

```
ap_plus_button         threshold=0.93  roi=(0.00,0.00,1.00,0.30)
ap_recovered_use_max   threshold=0.93  roi=None
reisseki_zero_guard    threshold=0.90  roi=(0.66,0.20,0.30,0.65)  ← 必須 ROI
use_button             threshold=0.93  roi=None
tap_indicator          threshold=0.90  roi=None
toubatsu_button        threshold=0.93  roi=(0.40,0.50,0.60,0.50)
toubatsu_start         threshold=0.92  roi=(0.55,0.55,0.45,0.45)
next_button            threshold=0.96  roi=(0.00,0.65,1.00,0.35)
close_button           threshold=0.93  roi=(0.20,0.50,0.60,0.45)
```

## 6.10 不変条件サマリ (起動時に機械的に強制)

| 条件 | エラー時の挙動 |
|---|---|
| `safety.dry_run` 既定 true | テストが回帰防止 |
| `reisseki_zero_guard` テンプレが必ず登録されている | `BotError::Config` で起動失敗 |
| `reisseki_zero_guard.threshold >= 0.80` | 同上 |
| `reisseki_zero_guard.roi` が必ず指定されている | 同上 |
| 全テンプレの `threshold ∈ [0.0, 1.0]` | 同上 |
| 全テンプレの `roi.*_pct ∈ [0.0, 1.0]`、`w_pct,h_pct > 0` | 同上 |
| 全テンプレの `file` が `..` / 絶対パス / ドライブレターを含まない (パストラバーサル防止) | 同上 |
| ポーリング間隔 (default/post_battle/debounce) ≥ 100ms | 同上 |
| `stability_poll_ms ≥ 50ms` | 同上 |
| `loop.coord_cache.search_pad_px ∈ [1, 256]` | 同上 (DESIGN/11 §11.7) |
| `[input]` min/max ペアが `min ≤ max` (`click_press_duration_*_ms` / `pre_click_*_ms` / `post_click_*_ms`) | 同上 (TOML 編集ミスで逆転した値を起動時に弾く) |
