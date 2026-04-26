# 11. 座標キャッシュ機構 (CoordCache)

## 11.1 目的とスコープ

サイクル 1 で記録した **静的位置テンプレ** のマッチ中心座標 (center_x, center_y) を、
サイクル 2 以降は **キャッシュ位置周辺の小 ROI** で先に NCC を走らせ、ヒットしなければ
通常 ROI へフォールバックする。NCC 探索面積を縮め CPU と取得遅延を下げるのが目的。

**スコープ (キャッシュ対象 — ホワイトリスト固定):**

| テンプレ | 静的位置の根拠 |
|---|---|
| `ap_plus_button` | Home 上部 AP 表示帯 (動かない) |
| `use_button` | 「最大選択」モーダル内 (位置固定) |
| `toubatsu_button` | Home 右下 (位置固定) |
| `toubatsu_start` | PartySelect 右下 (位置固定) |

**非スコープ (キャッシュ対象外 — ブラックリスト):**

| テンプレ | 除外理由 |
|---|---|
| `reisseki_zero_guard` | 霊晶石ガード安全装置 — ROI 緩和方向の事故を絶対回避 (DESIGN/08 §8.2.4) |
| `next_button` | リザルト系の連続画面で位置が動的に変わる (CHECKPOINT.md) |
| `close_button` | モーダル位置依存 (同上) |
| `tap_indicator` | 光って動く部分でアニメ依存 (DESIGN/04 §4.5.3) |
| `ap_recovered_use_max` | 「最大選択」スクロールで位置が揺れる (画面全体探索) |

**非機能:**

- スレッド共有なし、永続化なし (プロセス内・サイクル間のみ)
- DPI 変更や `capture_method` 変更の直接検出は今回はしない
  (`(client_w, client_h)` 変動で間接的にカバー)

## 11.2 データ構造

`Rect` 型は [`04-vision-and-templates.md`](04-vision-and-templates.md) §4.1
`vision/matcher.rs::Rect { x: u32, y: u32, w: u32, h: u32 }` を再利用する
(本機構は新型を導入しない)。

`src/vision/coord_cache.rs` (新設):

```rust
pub struct CoordCache {
    /// 観測したクライアント領域 (W, H)。これが変わったら全エントリ破棄。
    baseline: Option<(u32, u32)>,
    entries: HashMap<String, CachedCenter>,
    stats: CoordCacheStats,
}

#[derive(Debug, Clone, Copy)]
pub struct CachedCenter {
    pub center_x: u32,
    pub center_y: u32,
    pub last_score: f32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CoordCacheStats {
    pub hits: u64,                  // 小ROI で stability check を抜けクリック発行に至った回数
    pub small_roi_misses: u64,      // 小ROI で未検出 → 大ROIへフォールバックした回数
    pub fallback_succeeded: u64,    // 大ROI フォールバックで本物が見つかった回数
    pub fallback_failed: u64,       // 大ROI フォールバックも空振りで終わった回数
    pub invalidations: u64,         // baseline 不一致による全破棄の回数
}
```

公開 API:

```rust
impl CoordCache {
    pub fn new() -> Self;

    /// 毎キャプチャ前に呼ぶ。`(client_w, client_h)` が前回と異なれば全エントリ破棄。
    pub fn observe(&mut self, client_w: u32, client_h: u32);

    /// ホワイトリストに含まれ、かつエントリが存在する場合のみ Some。
    pub fn lookup(&self, name: &str) -> Option<CachedCenter>;

    /// クリック発行直前に呼ぶ。ホワイトリスト外なら無視 (no-op)。
    pub fn record(&mut self, name: &str, center: CachedCenter);

    /// 大ROIフォールバックで別位置が見つかった時に呼ぶ (古いキャッシュを除去)。
    pub fn evict(&mut self, name: &str);

    pub fn stats(&self) -> CoordCacheStats;
    pub fn entries_len(&self) -> usize;

    // 統計カウンタ更新用 (CoordCacheStats のフィールドを公開せずカプセル化)。
    pub fn note_hit(&mut self);
    pub fn note_small_roi_miss(&mut self);
    pub fn note_fallback_succeeded(&mut self);
    pub fn note_fallback_failed(&mut self);
}

/// ホワイトリスト (固定)。
pub const CACHEABLE_TEMPLATES: &[&str] = &[
    "ap_plus_button",
    "use_button",
    "toubatsu_button",
    "toubatsu_start",
];

/// 小ROI を中心座標 + テンプレサイズ + パディングから生成。
/// クライアント境界でクランプし、u32 underflow / 巨大 Rect を生成しない。
pub fn small_roi(
    center: CachedCenter,
    template_w: u32, template_h: u32,
    pad_px: u32,
    client_w: u32, client_h: u32,
) -> Rect;
```

## 11.3 ホワイトリスト/ブラックリスト方針

- ホワイトリストは `CACHEABLE_TEMPLATES` に **コンパイル時定数** として固定。
  起動時 TOML で上書きできない (誤設定で霊晶石ガードや動的テンプレが入り込まないため)。
- `lookup` / `record` は **`CACHEABLE_TEMPLATES` に含まれない名前を即無視**。
  この振る舞いをユニットテストで機械的に検証する (§11.10)。

## 11.4 失効条件 (invalidation)

`CoordCache::observe(client_w, client_h)` を `try_click_template` のループ前に 1 回呼ぶ。

| 状況 | 動作 |
|---|---|
| `baseline = None` | 初回観測 → baseline を設定するだけ (エントリは空のまま) |
| `baseline = Some((w, h)) == (client_w, client_h)` | no-op |
| `baseline = Some((w, h)) != (client_w, client_h)` | **全エントリ破棄** + `stats.invalidations += 1` + `warn!` |

ウィンドウのスクリーン座標 (left/top) は **キャッシュキーに含めない**:
クライアント内座標は不変で、`click_match` が `client_rect()` を都度取得して
スクリーン座標へ変換しているため、ウィンドウ移動はクリック先には影響しない。

DPI / `capture_method` 変動の直接検出は本機構の対象外。実機運用上は
`(client_w, client_h)` 変動を伴うことが多く、二次的にカバーされる。

## 11.5 小 ROI 生成

```rust
pub fn small_roi(
    center: CachedCenter,
    template_w: u32, template_h: u32,
    pad_px: u32,
    client_w: u32, client_h: u32,
) -> Rect {
    let half_w = template_w / 2;
    let half_h = template_h / 2;
    // i32 空間で「左上」を計算してから 0 でクランプ (角ボタンで underflow しない)。
    let left = (center.center_x as i32) - (half_w as i32) - (pad_px as i32);
    let top  = (center.center_y as i32) - (half_h as i32) - (pad_px as i32);
    let x = left.max(0) as u32;
    let y = top.max(0) as u32;
    let w = template_w.saturating_add(pad_px.saturating_mul(2));
    let h = template_h.saturating_add(pad_px.saturating_mul(2));
    Rect {
        x: x.min(client_w.saturating_sub(1)),
        y: y.min(client_h.saturating_sub(1)),
        w: w.min(client_w.saturating_sub(x)),
        h: h.min(client_h.saturating_sub(y)),
    }
}
```

整地済み前提:

- `vision/coords.rs::roi_to_rect` は `saturating_sub` に統一済み (ROB-1)
- `vision/matcher.rs::find_in_rect` は `roi_w == 0 || roi_h == 0 || template.{w,h} == 0`
  で早期 return (ROB-2)
- `roi_to_rect` の NaN 防御も追加済み (ROB-9)

## 11.6 統合点 (`bot/sequence.rs::try_click_template`)

```text
let tpl = templates.require(name)?;
let baseline_w, baseline_h = capture (gray) で 1 度確定
coord_cache.observe(baseline_w, baseline_h)

loop {
    gray = capture_gray()                       // baseline と (W, H) が一致する前提
    if (gray.w, gray.h) != (baseline_w, baseline_h):
        coord_cache.observe(gray.w, gray.h)     // 念のため再観測 (リサイズ対応)
        baseline_w, baseline_h = gray.w, gray.h

    let cached = if cache_enabled then coord_cache.lookup(name) else None
    let used_small_roi = false

    let (matched, score, applied_roi) = match cached:
        Some(cc):
            let small = small_roi(cc, tpl.w, tpl.h, pad_px, gray.w, gray.h)
            let r = matcher.find_in_rect(gray, tpl, small)
            used_small_roi = true
            (r.0, r.1, small)
        None:
            let full = tpl.resolve_roi(gray.w, gray.h)
            let r = matcher.find_in_rect(gray, tpl, full)
            (r.0, r.1, full)

    // 小ROIで見つからなかったら通常ROIへフォールバック (1 度だけ)
    if matched.is_none() && used_small_roi:
        coord_cache.note_small_roi_misses += 1
        let full = tpl.resolve_roi(gray.w, gray.h)
        let (m2, s2) = matcher.find_in_rect(gray, tpl, full)
        if let Some(_) = m2:
            coord_cache.note_fallback_succeeded += 1
            coord_cache.evict(name)              // 古いキャッシュを除去
        else:
            coord_cache.note_fallback_failed += 1
        // 以降 m2/s2 を採用
        matched, score = (m2, s2)
        applied_roi = full

    // 既存の stability check ロジック (DESIGN/03 §3.6) を **そのまま** 通過させる
    ... stable_count, last_match の更新 ...

    // クリック発行直前 (stable_count >= stability_required) でキャッシュ更新
    if click_about_to_fire {
        coord_cache.record(name, CachedCenter { center_x, center_y, last_score: m.score })
        if used_small_roi: coord_cache.note_hits += 1
        click_match(m)
        return Ok((Some(m), best_score))
    }
}
```

**stability check は既定で緩和しない**:
PRJ-3 リスク (リザルト系で偽マッチを 1 フレーム拾った瞬間に発火) を避けるため、
キャッシュヒット時も `stability_count >= stability_required` を維持。
将来の最適化用に Config フラグ `relax_stability_on_hit` を用意するが **既定 false**。

## 11.7 Config 追加

`config/default.toml` に新節:

```toml
[loop.coord_cache]
enabled = true
search_pad_px = 24
relax_stability_on_hit = false
```

Rust 側 (`src/config.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordCacheConfig {
    #[serde(default = "default_coord_cache_enabled")]
    pub enabled: bool,
    /// キャッシュ中心 ± この値 px をテンプレ寸法に加えた範囲を小 ROI とする。
    #[serde(default = "default_coord_cache_search_pad_px")]
    pub search_pad_px: u32,
    /// キャッシュヒット時に stability_count を緩和するか (既定 false; 安全側)。
    #[serde(default = "default_coord_cache_relax_stability_on_hit")]
    pub relax_stability_on_hit: bool,
}

fn default_coord_cache_enabled() -> bool { true }
fn default_coord_cache_search_pad_px() -> u32 { 24 }
fn default_coord_cache_relax_stability_on_hit() -> bool { false }
```

`LoopConfig` に `pub coord_cache: CoordCacheConfig` を追加 (`#[serde(default)]`)。

**バリデーション (`Config::validate` で機械的に強制):**

- `search_pad_px ∈ [COORD_CACHE_MIN_PAD_PX, COORD_CACHE_MAX_PAD_PX] = [1, 256]`
  範囲外で `BotError::Config`。下限 0 はキャッシュ位置のわずかなズレを許容できず、
  上限超は通常 ROI と差がなくなり機構意義が薄れるため。

`enabled = false` で機構を完全 bypass (デバッグ・回帰検証用)。

## 11.8 観測性

**サイクル末サマリ (INFO):**

```
coord cache: hits=5 misses=1 (recovered=1 still_missing=0) invalidations=0 entries=4
```

| 表示語 | 内部フィールド | 意味 |
|---|---|---|
| `hits` | `stats.hits` | 小 ROI で stability check を通過しクリックに至った回数 |
| `misses` | `stats.small_roi_misses` | 小 ROI で未検出 → 大 ROI へフォールバック発動した回数 |
| `recovered` | `stats.fallback_succeeded` | 大 ROI フォールバックで本物が見つかった回数 (cache evict 済) |
| `still_missing` | `stats.fallback_failed` | 大 ROI でも見つからなかった回数 (画面遷移途中の可能性、次 poll で retry) |
| `invalidations` | `stats.invalidations` | クライアント (W,H) 変動で全エントリを破棄した回数 |
| `entries` | `entries.len()` | 現在キャッシュに乗っているテンプレ数 |

`run_one_cycle` の return 直前に 1 行出す (累積、サイクル間で持続)。

**個別イベント:**

| 場面 | レベル | 例 |
|---|---|---|
| キャッシュヒット (stable で click 発行直前) | INFO | `info!("{name} cache hit: clicked at ({x},{y}) score={:.4}")` |
| 小 ROI ミス → 大 ROI フォールバック開始 | INFO | `info!("{name} small ROI miss (best={:.4} < threshold={:.4}) — falling back to full ROI")` |
| 大 ROI フォールバック成功 | INFO | `info!("{name} fallback recovered (score={:.4}) — refreshing cache on stable click")` |
| 大 ROI フォールバックも空振り | WARN | `warn!("{name} fallback also missed (best={:.4} < threshold={:.4}) — likely transition not settled, will retry")` |
| baseline 変動による全破棄 | WARN | `warn!("client size changed {Wp}x{Hp} → {Wn}x{Hn} — invalidating coord cache (entries={N})")` |
| 通常 ROI 探索 (キャッシュ無し) | DEBUG | 既存 `match '{name}': search rect ...` をそのまま流用 |

**`detect-once` サブコマンドへの影響なし:**
`detect-once` はステートレスな単発探索のため、キャッシュは利用しない。

## 11.9 不変条件サマリ

| 不変条件 | 保証手段 |
|---|---|
| 霊晶石ガード経路がキャッシュを使わない | (1) `do_assert_reisseki_zero` は `try_click_template` を呼ばない (構造分離); (2) `CACHEABLE_TEMPLATES` に `reisseki_zero_guard` を含めない (テストで検証) |
| 動的位置テンプレがキャッシュを使わない | `CACHEABLE_TEMPLATES` の固定リスト + ホワイトリスト網羅テスト |
| ウィンドウサイズ変動で全エントリ破棄 | `CoordCache::observe` で `(W,H)` 一致のみ保持 + テスト |
| stability check 緩和は既定 OFF | `default_coord_cache_relax_stability_on_hit() == false` をテストで固定 |
| クライアント 0×0 / 角ボタン / 巨大 pad で安全 | `roi_to_rect` 早期 return + `small_roi` の i32 空間計算 + `find_in_rect` の 0 寸法ガード |
| `safety.dry_run` 既定 true / JST ログ / `REISSEKI_GUARD_MIN_THRESHOLD` 0.80 | 本機構は SafetyConfig / cycle::jst_offset / Config::validate を **触らない** |

## 11.10 テスト計画

`vision/coord_cache.rs` の `#[cfg(test)] mod tests` に下記を追加:

| テスト名 | 検証内容 |
|---|---|
| `whitelist_excludes_reisseki_guard` | `CACHEABLE_TEMPLATES` に `reisseki_zero_guard` が含まれない |
| `whitelist_excludes_dynamic_templates` | `next_button` / `close_button` / `tap_indicator` / `ap_recovered_use_max` 非含有 |
| `record_ignores_non_whitelisted` | `record("reisseki_zero_guard", _)` 後でも `lookup` が None を返す (ホワイトリスト内は保存される) |
| `observe_invalidates_on_size_change` | `(1280,720) → (1920,1080)` で全エントリ削除 + `invalidations += 1` |
| `observe_same_size_is_noop` | 同サイズ再観測で entries 保持 + invalidations 不変 |
| `small_roi_clamps_at_origin` | `cx=10, cy=10, pad=24` でも `x>=0, y>=0` (パニック / underflow なし) |
| `small_roi_clamps_at_far_edge` | `cx=client_w-1, cy=client_h-1, pad=24` で `w/h` がクランプされる |
| `small_roi_handles_zero_client` | `client_w==0 || client_h==0` でも矩形が成立 (`w>=0, h>=0`) |

`src/config.rs` の `#[cfg(test)] mod tests` に追加:

| テスト名 | 検証内容 |
|---|---|
| `validate_rejects_zero_search_pad` | `search_pad_px = 0` で `Config::validate` が Err |
| `validate_rejects_huge_search_pad` | `search_pad_px = 257` で Err |
| `coord_cache_default_round_trip` | `[loop.coord_cache]` 省略時の既定値 (`enabled=true, pad=24, relax=false`) と バリデーション通過を一括検証。`relax_stability_on_hit` 既定 false の回帰防止もここで担う。 |

合計: 既存 9 件 + 8 件 (coord_cache) + 3 件 (config) = **20 件**。

## 11.11 既存ファイルとの統合変更

| ファイル | 変更内容 |
|---|---|
| `src/vision/coord_cache.rs` | **新設** (上記 §11.2–11.5, 11.10 を実装) |
| `src/vision/mod.rs` | `pub mod coord_cache;` 追加、必要なら `pub use coord_cache::*;` |
| `src/config.rs` | `CoordCacheConfig` + `LoopConfig::coord_cache` + `Config::validate` 追記 |
| `src/bot/sequence.rs` | `BotEngine` に `coord_cache: RefCell<CoordCache>` 追加、`try_click_template` で参照 |
| `config/default.toml` | `[loop.coord_cache]` 節追加 (現行値の明示) |
| `DESIGN/04-vision-and-templates.md` | 新節 §4.6 で本書への相互参照 (1〜2 段落) |
| `DESIGN/06-config-schema.md` | 既存 §6.5 の隣に §6.5.1 として `[loop.coord_cache]` を追記 |
| `DESIGN/02-architecture.md` | `vision/coords.rs` 行の docline (`/ full_rect /`) を実装と同期 (整地で削除済) |

## 11.12 受け入れ基準

- `cargo build --release` 成功
- `cargo clippy --workspace --all-targets` 警告 0
- `cargo test --workspace` で **20 件 PASS** (新規 11 件含む)
- `target/release/dmm-game-bot.exe -v --max-cycles 2 --live` で:
  - サイクル 1: cache hits=0, fallback_succeeded=0 (cold start)
  - サイクル 2: 4 種テンプレで cache hits=4 を観測
  - サマリログが各サイクル末で出る
- 既存 9 ステップフローの挙動 (霊晶石ガード PASS、Step 6 の post_battle_min_wait、
  Next1/Next2 のハード sleep) が変わらない

## 11.13 不採用案 (FYI)

- **`(screen_x, screen_y)` をキャッシュキーに含める**:
  ROB-6 で挙がった「ウィンドウ移動で stale」は実害がないため不採用
  (`click_match` が毎回 `client_rect()` を再取得するロジックを既に持つ)。
- **キャッシュ ROI でのスコアが N% 低下したら invalidate**:
  実装複雑度に対する利得が読めないため見送り。
  代わりに「小 ROI ミス → フォールバック → evict」の単純な反応で十分。
- **永続化 (`CHECKPOINT.md` のように .json で保存)**:
  サイクル間/プロセス間のクライアント領域同一性を保証できないため不採用。
