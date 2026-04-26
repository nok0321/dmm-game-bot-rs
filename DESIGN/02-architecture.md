# 02. アーキテクチャ

## 2.1 レイヤー構造

```
┌──────────────────────────────────────────────────────┐
│  CLI / 設定層                                          │
│  - clap (引数解析、サブコマンド)                        │
│  - tracing / tracing-subscriber (構造化ログ + JST)     │
│  - serde + toml (TOML 設定)                           │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  オーケストレータ層 (bot/)                              │
│  - BotEngine (run_loop / run_one_cycle)                │
│  - do_step / do_click_then_min_wait /                  │
│    do_assert_reisseki_zero / try_click_template        │
│  - cycle (JST デイリー切替判定)                         │
│  - humanize (クリック座標 / タイミングのジッタ)          │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  ドメイン層 (domain/)                                   │
│  - Step enum (10 種: ApPlus..Close)                    │
│    (うちクリックを伴うのは 9 種、テンプレ画像も 9 種     │
│     — 詳細は DESIGN/01 §1.2 用語ガイドライン)            │
│  - StepLog (1 ステップの実行記録)                        │
│  - Action / GuardAction enum (DSL 化用、現状未使用)     │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  プラットフォーム抽象層 (platform/)                      │
│  ┌─────────────┬──────────────┬────────────────────┐ │
│  │ window      │ capture      │ input              │ │
│  │ (HWND取得)   │ (画像取得)    │ (SendInput)        │ │
│  └─────────────┴──────────────┴────────────────────┘ │
│  + dpi (DPI Awareness 設定)                            │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  画像処理層 (vision/)                                   │
│  - Template / TemplateLibrary (PNG → GrayImage)        │
│  - Matcher (NCC, ROI 限定, rayon 並列)                 │
│  - coords (RoiPct → Rect, client → screen 変換)        │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  OS 層 (Windows API、windows-rs 0.58)                  │
│  - Win32_Foundation / Win32_Graphics_Gdi               │
│  - Win32_UI_WindowsAndMessaging / Win32_UI_HiDpi       │
│  - Win32_UI_Input_KeyboardAndMouse                     │
│  - Win32_Storage_Xps (PrintWindow)                     │
└──────────────────────────────────────────────────────┘
```

## 2.2 モジュール構成

```
dmm-game-bot/
├── Cargo.toml
├── Cargo.lock
├── .cargo/config.toml          # crt-static
├── config/
│   └── default.toml            # 9 種テンプレ + ROI + パラメータ
├── templates/                  # 9 種 PNG。Step enum 10 要素から Next1/Next2 が
│   │                           # next_button.png を共有して 9 にまで縮約。
│   │                           # `reisseki_zero_guard.png` は Step::ReissekiGuard
│   │                           # (Step 3 UseButton の前段アサート) 専用。
│   ├── ap_plus_button.png      # 9 種テンプレ画像
│   ├── ap_recovered_use_max.png
│   ├── reisseki_zero_guard.png
│   ├── use_button.png
│   ├── tap_indicator.png
│   ├── toubatsu_button.png
│   ├── toubatsu_start.png
│   ├── next_button.png
│   ├── close_button.png
│   ├── README.md               # テンプレ画像の仕様
│   └── dmm-game-bot_architecture.md  # v1.1 設計書 (歴史的参考)
├── DESIGN/                     # 本書 (v1.1 から差分込みの現行設計)
├── CHECKPOINT.md               # セッション継続用作業メモ
└── src/
    ├── main.rs                 # 薄い bin: cli::main() を呼ぶだけ
    ├── lib.rs                  # 公開モジュール群
    ├── cli.rs                  # CLI + JstTime + init_logging
    ├── config.rs               # Config / *Config 構造体 + validate()
    ├── error.rs                # BotError
    ├── domain/
    │   ├── mod.rs
    │   ├── step.rs             # Step / StepLog
    │   └── action.rs           # Action / GuardAction (DSL 化用、未使用)
    ├── vision/
    │   ├── mod.rs
    │   ├── template.rs         # Template / TemplateLibrary
    │   ├── matcher.rs          # Matcher / Match / Rect
    │   ├── coords.rs           # roi_to_rect / client_to_screen
    │   └── coord_cache.rs      # CoordCache (静的位置テンプレ用、§DESIGN/11)
    ├── platform/
    │   ├── mod.rs
    │   ├── dpi.rs              # set_dpi_aware
    │   ├── window.rs           # GameWindow / WindowRect (windows / stub 切替)
    │   ├── capture.rs          # PrintWindowCapturer / BitBltCapturer
    │   └── input.rs            # SendInputSender / DryRunSender
    └── bot/
        ├── mod.rs
        ├── humanize.rs         # jitter_click_point / random_delay
        ├── cycle.rs            # CycleReport / now_jst / parse_cutoff_hh_mm / next_cutoff_after / jst_offset
        └── sequence.rs         # BotEngine (順序ランナー本体)
```

`#[cfg(windows)]` / `#[cfg(not(windows))]` で `window.rs` / `capture.rs` /
`input.rs` / `dpi.rs` は非 Windows 環境用のスタブを別 mod として持ち、
ビルド自体は他 OS でも通る (実行は Windows のみ)。

## 2.3 採用クレート

| クレート | バージョン | 用途 |
|---------|----------|------|
| `windows` | 0.58 | Win32 API バインディング (HWND, SendInput, PrintWindow, BitBlt 他) |
| `image` | 0.25 | 画像表現 (`RgbaImage` / `GrayImage`)、PNG 読込 |
| `imageproc` | 0.25 | テンプレートマッチング (`match_template_parallel`) |
| `serde` | 1 | 設定シリアライズ (derive 使用) |
| `toml` | 0.8 | TOML パース |
| `clap` | 4 | CLI 引数 (derive 使用) |
| `tracing` | 0.1 | 構造化ログ |
| `tracing-subscriber` | 0.3 | ログフォーマッタ (`env-filter`, `fmt`) |
| `anyhow` | 1 | `main` 戻り値の汎用エラー |
| `thiserror` | 1 | `BotError` 派生 |
| `chrono` | 0.4 (default-features=false, `clock`,`std`) | JST デイリー切替判定 |
| `rand` | 0.8 | クリック座標 / タイミングジッタ |

**依存ポリシー** (CHECKPOINT 確定):

- **OpenCV / xcap / GPU template-matching を採用しない**: 単一 exe 配布の単純化。
- **tokio を採用しない**: 同期ランタイム + `std::thread::sleep` のみで完結。
  v1.1 では tokio を候補に挙げていたが、ホットキー監視等が不要なため不採用。

## 2.4 動作環境

- プラットフォーム: Windows 10 / 11 (x86_64)
- ターゲット: `x86_64-pc-windows-msvc` (`crt-static`)
- 検証 Rust: 1.94.0 (msvc)
- 依存ランタイム: 不要 (Visual C++ 再頒布パッケージは crt-static により回避)
