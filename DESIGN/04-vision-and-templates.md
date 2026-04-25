# 04. 画像処理層 (vision/)

## 4.1 構造体

### Template (`vision/template.rs`)

```rust
pub struct Template {
    pub name: String,
    pub image: GrayImage,    // PNG → to_luma8() でグレースケール化済み
    pub width: u32,
    pub height: u32,
    pub threshold: f32,      // NCC 採用閾値 (0.0〜1.0)
    pub roi: Option<RoiPct>, // クライアント領域に対する比率指定 ROI
}

impl Template {
    pub fn load_from_file(name, path, threshold, roi) -> Result<Self>;
    pub fn resolve_roi(screen_w, screen_h) -> Rect;  // RoiPct → Rect 解決
}
```

PNG をロードし `image::DynamicImage::to_luma8()` でグレースケール化して保持する。
threshold / roi はテンプレ毎に `[templates.<name>]` の TOML 設定から渡される。

### TemplateLibrary (`vision/template.rs`)

```rust
pub struct TemplateLibrary { templates: HashMap<String, Template> }

impl TemplateLibrary {
    pub fn load_from_dir(dir, configs) -> Result<Self>;
    pub fn get(name) -> Option<&Template>;
    pub fn require(name) -> Result<&Template>;     // 無ければ TemplateNotFound
    pub fn names() -> Vec<&str>;
}
```

`load_from_dir` で `[templates.*]` に登録された全テンプレを並列にロードし、
ロード時に `info!("loaded template '{name}' (size={W}x{H}, threshold=..., roi=...)")`
を出力する。テンプレディレクトリが存在しない、ファイルが見つからない場合は
起動時に `BotError::Config` で停止。

### Match (`vision/matcher.rs`)

```rust
pub struct Match {
    pub score: f32,           // NCC スコア (採用閾値以上であることが保証される)
    pub center_x: u32,        // クライアント領域内座標 (ROI オフセット適用後)
    pub center_y: u32,
}

pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
```

### Matcher (`vision/matcher.rs`)

```rust
pub struct Matcher;

impl Matcher {
    pub fn new() -> Self;
    pub fn find_in_rect(
        &self,
        screen: &GrayImage,
        template: &Template,
        roi: Rect,
    ) -> (Option<Match>, f32);  // 第二戻り値は閾値未満も含む best score (デバッグ用)
}
```

## 4.2 マッチングアルゴリズム

採用方式: **NCC (Normalized Cross-Correlation)**
- API: `imageproc::template_matching::match_template_parallel(img, template, MatchTemplateMethod::CrossCorrelationNormalized)`
- 値域: 0.0〜1.0
- 明るさ変動に頑健 (ゲーム内アニメで明度が変わっても安定)
- `match_template_parallel` で内部的に rayon 並列化される

### `find_in_rect` の動作

1. ROI を画面サイズにクランプ:
   ```
   roi_x = roi.x.min(screen_w - 1)
   roi_y = roi.y.min(screen_h - 1)
   roi_w = roi.w.min(screen_w - roi_x)
   roi_h = roi.h.min(screen_h - roi_y)
   ```
2. ROI がテンプレより小さければ即 `(None, 0.0)` を返す。
3. ROI が画面全体を覆う場合は crop コピーを省略 (ホットパスのアロケ削減)。
4. ROI 範囲を `match_template_parallel` で走査し、`find_extremes` で最大値を取る。
5. `max_value >= template.threshold` なら `Match{ score, center_x, center_y }` を返す。
   `(center_x, center_y) = (roi_x + match_x + tpl_w/2, roi_y + match_y + tpl_h/2)`

### `vision/coords.rs`

```rust
pub fn client_to_screen(window_screen_x, window_screen_y, client_x, client_y) -> (i32, i32);
pub fn roi_to_rect(roi: &RoiPct, client_w: u32, client_h: u32) -> Rect;
pub fn full_rect(client_w: u32, client_h: u32) -> Rect;
```

`client_to_screen` は現状 `bot/sequence.rs::click_match` 内でインライン展開
(`rect.screen_x + cx`) しているため呼び出されないが、座標変換ヘルパとして
公開 API に残置している (将来の DSL / モック実装からの再利用に備える)。

`roi_to_rect` の安全装置:
- `client_w==0 || client_h==0` (ウィンドウ最小化等) なら空矩形を即返す。
- `x_pct..h_pct` を `clamp(0.0, 1.0)` してから乗算 → 整数化。
- `w`, `h` は最終的に `max(1)` で 0 を防ぎ、画面端越えはクランプ。

## 4.3 ROI 戦略

ROI は **「クライアント領域全体に対する比率」** (`RoiPct { x_pct, y_pct, w_pct, h_pct }`)
で指定する。ウィンドウサイズが多少変動しても ROI が破綻しない。

現行 `config/default.toml` での主要 ROI:

| テンプレ | ROI | 意図 |
|---|---|---|
| `ap_plus_button` | `(0.00, 0.00, 1.00, 0.30)` | Home 上部 AP 表示帯 |
| `reisseki_zero_guard` | `(0.66, 0.20, 0.30, 0.65)` | **必須**。右端 4 列目スロット限定 (他スロットの「0」を誤マッチしない) |
| `toubatsu_button` | `(0.40, 0.50, 0.60, 0.50)` | Home 右下の朱色「討伐」 |
| `toubatsu_start` | `(0.55, 0.55, 0.45, 0.45)` | PartySelect 画面右下の「討伐開始!」 (戦闘中右下隅 0.9157 偽マッチを抑制) |
| `next_button` | `(0.00, 0.65, 1.00, 0.35)` | 結果画面下部中央 (戦闘画面右下隅の 0.94〜0.95 偽マッチを除外) |
| `close_button` | `(0.20, 0.50, 0.60, 0.45)` | 報酬モーダル下部中央。bottom 5% を除外し、静的偽マッチ (791, 668) を排除 |

`ap_recovered_use_max` / `use_button` / `tap_indicator` は ROI 未設定 (画面全体探索)。
最終ベータ時点で偽マッチ問題が顕在化していなかったため。

## 4.4 閾値設計

実機ログ (2026-04-25 周回継続成立時) では、本物のマッチは概ね **0.99+** で観測。
偽マッチ抑制のため通常テンプレは `0.93〜0.96` 程度に底上げ。

| テンプレ | threshold | 根拠 |
|---|---|---|
| `ap_plus_button` | 0.93 | 通常テンプレ (本物 0.99+) |
| `ap_recovered_use_max` | 0.93 | 同上 |
| `reisseki_zero_guard` | 0.90 | **絶対不変条件: 0.80 未満は `Config::validate` で起動禁止** |
| `use_button` | 0.93 | 通常テンプレ |
| `tap_indicator` | 0.90 | 光って動く部分なので少し緩め |
| `toubatsu_button` | 0.93 | 通常テンプレ |
| `toubatsu_start` | 0.92 | 戦闘中の右下隅 0.9157 偽マッチを切り、本物 1.0 を通す |
| `next_button` | 0.96 | 戦闘画面 0.94〜0.95 の偽マッチを抑制 |
| `close_button` | 0.93 | 静的偽マッチ (791, 668) score=0.9053 を切り、本物 0.9262+ を通す |

霊晶石ガードのみ閾値が低めなのは、**緩めると事故** なので
`Config::validate` で `REISSEKI_GUARD_MIN_THRESHOLD = 0.80` を強制
(下げる方向の事故を機械的に阻止)。

## 4.5 テンプレ画像の作り方ガイドライン

(運用上重要、`templates/README.md` も参照)

1. **対象ウィンドウサイズで切り出す**:
   切り出し元は 1277×693 px (Chrome、DPI 100% 想定)。
   運用ウィンドウサイズが大きく異なる場合は再切り出し or 別解像度テンプレ追加。
2. **小さく切る**:
   ボタン 1 個分など特徴的な最小領域に限定。誤検出と計算量を抑える。
3. **アニメーション部分を避ける**:
   TAP インジケータの矢印など光って動く部分は中心の固定文字部分のみ切り出す。
4. **背景透過は不要**: グレースケール変換するため不透明 PNG で十分。
5. **霊晶石ガード専用の追加要件**:
   - **必ず ROI を設定する** (`Config::validate` が `roi: None` を弾く)。
   - **必ず右端 4 列目スロットに限定する** (他スロットの「0」と区別するため)。
