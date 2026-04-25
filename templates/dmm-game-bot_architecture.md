# DMM ブラウザゲーム自動化ツール アーキテクチャ設計書

**プロジェクト名**: dmm-game-bot (仮)
**実装言語**: Rust (純Rust構成、image / imageproc中心)
**対象プラットフォーム**: Windows 10/11
**対象ゲーム**: DMM GAMES / FANZA GAMES (Canvas/WebGL ブラウザゲーム)
**作成日**: 2026年4月25日
**版**: 1.1 (順序主導モデル)

---

## 1. 目的とスコープ

### 1.1 目的

DMM ブラウザゲームの定型操作 (AP回復 → 討伐周回) を自動実行する Windows 用 CLI ツールを Rust で実装する。最終形は単一の exe バイナリとして配布可能であること。

### 1.2 自動化対象シーケンス

順序主導 9 ステップ (画面アンカーによる状態判定は行わず、指定順にクリック対象テンプレを探して押す):

| Step | 操作 | 検出テンプレート |
|------|------|------------------|
| 1 | AP+ アイコンをクリック | `ap_plus_button` |
| 2 | 「最大選択」をクリック | `ap_recovered_use_max` |
| 2.5 | 霊晶石スロットの選択数=0 をテンプレマッチで確認 (ガード) | `reisseki_zero_guard` |
| 3 | 「使用」をクリック | `use_button` |
| 4 | 回復モーダルの「TAP」インジケータをクリック | `tap_indicator` |
| 5 | 「討伐」(朱色) をクリック | `toubatsu_button` |
| 6 | 「討伐開始!」をクリック (以後、ゲーム内で 30 分以上の自動周回) | `toubatsu_start` |
| 7 | 周回完了後の「次へ」(1 回目) | `next_button` |
| 8 | 「次へ」(2 回目) | `next_button` |
| 9 | (Optional) 報酬達成モーダルの「閉じる」 | `close_button` |

ステップ 9 は報酬上限到達後は表示されないため、未検出でも正常パス。
ループ復帰判定は `ap_plus_button` の再検出。

### 1.3 スコープ外 (本設計書では扱わない)

- DMM Games Player (デスクトップアプリ) 経由のゲーム自動化
- 戦闘中の高度な判断 (コマンド選択、対象選択など)
- ログイン処理、2段階認証
- マルチアカウント並列実行
- 他のブラウザ (Edge, Firefox) サポート
- macOS / Linux サポート

### 1.4 非機能要件

| 項目 | 目標値 | 備考 |
|------|--------|------|
| 1周回あたりCPU時間 | < 5% (Ryzen/Core i5相当) | テンプレートマッチングは並列化しても軽量に |
| メモリフットプリント | < 100MB | テンプレートと直近スクショのみ保持 |
| ステップ間ポーリング間隔 (通常) | 1〜2秒 (タイムアウト 60秒) | ステップ 1〜5, 7, 8 |
| ステップ間ポーリング間隔 (周回後) | 5〜10秒 (タイムアウト 45分) | ステップ 6 → 7 のロング待機 |
| close_button 探索上限 | 30秒 (タイムアウト時は正常スキップ) | ステップ 9 (Optional) |
| 配布形態 | 単一 exe | 動的リンクなし、Visual C++ 再頒布パッケージ不要 |
| ウィンドウ位置 | 可変対応 | 起動時およびループごとにウィンドウ矩形を再取得 |
| DPIスケーリング | 100% / 125% / 150% 対応 | システムDPI Awareness 設定 |

---

## 2. 全体方針

### 2.1 自動化アプローチの選定

| 方式 | 採用可否 | 理由 |
|------|---------|------|
| Chrome DevTools Protocol | 不採用 | DMMゲーム本体が Canvas/WebGL のため DOM が空。テキスト要素が取れない |
| UI Automation (UIA) | 不採用 | Canvas に対しアクセシビリティツリーが提供されない |
| **画像認識 + 入力エミュレーション** | **採用** | 描画方式に依存せず、画面に出ている情報のみで完結する |
| メモリ読み取り / DLLインジェクション | 不採用 | 規約上のリスクが高く、検知されやすい。技術的にも保守性が低い |

### 2.2 設計原則

1. **画面に映っているものだけを根拠にする**: ゲーム内部状態への侵入はせず、人間がプレイする際と同じ情報源 (画面ピクセル) のみから判断する
2. **順序主導で進行管理**: 9 ステップの固定シーケンスとして実装し、各ステップで指定テンプレートをタイムアウト付きで待ち受け→クリック、を順次実行する。画面アンカーによる状態判定は行わない
3. **冪等な遷移**: 同一状態のテンプレートが連続検出されても二重操作しない
4. **失敗時は安全側に倒す**: 想定外の画面が出たら停止 → 人間に通知 (リソース消費を伴う操作の取りこぼし対策)
5. **ドライランモード必須**: クリック送信を行わず、検出ログのみ出すモードを必ず備える
6. **設定外部化**: テンプレート画像、許容スコア閾値、待機時間などは TOML で外部化

### 2.3 規約上の注意事項 (設計上の前提)

DMM GAMES 利用規約は通常「ツール、bot、自動化ソフトウェアによるアクセス」を禁止しています。本ツールの開発・使用にあたっては以下を前提とします:

- 検証用アカウントでの動作確認のみを推奨し、本番アカウントでの常用は利用者の自己責任
- 配布する場合も「教育目的・学習用途」の明示
- ヒューマンライク化 (クリック間隔のジッタ、座標微小ランダム化) は規約回避目的ではなく安定動作のため

---

## 3. システム構成

### 3.1 レイヤー構造

```
┌──────────────────────────────────────────────────────┐
│  CLI / 設定層                                          │
│  - clap (引数解析)                                     │
│  - config (TOML読込, シリアライズ: serde)              │
│  - tracing / tracing-subscriber (構造化ログ)           │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  オーケストレータ層 (Bot Core)                          │
│  - 順序シーケンスランナー (SequenceRunner)              │
│  - サイクル制御 (run_cycle) + デイリー停止判定          │
│  - 霊晶石ガード / リトライ / タイムアウト管理           │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  ドメイン層                                             │
│  - ステップ定義 (Step enum)                            │
│  - テンプレート定義 (Template struct)                  │
│  - アクション定義 (Action enum)                        │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  プラットフォーム抽象層                                  │
│  ┌─────────────┬──────────────┬────────────────────┐ │
│  │ Window mgmt │ Capture      │ Input              │ │
│  │ (HWND取得)   │ (画面取得)    │ (SendInput)        │ │
│  └─────────────┴──────────────┴────────────────────┘ │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  画像処理層                                             │
│  - image (RgbaImage / GrayImage)                     │
│  - imageproc::template_matching                      │
│  - 座標変換 (画像座標 ↔ スクリーン座標)                  │
└──────────────────────────────────────────────────────┘
                         ↓
┌──────────────────────────────────────────────────────┐
│  OS層 (Windows API)                                  │
│  - windows-rs (Win32: User32, GDI32)                 │
└──────────────────────────────────────────────────────┘
```

### 3.2 採用クレート

| クレート | バージョン目安 | 用途 |
|---------|--------------|------|
| `windows` | 0.58+ | Win32 API バインディング (HWND, SendInput, BitBlt) |
| `image` | 0.25+ | 画像表現 (RgbaImage, GrayImage), PNG読込 |
| `imageproc` | 0.25+ | テンプレートマッチング (`match_template_parallel`) |
| `xcap` | 0.0.x | 高レベル画面キャプチャ (フォールバック) |
| `serde` + `toml` | 1.x / 0.8 | 設定ファイル |
| `clap` | 4.x | CLI引数 (derive) |
| `tracing` + `tracing-subscriber` | 0.1 / 0.3 | ログ |
| `anyhow` + `thiserror` | 1.x | エラー |
| `tokio` | 1.x (rt feature) | 非同期ランタイム (ホットキー監視等で利用、必要に応じて) |
| `rayon` | 1.x | imageproc 内部で利用 (並列化) |
| `chrono` | 0.4 | デイリー切替 (05:00 JST) 判定 |

**依存ポリシー**:
- OpenCV依存は採用しない (純Rust構成、配布時の負担が大きいため)
- GPU依存 (`template-matching` クレート, WGPU) も初期は採用しない (CPUで十分な性能が出る見込み)

---

## 4. 主要コンポーネント詳細設計

### 4.1 モジュール構成

```
dmm-game-bot/
├── Cargo.toml
├── config/
│   ├── default.toml             # デフォルト設定
│   └── templates/               # テンプレート画像 (PNG, グレースケール想定)
│       ├── ap_plus_button.png
│       ├── ap_recovered_use_max.png
│       ├── reisseki_zero_guard.png
│       ├── use_button.png
│       ├── tap_indicator.png
│       ├── toubatsu_button.png
│       ├── toubatsu_start.png
│       ├── next_button.png
│       └── close_button.png
└── src/
    ├── main.rs                  # エントリ、CLI、ログ初期化
    ├── lib.rs                   # 公開API (テスト用)
    ├── config.rs                # 設定構造体、ロード
    ├── error.rs                 # エラー型 (BotError)
    ├── platform/                # OS依存
    │   ├── mod.rs
    │   ├── window.rs            # ウィンドウ列挙、HWND管理、矩形取得
    │   ├── capture.rs           # スクリーンキャプチャ (BitBlt or PrintWindow)
    │   ├── input.rs             # SendInput でクリック送信
    │   └── dpi.rs               # DPI Awareness 設定、座標補正
    ├── vision/                  # 画像処理
    │   ├── mod.rs
    │   ├── template.rs          # Template, TemplateLibrary
    │   ├── matcher.rs           # match_template ラッパー、スコア閾値判定 (ROI 限定対応)
    │   └── coords.rs            # 画像座標 ↔ スクリーン座標変換
    ├── domain/                  # ドメイン
    │   ├── mod.rs
    │   ├── step.rs              # Step enum (ApPlus, UseMax, ReissekiGuard, ...)
    │   └── action.rs            # Action enum (ClickTemplate, WaitForTemplate, AssertTemplate, ...)
    └── bot/                     # オーケストレーション
        ├── mod.rs
        ├── sequence.rs          # 9 ステップ順序実行ランナー
        ├── cycle.rs             # 1周回の制御 + デイリー停止判定
        ├── guard.rs             # 霊晶石ガード (ROI 限定マッチ)
        └── humanize.rs          # クリック座標/タイミングのジッタ
```

### 4.2 platform 層

#### 4.2.1 ウィンドウ管理 (`platform/window.rs`)

**責務**: 対象ブラウザウィンドウの HWND を取得し、現在のクライアント領域矩形 (左上スクリーン座標 + 幅高さ) を提供する。

**主要API**:

```rust
pub struct GameWindow {
    hwnd: HWND,
}

pub struct WindowRect {
    pub screen_x: i32,
    pub screen_y: i32,
    pub width: u32,
    pub height: u32,
}

impl GameWindow {
    /// タイトルパターン (例: "天地狂乱" や URL の一部) で検索
    pub fn find_by_title_substring(pattern: &str) -> Result<Self>;

    /// クライアント領域矩形を取得 (DPI補正後)
    pub fn client_rect(&self) -> Result<WindowRect>;

    /// ウィンドウを最前面に
    pub fn focus(&self) -> Result<()>;
}
```

**Win32 API**:
- `EnumWindows` + `GetWindowTextW` → タイトル一致検索
- `GetClientRect` + `ClientToScreen` → クライアント領域のスクリーン座標
- `SetForegroundWindow` → 最前面化

**ウィンドウ可変対応**: `client_rect()` を毎サイクル呼び出して最新の位置・サイズを取得する。テンプレートマッチングは常にクライアント領域内を対象とする。

#### 4.2.2 画面キャプチャ (`platform/capture.rs`)

**責務**: 指定ウィンドウのクライアント領域をビットマップとして取得し、`image::RgbaImage` を返す。

**実装方式**:
1. **第一候補**: `PrintWindow` API + `PW_RENDERFULLCONTENT` フラグ
   - 非アクティブ・最小化・他ウィンドウに隠れたウィンドウでも取得可能
   - Chrome / Edge は GPU プロセスで描画するため、`PW_RENDERFULLCONTENT` (0x02) が必須
2. **フォールバック**: `BitBlt` (画面 → DC) でデスクトップ全体から切り出す
   - `PrintWindow` が失敗した場合
   - ウィンドウは最前面・非最小化が前提

**主要API**:

```rust
pub trait Capturer {
    fn capture(&self, window: &GameWindow) -> Result<RgbaImage>;
}

pub struct PrintWindowCapturer;
pub struct BitBltCapturer;
```

**注意**: ハードウェアアクセラレーション有効な Chrome では `PrintWindow` で黒画面が返ることがある。その場合 `--disable-gpu` で起動するか、`BitBlt` フォールバック + 最前面化で運用する。

#### 4.2.3 入力エミュレーション (`platform/input.rs`)

**責務**: 指定スクリーン座標にマウス左クリックを送信する。

**主要API**:

```rust
pub trait InputSender {
    fn click_at(&self, screen_x: i32, screen_y: i32) -> Result<()>;
    fn move_to(&self, screen_x: i32, screen_y: i32) -> Result<()>;
}

pub struct SendInputSender;
```

**Win32 API**:
- `SendInput` with `INPUT_MOUSE`
  - `MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE` で移動
  - `MOUSEEVENTF_LEFTDOWN` → 短時間スリープ (50〜100ms ジッタ) → `MOUSEEVENTF_LEFTUP`
- 座標は仮想スクリーン基準の正規化座標 (0〜65535) に変換が必要

**ヒューマンライク化** (`bot/humanize.rs`):
- クリック座標に ±3px 程度のガウシアンジッタ
- ボタン押下時間を 60〜120ms でランダム化
- 連続操作の間に 200〜500ms のランダム待機

#### 4.2.4 DPI 対応 (`platform/dpi.rs`)

- アプリ起動時に `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` を呼び出す
- `GetClientRect` / `ClientToScreen` で得られる座標は物理ピクセル
- `SendInput` の正規化座標も物理ピクセル基準でOK
- マルチモニタ環境では `GetSystemMetrics(SM_CXVIRTUALSCREEN)` でデスクトップ全体サイズを取得して正規化

### 4.3 vision 層

#### 4.3.1 テンプレート (`vision/template.rs`)

```rust
pub struct Template {
    pub name: String,
    pub image: GrayImage,           // グレースケール変換済み
    pub width: u32,
    pub height: u32,
    pub threshold: f32,             // 採用するNCCスコアの閾値
}

pub struct TemplateLibrary {
    templates: HashMap<String, Template>,
}

impl TemplateLibrary {
    pub fn load_from_dir(dir: &Path, config: &TemplateConfig) -> Result<Self>;
    pub fn get(&self, name: &str) -> Option<&Template>;
}
```

**テンプレート画像の作り方ガイドライン** (運用上重要):

1. **対象ウィンドウサイズで切り出す**: ゲームウィンドウのサイズが変わるとピクセル一致しなくなるため、運用するウィンドウサイズを固定するか、複数解像度のテンプレートを用意する
2. **小さく切る**: 大きすぎるテンプレートは誤検出が増え、計算量も増える。ボタン1個分など、特徴的な最小領域に限定する
3. **アニメーション部分を避ける**: TAPインジケータの矢印など光って動く部分は中心の固定文字部分のみを切り出す
4. **背景透過は不要**: グレースケール化するため、不透明PNGでよい

#### 4.3.2 マッチング (`vision/matcher.rs`)

```rust
pub struct Match {
    pub template_name: String,
    pub score: f32,
    pub center_x: u32,      // クライアント領域内座標
    pub center_y: u32,
}

pub struct Matcher;

impl Matcher {
    /// クライアント領域画像内でテンプレートを探索。閾値を超えたものを返す。
    /// 同一テンプレートで最良の1つだけ返す。
    pub fn find(&self, screen: &GrayImage, template: &Template) -> Option<Match>;

    /// 指定領域 (ROI) に限定して探索 (高速化)
    pub fn find_in_roi(&self, screen: &GrayImage, template: &Template, roi: Rect) -> Option<Match>;
}
```

**スコアリング方式**:
- `imageproc::template_matching::MatchTemplateMethod::CrossCorrelationNormalized` (NCC) を採用
- 値域 0.0〜1.0、明るさ変動に頑健
- `match_template_parallel` で rayon 並列化
- 閾値はテンプレートごとに設定 (typically 0.85〜0.95)

**ROI最適化**:
現在のステップに対応するテンプレートのみを既知の ROI に絞って探索する。例えばステップ 1 の `ap_plus_button` は AP 表示の上半分にしか出ないため、画面上部 20% だけ探索すれば十分。`reisseki_zero_guard` のように右端 4 列目に限定が必須なケースもある。これによりCPU時間を大幅削減できる。

### 4.4 domain 層

#### 4.4.1 ステップ定義 (`domain/step.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    ApPlus,           // 1: AP+ アイコン
    UseMax,           // 2: 「最大選択」
    ReissekiGuard,    // 2.5: 霊晶石スロット 0 確認 (ポジティブ確認)
    UseButton,        // 3: 「使用」
    TapIndicator,     // 4: 回復モーダル「TAP」
    Toubatsu,         // 5: 「討伐」(朱色)
    ToubatsuStart,    // 6: 「討伐開始!」
    Next1,            // 7: 戦闘後「次へ」(1 回目)
    Next2,            // 8: 「次へ」(2 回目)
    Close,            // 9: (Optional) 報酬モーダル「閉じる」
}
```

画面アンカーによる状態判定は廃止したため、`Screen` enum は持たない。
順序主導モデルでは「次に押すべきテンプレ」がステップから一意に決まる。

#### 4.4.2 アクション定義 (`domain/action.rs`)

```rust
pub enum Action {
    /// テンプレを `timeout_ms` 内に `poll_ms` 間隔で探索し、見つけたらクリック
    ClickTemplate { template_name: &'static str, timeout_ms: u64, poll_ms: u64 },

    /// クライアント座標固定でクリック (TAP のフォールバック用、現状未使用)
    ClickAtClient { x: u32, y: u32 },

    /// テンプレ消失を確認 (デバウンス用、ステップ 7→8 で使用)
    WaitForTemplateGone { template_name: &'static str, timeout_ms: u64, poll_ms: u64 },

    /// ROI 限定のポジティブ確認 (霊晶石ガード)。マッチしなければ `on_miss` で停止
    AssertTemplate {
        template_name: &'static str,
        roi: Option<RoiPct>,
        timeout_ms: u64,
        on_miss: GuardAction,
    },

    Sleep { ms: u64 },
}

pub enum GuardAction {
    /// 即座に BotError::ReissekiGuardFailed で停止 (クリック発行は一切行わない)
    Abort,
}
```

### 4.5 bot 層

#### 4.5.1 順序ランナー (`bot/sequence.rs`)

```
[Cycle Start]
  │
  ├── デイリー切替判定 (現在時刻 ≥ 05:00 JST かつ前回サイクル外) → [Stop]
  ↓
[Step 1: ApPlus]          WaitForTemplate(ap_plus_button, 60s, 1〜2s) → Click
  ↓
[Step 2: UseMax]          WaitForTemplate(ap_recovered_use_max, 60s, 1〜2s) → Click
  ↓
[Step 2.5: ReissekiGuard] AssertTemplate(reisseki_zero_guard, ROI=右端 4 列目, 数秒)
  │                         └ Miss → ReissekiGuardFailed で即停止 (使用ボタンは絶対に押さない)
  ↓
[Step 3: UseButton]       WaitForTemplate(use_button, 60s, 1〜2s) → Click
  ↓
[Step 4: TapIndicator]    WaitForTemplate(tap_indicator, 60s, 1〜2s) → Click
  ↓
[Step 5: Toubatsu]        WaitForTemplate(toubatsu_button, 60s, 1〜2s) → Click
  ↓
[Step 6: ToubatsuStart]   WaitForTemplate(toubatsu_start, 60s, 1〜2s) → Click
  ↓  ─── ここからゲーム内で 30 分以上の自動周回 ───
  ↓
[Step 7: Next1]           WaitForTemplate(next_button, 45min, 5〜10s) → Click
  ↓
[Debounce]                WaitForTemplateGone(next_button, 60s, 1〜2s)  ← 同一画像連続クリック防止
  ↓
[Step 8: Next2]           WaitForTemplate(next_button, 60s, 1〜2s) → Click
  ↓
[Step 9: Close]           WaitForTemplate(close_button, 30s, 1〜2s)
  │                         ├ Match → Click
  │                         └ Timeout → 正常 (報酬上限到達後パス)
  ↓
[Cycle Complete] → ループ先頭の Step 1 が ap_plus_button 再検出を拾う
```

**順序ランナーの実装方針**:

```rust
pub struct BotEngine<'a> {
    window: &'a GameWindow,
    capturer: &'a dyn Capturer,
    matcher: &'a Matcher,
    input: &'a dyn InputSender,
    templates: &'a TemplateLibrary,
    config: &'a Config,
    clock: &'a dyn Clock,         // デイリー停止判定用 (05:00 JST)
    dry_run: bool,
}

impl<'a> BotEngine<'a> {
    pub fn run_one_cycle(&self) -> Result<CycleReport>;

    pub fn run_loop(&self, max_cycles: Option<u32>) -> Result<()>;

    /// 指定テンプレを timeout_ms 内に poll_ms 間隔で探し、見つけたらクリック
    fn click_template(&self, name: &str, timeout_ms: u64, poll_ms: u64) -> Result<Match>;

    /// 指定テンプレが画面から消えるのを待つ (デバウンス用)
    fn wait_template_gone(&self, name: &str, timeout_ms: u64, poll_ms: u64) -> Result<()>;

    /// ROI 限定でテンプレを 1 度だけ確認。マッチしなければ ReissekiGuardFailed で停止
    fn assert_reisseki_zero(&self) -> Result<()>;
}
```

**実行アルゴリズム**:
状態判定は不要。固定の `Step` シーケンスを `Action` のリストに展開して順次実行するだけ。
各ステップで指定テンプレートのみを探索するため、ROI 限定と合わせて検索コストは最小化される。

#### 4.5.2 サイクル制御 (`bot/cycle.rs`)

```rust
pub struct CycleReport {
    pub started_at: SystemTime,
    pub completed_at: SystemTime,
    pub steps: Vec<StepLog>,
    pub success: bool,
    pub error: Option<String>,
}

pub struct StepLog {
    pub step: Step,                  // Step enum (ApPlus..Close)
    pub elapsed_ms: u64,
    pub matched_score: Option<f32>,
    pub skipped: bool,               // ステップ 9 (close) のタイムアウト = 正常スキップ
}
```

`run_one_cycle` の冒頭でデイリー切替判定 (現在時刻 ≥ 05:00 JST) を行い、
切替済みならサイクルを開始せず正常終了。
ただしステップ 6 (討伐開始) を踏んだ後にデイリー切替が来た場合は、
途中停止すると霊薬を消費しただけで報酬を取り損ねるため、その回は完走させてから停止する。

各サイクルで詳細なログを残し、`tracing` 経由で構造化出力する。

#### 4.5.3 ヒューマナイズ (`bot/humanize.rs`)

```rust
pub fn jitter_click_point(center: (u32, u32), radius: u32) -> (u32, u32);
pub fn random_delay(min_ms: u64, max_ms: u64) -> Duration;
```

ガウシアン分布で中心からのオフセット、一様分布で待機時間。

### 4.6 設定 (`config.rs`)

```toml
# config/default.toml

[window]
title_pattern = "天地狂乱"  # ブラウザのページタイトルにこれが含まれる
# title_pattern_alt = "play.games.dmm.co.jp"

[capture]
method = "print_window"  # "print_window" | "bitblt"

[loop]
max_cycles = 0                       # 0 = 無限

[loop.poll]
default_interval_ms = 1500           # ステップ 1〜5, 7, 8 の通常ポーリング
default_timeout_ms = 60000           # 通常ステップの未検出タイムアウト
post_battle_interval_ms = 7000       # ステップ 6→7 のロング待機 (5〜10秒)
post_battle_timeout_ms = 2700000     # 45 分
close_button_timeout_ms = 30000      # ステップ 9 の上限 (タイムアウトは正常スキップ)
debounce_interval_ms = 1500          # next_button 消失確認

[stop]
daily_cutoff_jst = "05:00"           # デイリー切替で通常停止
finish_in_progress_cycle = true      # ステップ 6 以降を踏んだ周は完走させてから停止

[input]
click_jitter_radius_px = 3
click_press_duration_min_ms = 60
click_press_duration_max_ms = 120

[safety]
dry_run = true                       # デフォルトはドライラン (本番運用時のみfalse)
abort_on_reisseki_guard_miss = true  # 霊晶石ガード失敗時の挙動 (常に true 推奨)

# テンプレート定義 (現状 9 種、ROI は実機 detect-once で再キャリブレーション)
[templates.ap_plus_button]
file = "ap_plus_button.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.ap_recovered_use_max]
file = "ap_recovered_use_max.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.reisseki_zero_guard]
file = "reisseki_zero_guard.png"
threshold = 0.92                     # ガード用は閾値を高めに
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }  # 右端 4 列目に限定 (必須)

[templates.use_button]
file = "use_button.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.tap_indicator]
file = "tap_indicator.png"
threshold = 0.85                     # 光って動く部分は閾値を緩めに
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.toubatsu_button]
file = "toubatsu_button.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.toubatsu_start]
file = "toubatsu_start.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.next_button]
file = "next_button.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }

[templates.close_button]
file = "close_button.png"
threshold = 0.90
roi = { x_pct = "TBD", y_pct = "TBD", w_pct = "TBD", h_pct = "TBD" }
```

ROIは「クライアント領域全体に対する比率」で指定することで、ウィンドウサイズ変動に強くする。
`reisseki_zero_guard` の ROI は他スロットの「0」表示と区別するため、必ず右端 4 列目に限定する。

---

## 5. データフロー (1サイクル)

```
[ループ開始]
  │
  ├── デイリー切替判定 (chrono で JST 現在時刻 vs 05:00) → 切替済みなら停止
  │
  ▼
[各 Step ごとに以下を反復]
  │
  ▼
window.client_rect() ──────────────────→ 矩形 (毎ステップ取得 = ウィンドウ移動対応)
  │
  ▼
capturer.capture(window) ──────────────→ RgbaImage
  │
  ▼
to_grayscale() ────────────────────────→ GrayImage
  │
  ▼
matcher.find_in_roi(image, step.template_name(), step.roi()) ──→ Option<Match>
  │
  ├─ Some(m) → click_at(m.center + jitter) → 次の Step へ
  │
  └─ None → wait poll_ms → step タイムアウト超過なら BotError::TemplateWaitTimeout
  │
  ▼
[Step 2.5 ReissekiGuard だけは例外: クリックせず ROI 限定 Assert のみ]
  └─ Miss → BotError::ReissekiGuardFailed で即停止 (使用ボタンは押さない)
  │
  ▼
[Step 9 Close だけは例外: タイムアウトは正常スキップ]
  │
  ▼
[全 Step 完了 = 1 サイクル完了]
  │
  ▼
[次サイクルへ (= ループ先頭の Step 1 が ap_plus_button を待つ)]
```

---

## 6. CLI設計

```
dmm-game-bot.exe [OPTIONS]

OPTIONS:
  -c, --config <PATH>          設定ファイルパス [default: config/default.toml]
      --dry-run                クリックを送信せず、検出結果のみログ出力
      --max-cycles <N>         実行サイクル数の上限 [default: 0 (無限)]
      --templates-dir <PATH>   テンプレート画像ディレクトリ
      --window-title <STR>     ウィンドウタイトルパターン (設定を上書き)
  -v, --verbose                詳細ログ (-vv でtrace)
      --log-file <PATH>        ログファイル出力先
      --capture-debug <DIR>    キャプチャ画像とマッチング結果を画像保存 (デバッグ用)
  -h, --help
  -V, --version

SUBCOMMANDS:
  run            通常実行 (デフォルト)
  detect-once    1回だけ画面を検出して状態を出力 (テンプレート調整用)
  capture        スクリーンショットを保存して終了 (テンプレート切り出し用)
```

**緊急停止**:
- `Ctrl+C` で SIGINT → 安全に停止
- グローバルホットキー (例: F12) でも停止可能にする (`windows::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey`)

---

## 7. エラーハンドリングと安全性

### 7.1 エラー型階層

```rust
#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error("window not found: {0}")]
    WindowNotFound(String),

    #[error("capture failed: {0}")]
    CaptureFailed(String),

    #[error("template wait timeout: {template} for {elapsed_ms}ms (best score: {best_score})")]
    TemplateWaitTimeout { template: String, elapsed_ms: u64, best_score: f32 },

    #[error("reisseki guard failed: zero-state template did not match — refusing to click 'use' (best score: {best_score})")]
    ReissekiGuardFailed { best_score: f32 },

    #[error("input send failed: {0}")]
    InputFailed(String),

    #[error("config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

### 7.2 安全装置

| 装置 | 動作 |
|------|------|
| ドライランモード | `--dry-run` でクリック送信を完全に無効化、ログのみ |
| テンプレート待機タイムアウト | 通常ステップ 60 秒 / 周回後ステップ 45 分 / close 30 秒 (close のみタイムアウトは正常スキップ) |
| 霊晶石ガード (最優先停止) | ステップ 2.5 の `reisseki_zero_guard` ポジティブ確認に失敗したら即停止、使用ボタンは絶対に押さない |
| デイリー切替停止 | 毎日 05:00 JST にループ終了。ステップ 6 以降を踏んだサイクルは完走させてから停止 |
| Ctrl+C ハンドラ | 進行中のクリック完了後、ループを抜けて正常終了 |
| サイクル数上限 | `--max-cycles 5` などで段階的に検証可能 |
| キャプチャデバッグ出力 | 各検出失敗時にスクショ + 検出スコア画像を出力 |

**霊晶石ガード (最優先停止条件)**:
ステップ 2 の「最大選択」クリック直後、ステップ 3 の「使用」クリック前に、霊晶石スロット (右端 4 列目) の数量カウンタ部分 ROI に対して `reisseki_zero_guard.png` のテンプレートマッチを実施する。

- マッチ成功 (NCC ≥ 0.92) → 霊晶石は未選択と確認 → ステップ 3 へ進行
- マッチ失敗 → 霊晶石が選択されている可能性あり → `ReissekiGuardFailed` で即停止し、「使用」ボタンは絶対に押さない。クリック発行は一切行わずプロセス終了

ROI を右端 4 列目に限定するのは、他スロットの「0」表示と区別するため。NCC 閾値を高め (0.92) に取るのは false-positive で誤って通過させないため。

「最大選択」は AP 不足分を `英気の霊薬(小→中→大) → 霊晶石` の順に自動充填する仕様で、英気の霊薬枯渇時に最大選択 → 使用を押すと課金通貨である霊晶石 (1 個 = 185 AP 回復) が自動消費される。実機で 13.5 万個保有しているような場合、誤動作 1 回でも被害が大きい高リスクポイント。

### 7.3 ログ設計

- `INFO`: サイクル開始/終了、状態遷移、クリック実行
- `DEBUG`: テンプレートマッチングスコア、ROI、待機状況
- `TRACE`: 個別ピクセル比較、キャプチャタイミング
- `WARN`: スコアが閾値ぎりぎり、リトライ発生
- `ERROR`: タイムアウト、捕捉できない例外

ログは `tracing-subscriber` の `EnvFilter` で `RUST_LOG` 環境変数制御。

---

## 8. テスト戦略

### 8.1 単体テスト

| 対象 | テスト内容 |
|------|----------|
| `vision::matcher` | 既知のスクショ + テンプレートでスコアと座標を検証 |
| `vision::matcher` (ROI) | reisseki_zero_guard を ROI 限定で実行し、他スロットの「0」を誤検出しないこと |
| `vision::coords` | クライアント座標 ↔ スクリーン座標変換の境界値 |
| `bot::humanize` | ジッタの分布が想定通り (ガウシアン、レンジ) |
| `bot::sequence` | 9 ステップが正しい順序で起動すること、close タイムアウトが正常スキップ扱いになること |
| `bot::guard` | ReissekiGuard マッチ失敗時にクリック発行が一切起こらないこと |
| `bot::cycle` | デイリー切替判定 (05:00 JST) と進行中サイクル完走方針の境界条件 |

### 8.2 統合テスト

実機ゲーム接続テストは難しいため、**スクショ集を使ったオフライン回帰テスト**を整備する:

```
tests/fixtures/
  cycle_001/                     # 通常パス (close ありの周)
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
  cycle_002/                     # close 出現しない正常スキップパス
    step01_ap_plus.png
    ...
    step08_next2.png
    step09_no_close.png          # close_button が見えない通常画面
  cycle_guard_fail/              # 霊晶石ガード失敗パス
    step01_ap_plus.png
    step02_use_max.png
    step02_5_reisseki_selected.png  # 0 と認識されない画像
```

各スクショを `MockCapturer` で順次返し、順序ランナーが 9 ステップを正しい順序で踏み、デバウンス・霊晶石ガード・close タイムアウトのスキップが期待通りに動くことを assert する。`cycle_guard_fail` では `MockInputSender` がクリック発行回数を記録し、ガード失敗後にクリックが追加で発生していないことを assert する。

### 8.3 E2Eテスト

検証用アカウント + ドライランモードで実機実行し、ログを目視確認する手動テストフェーズを設ける。

---

## 9. 配布とビルド

### 9.1 ビルドコマンド

```bash
# 開発ビルド
cargo build

# リリースビルド (単一exe)
cargo build --release --target x86_64-pc-windows-msvc
```

### 9.2 リリース構成

```
release/
├── dmm-game-bot.exe         # 単一バイナリ
├── config/
│   ├── default.toml
│   └── templates/
│       └── *.png
└── README.md
```

`templates/` は exe と分離して同梱することで、ゲーム側UI変更時にテンプレート差し替えのみで対応可能にする。

### 9.3 静的リンク

`Cargo.toml`:
```toml
[profile.release]
lto = true
codegen-units = 1
strip = true
opt-level = 3
panic = "abort"
```

`.cargo/config.toml`:
```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

これで Visual C++ 再頒布パッケージ不要のスタンドアロンexeになる。

---

## 10. 実装フェーズ計画

| フェーズ | 内容 | 想定工数 (個人開発) |
|--------|------|------------------|
| Phase 0 | プロジェクト雛形、依存セットアップ、CLI骨格 | 0.5日 |
| Phase 1 | platform層: ウィンドウ列挙 + キャプチャ + `capture` サブコマンド | 1.5日 |
| Phase 2 | vision層: テンプレートマッチング、`detect-once` サブコマンド | 1日 |
| Phase 3 | platform/input.rs: SendInput クリック実装、ドライラン対応 | 1日 |
| Phase 4 | bot層: 順序ランナー + サイクル制御 (天地狂乱 9 ステップ) | 2日 |
| Phase 5 | 安全装置 (霊晶石ポジティブ確認ガード、Ctrl+C、デイリー切替停止) | 0.5日 |
| Phase 6 | テスト整備、ログ整備、ドキュメント | 1日 |
| Phase 7 | 実機デバッグ、テンプレート調整 | 1〜2日 |

合計: 8〜10日程度。フェーズ1〜2で「画面を取って状態を当てる」までが見えれば、残りは積み上げ。

---

## 11. 拡張余地 (将来的な発展)

1. **複数イベントへの対応**: 天地狂乱以外のクエストフローを設定ファイル + ステップ列DSLで追加可能に
2. **OCR統合**: AP数値や所持アイテム数を読み取り、より賢い判断 (例: AP最大近くなら使用しない)
3. **WebSocket / IPC**: 他プログラムとの連携 (進捗ダッシュボード)
4. **テンプレート自動学習**: SIFT/ORB特徴量によるスケール不変マッチング
5. **GPUアクセラレーション**: `template-matching` クレート (WGPU) への切り替え
6. **DMM Games Player対応**: ブラウザではなくクライアントアプリ版への対応
7. **複数ウィンドウ並行制御**: 1プロセスから複数アカウントの並列周回

---

## 12. リスクと既知の問題

| リスク | 影響度 | 対策 |
|--------|-------|------|
| 規約違反によるBAN | 高 | 検証アカウントでの利用、本番は自己責任での運用 |
| ゲームUI更新でテンプレート無効化 | 中 | テンプレートを exe と分離、外部差し替え可能に |
| Chrome の `PrintWindow` で黒画面 | 中 | `BitBlt` フォールバック、`--disable-gpu` 起動指示 |
| 高価アイテム (霊晶石) の誤消費 | 高 | reisseki_zero_guard.png によるポジティブ確認をステップ 2.5 で必須実装。マッチ失敗時は使用ボタン押下せず即停止 |
| デイリー切替時刻 (05:00 JST) を跨いだ無駄周回 | 低 | サイクル先頭で時刻判定。ステップ 6 以降進行中なら完走後停止 |
| AP満タンでクエスト不要なときの空打ち | 低 | サイクル開始時にAP値をOCRで読むか、ホーム画面で討伐ボタンが活性化しているかをテンプレート判定 |
| Windows更新でWin32 API挙動変化 | 低 | windows-rs を最新に追従、CIでビルド維持 |

---

## 13. 参考資料

- imageproc::template_matching ドキュメント (docs.rs/imageproc/latest/imageproc/template_matching/)
- Microsoft Learn: SendInput function (Win32 API)
- Microsoft Learn: PrintWindow function (Win32 API, PW_RENDERFULLCONTENT)
- windows-rs クレート (crates.io/crates/windows)

---

**改訂履歴**

| 版 | 日付 | 変更内容 | 作成者 |
|----|------|---------|--------|
| 1.0 | 2026-04-25 | 初版作成 | - |
| 1.1 | 2026-04-25 | 順序主導 9 ステップモデルへ移行 (旧 4 状態機械を廃止)、霊晶石ガードをポジティブ確認方式に反転、デイリー切替 (05:00 JST) 停止条件を追加、テンプレ 9 種に再編 | - |
