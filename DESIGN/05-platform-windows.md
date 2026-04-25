# 05. プラットフォーム抽象層 (platform/)

Windows 専用ロジックを `#[cfg(windows)]` で囲み、それ以外の OS には
`stub_impl` でビルドだけ通すスタブを置いている (実行は Windows のみ可)。

## 5.1 ウィンドウ管理 (`platform/window.rs`)

### 構造体

```rust
pub struct WindowRect {
    pub screen_x: i32,
    pub screen_y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub struct GameWindow { hwnd: HWND }

impl GameWindow {
    pub fn find_by_title_substring(pattern: &str) -> Result<Self>;
    pub fn client_rect(&self) -> Result<WindowRect>;  // DPI 補正後の物理ピクセル
    pub fn focus(&self) -> Result<()>;
    pub(crate) fn raw(&self) -> HWND;                 // capture から使用
}
```

### Win32 API 呼び出し

| 動作 | API |
|---|---|
| ウィンドウ列挙 | `EnumWindows` + `GetWindowTextLengthW` + `GetWindowTextW` |
| 可視判定 | `IsWindowVisible` (不可視ウィンドウはスキップ) |
| クライアント領域取得 | `GetClientRect` + `ClientToScreen(POINT(0,0))` |
| 最前面化 | `SetForegroundWindow` |

タイトル検索は **部分一致** (`title.contains(pattern)`)。
`config.window.title_pattern` (例: "あやかしランブル") を含むタイトルを持つ
最初の可視ウィンドウを返す。見つからなければ `BotError::WindowNotFound`。

### ウィンドウ可変対応

`client_rect()` は **毎クリック・毎キャプチャ呼び出し** することで、
ウィンドウのドラッグ移動・リサイズに対応する。テンプレートマッチング結果の
クライアント座標 → スクリーン座標変換でも、その時点の rect を使う。

## 5.2 画面キャプチャ (`platform/capture.rs`)

### Capturer trait

```rust
pub trait Capturer {
    fn capture(&self, window: &GameWindow) -> Result<RgbaImage>;
}

pub struct PrintWindowCapturer;
pub struct BitBltCapturer;

pub fn build_capturer(method: CaptureMethod) -> Box<dyn Capturer + Send + Sync>;
```

`config.capture.method = "print_window" | "bitblt"` で切り替え。
ベータ運用は `print_window` (失敗時 BitBlt フォールバック) が既定。

### PrintWindowCapturer (推奨)

```text
1. GetWindowRect(hwnd) でウィンドウ全体の矩形を取得 (win_w, win_h)
2. GetDC(hwnd) → CreateCompatibleDC → CreateCompatibleBitmap(win_w, win_h)
3. PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT=0x02))
   ↑ Chrome / Edge の GPU プロセス描画でも安定して取得するため必須フラグ
4. GetDIBits で BGRA 32bit ピクセル抽出 → BGRA→RGBA スワップ → RgbaImage
5. クライアント領域オフセットでクロップ:
   off_x = client.screen_x - win_rect.left
   off_y = client.screen_y - win_rect.top
   crop_imm(full, off_x, off_y, client.w, client.h).to_image()
6. PrintWindow 失敗時 (黒画面・エラー) は BitBlt フォールバック
```

**v1.1 → 現行ベータの変更点**:
v1.1 は「クライアント領域だけ PrintWindow に投げる」設計だったが、
Chrome の GPU プロセス描画では境界部分が欠ける問題があったため、
**ウィンドウ全体を PrintWindow → クライアント領域でクロップ** に変更。

### BitBltCapturer (フォールバック)

```text
1. GetDC(NULL) でデスクトップ全体の DC を取得
2. CreateCompatibleDC + CreateCompatibleBitmap(client.w, client.h)
3. BitBlt(mem_dc, 0,0, w,h, screen_dc, client.screen_x, client.screen_y, SRCCOPY)
4. GetDIBits → BGRA→RGBA → RgbaImage
```

最前面・非最小化が前提。他ウィンドウに隠れていると隠している側が映る。

### 共通ヘルパ `extract_pixels`

`BITMAPINFOHEADER.biHeight = -(height as i32)` で **トップダウン** ピクセル順を要求し、
`GetDIBits` の出力をそのまま `RgbaImage::from_raw` に渡せるようにしている。
リソースは GDI ハンドルを `DeleteObject` / `DeleteDC` / `ReleaseDC` で解放。

## 5.3 入力エミュレーション (`platform/input.rs`)

### InputSender trait

```rust
pub trait InputSender: Send + Sync {
    fn click_at(&self, screen_x: i32, screen_y: i32, press_duration_ms: u64) -> Result<()>;
}

pub struct DryRunSender;
pub struct SendInputSender;
```

`BotEngine::new` で `dry_run` フラグから両者を切り替える。

### DryRunSender

```rust
fn click_at(screen_x, screen_y, _press_duration_ms) -> Ok(()):
    warn!("[DRY-RUN] click suppressed at screen ({sx}, {sy}) — pass --live to actually send");
```

クリック送信を **完全に無効化** (副作用なし)。ログだけ残す。

### SendInputSender

```rust
fn click_at(screen_x, screen_y, press_duration_ms) -> Result<()>:
  (nx, ny) = normalize(screen_x, screen_y)         // 仮想スクリーン基準の 0〜65535
  send([INPUT_MOUSE { dx: nx, dy: ny,
        flags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK }])?
  sleep(20ms)
  send([INPUT_MOUSE { flags: MOUSEEVENTF_LEFTDOWN }])?
  sleep(press_duration_ms)                          // 60〜120ms ランダム
  send([INPUT_MOUSE { flags: MOUSEEVENTF_LEFTUP }])?
```

`MOUSEEVENTF_VIRTUALDESK` を付けることで `SM_XVIRTUALSCREEN`/`SM_YVIRTUALSCREEN` 起点の
正規化座標としてマルチモニタ環境でも正しい座標に飛ぶ。

`normalize` の実装 (`SM_*VIRTUALSCREEN` 物理ピクセル → 0..=65535):

```rust
let vw = vw.max(1);                              // 仮想スクリーン幅 0 を防ぐ
let vh = vh.max(1);
let nx = ((screen_x - vx) as i64) * 65535 / (vw - 1).max(1) as i64;  // i64 で桁あふれ回避
let ny = ((screen_y - vy) as i64) * 65535 / (vh - 1).max(1) as i64;
(nx as i32, ny as i32)
```

`i64` キャスト + `vw/vh` の `max(1)` + `(vw - 1).max(1)` の **三重防御** で、
極端なマルチモニタ構成 (vw=0 / vw=1 / 4K×4 等) でも 0 除算・桁あふれを起こさない。

`SendInput` の戻り値が要求イベント数と一致しなければ `BotError::InputFailed`。

### ヒューマナイズ (`bot/humanize.rs`)

```rust
pub fn jitter_click_point(center: (i32,i32), radius: u32) -> (i32, i32);
pub fn random_press_duration_ms(min_ms: u64, max_ms: u64) -> u64;
pub fn random_delay(min_ms: u64, max_ms: u64) -> Duration;
```

- `jitter_click_point`: `radius==0` なら center をそのまま返す。
  `radius` を `i32::MAX` で飽和させて `gen_range(-r..=r)` の panic を防ぐ。
  分布は **一様** (`gen_range`) で実装している (v1.1 の「ガウシアン」記述とは異なる)。
- `random_press_duration_ms`: `min..=max` 一様。`max <= min` のケースは `min` を返す。
- `random_delay`: `random_press_duration_ms` の `Duration` ラッパ。

呼び出し元 (`sequence.rs::try_click_template`):
- `pre_click_min_ms..=max_ms` (既定 150〜300ms): テンプレ検出 → クリック発行 の間。
  アニメーション完了待ち。
- `post_click_min_ms..=max_ms` (既定 400〜800ms): クリック発行 → 次の検出開始 の間。

## 5.4 DPI 対応 (`platform/dpi.rs`)

```rust
#[cfg(windows)]
pub fn set_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}
```

`cli::main` の最初期に 1 回だけ呼ぶ。これにより:

- `GetClientRect` / `ClientToScreen` の戻り値が **物理ピクセル** になる。
- `SendInput` の正規化座標も物理ピクセル基準で正しく動く。
- 125% / 150% スケーリング環境でもオフセットがずれない。

戻り値はチェックしない (1 回しか呼ばれず、失敗してもデフォルト挙動が
残るだけのため)。

## 5.5 既知の運用上の落とし穴

| 症状 | 原因 | 対策 |
|---|---|---|
| PrintWindow で黒画面 | Chrome のハードウェアアクセラレーション無効化されたタイミング | BitBlt フォールバック発動。または `--disable-gpu` 起動 |
| クリック座標がずれる | DPI Awareness 未設定 / マルチモニタで `MOUSEEVENTF_VIRTUALDESK` 漏れ | `set_dpi_aware()` 呼び出し確認、`MOUSEEVENTF_VIRTUALDESK` フラグ確認 |
| ウィンドウが見つからない | タイトル文字列の不一致 (全角/半角、別バージョン UI) | `--window-title <STR>` で部分一致パターンを上書き |
| `client_rect()` が 0×0 を返す | ウィンドウ最小化中 | `roi_to_rect` で空矩形を返してマッチをスキップ (実害なし) |
