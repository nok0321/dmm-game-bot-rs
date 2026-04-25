//! 座標キャッシュ機構。詳細は `DESIGN/11-coord-cache.md` を参照。
//!
//! 静的位置テンプレ (`CACHEABLE_TEMPLATES`) のマッチ中心座標をサイクル間で保持し、
//! サイクル 2 以降は `small_roi` で絞った ROI を `Matcher::find_in_rect` に渡して
//! 探索面積を縮小する。失敗時は呼び出し側が通常 ROI へフォールバックする責務を持つ。
//!
//! 絶対不変条件 (DESIGN/08 §8.2.4 / DESIGN/11 §11.9):
//! - `reisseki_zero_guard` は `CACHEABLE_TEMPLATES` に絶対に含めない。
//! - 動的位置テンプレ (`next_button` / `close_button` / `tap_indicator` /
//!   `ap_recovered_use_max`) も含めない。
//! - クライアント領域 (W, H) が変動したら全エントリを破棄する。

use std::collections::HashMap;

use crate::vision::matcher::Rect;

/// キャッシュ対象テンプレ (コンパイル時定数)。
/// 起動時 TOML で上書きできない (誤設定で霊晶石ガードや動的テンプレが入り込まないため)。
pub const CACHEABLE_TEMPLATES: &[&str] = &[
    "ap_plus_button",
    "use_button",
    "toubatsu_button",
    "toubatsu_start",
];

#[derive(Debug, Clone, Copy)]
pub struct CachedCenter {
    pub center_x: u32,
    pub center_y: u32,
    pub last_score: f32,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CoordCacheStats {
    /// 小 ROI で stability check を通過しクリック発行に至った回数。
    pub hits: u64,
    /// 小 ROI で未検出 → 通常 ROI フォールバックを発動した回数。
    pub small_roi_misses: u64,
    /// フォールバック後に通常 ROI で本物が見つかった回数 (キャッシュは evict 済)。
    pub fallback_succeeded: u64,
    /// フォールバックでも見つからなかった回数。
    pub fallback_failed: u64,
    /// クライアント (W, H) 不一致による全破棄の回数。
    pub invalidations: u64,
}

#[derive(Debug, Default)]
pub struct CoordCache {
    baseline: Option<(u32, u32)>,
    entries: HashMap<String, CachedCenter>,
    stats: CoordCacheStats,
}

impl CoordCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 毎キャプチャ時に呼ぶ。`(client_w, client_h)` が前回観測と異なれば全エントリを破棄。
    pub fn observe(&mut self, client_w: u32, client_h: u32) {
        match self.baseline {
            None => {
                self.baseline = Some((client_w, client_h));
            }
            Some((w, h)) if w == client_w && h == client_h => {}
            Some((w, h)) => {
                let n = self.entries.len();
                self.entries.clear();
                self.baseline = Some((client_w, client_h));
                self.stats.invalidations += 1;
                tracing::warn!(
                    "client size changed {}x{} -> {}x{} - invalidating coord cache (entries={})",
                    w, h, client_w, client_h, n
                );
            }
        }
    }

    /// ホワイトリストに含まれかつエントリが存在する場合のみ Some。
    pub fn lookup(&self, name: &str) -> Option<CachedCenter> {
        if !is_cacheable(name) {
            return None;
        }
        self.entries.get(name).copied()
    }

    /// クリック発行直前に呼ぶ。ホワイトリスト外なら no-op (絶対不変条件保護)。
    pub fn record(&mut self, name: &str, center: CachedCenter) {
        if !is_cacheable(name) {
            return;
        }
        self.entries.insert(name.to_string(), center);
    }

    /// 大 ROI フォールバックで別位置が見つかった時に呼ぶ (古いキャッシュ除去)。
    pub fn evict(&mut self, name: &str) {
        self.entries.remove(name);
    }

    pub fn stats(&self) -> CoordCacheStats {
        self.stats
    }

    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    pub fn note_hit(&mut self) {
        self.stats.hits += 1;
    }

    pub fn note_small_roi_miss(&mut self) {
        self.stats.small_roi_misses += 1;
    }

    pub fn note_fallback_succeeded(&mut self) {
        self.stats.fallback_succeeded += 1;
    }

    pub fn note_fallback_failed(&mut self) {
        self.stats.fallback_failed += 1;
    }
}

fn is_cacheable(name: &str) -> bool {
    CACHEABLE_TEMPLATES.contains(&name)
}

/// 中心座標 + テンプレ寸法 + パディングからクライアント領域内 Rect を生成。
/// i32 空間で計算し負値を 0 でクランプ、その後 `client_w/h` で saturating クランプ。
/// 角ボタン (`cx < pad_px + tpl_w/2`) や 0 寸法クライアントでも安全。
pub fn small_roi(
    center: CachedCenter,
    template_w: u32,
    template_h: u32,
    pad_px: u32,
    client_w: u32,
    client_h: u32,
) -> Rect {
    if client_w == 0 || client_h == 0 {
        return Rect { x: 0, y: 0, w: 0, h: 0 };
    }
    let half_w = template_w / 2;
    let half_h = template_h / 2;
    let left = (center.center_x as i32) - (half_w as i32) - (pad_px as i32);
    let top = (center.center_y as i32) - (half_h as i32) - (pad_px as i32);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(cx: u32, cy: u32) -> CachedCenter {
        CachedCenter { center_x: cx, center_y: cy, last_score: 0.99 }
    }

    #[test]
    fn whitelist_excludes_reisseki_guard() {
        // 絶対不変条件: 霊晶石ガードはキャッシュ対象外。
        assert!(!is_cacheable("reisseki_zero_guard"));
        assert!(!CACHEABLE_TEMPLATES.contains(&"reisseki_zero_guard"));
    }

    #[test]
    fn whitelist_excludes_dynamic_templates() {
        for n in [
            "next_button",
            "close_button",
            "tap_indicator",
            "ap_recovered_use_max",
        ] {
            assert!(
                !is_cacheable(n),
                "expected {} to NOT be cacheable (dynamic position or excluded template)",
                n
            );
        }
    }

    #[test]
    fn record_ignores_non_whitelisted() {
        let mut c = CoordCache::new();
        c.observe(1280, 720);
        c.record("reisseki_zero_guard", cc(100, 100));
        assert!(c.lookup("reisseki_zero_guard").is_none());
        c.record("next_button", cc(100, 100));
        assert!(c.lookup("next_button").is_none());
        // ホワイトリスト内は普通に保存される。
        c.record("ap_plus_button", cc(50, 50));
        assert!(c.lookup("ap_plus_button").is_some());
    }

    #[test]
    fn observe_invalidates_on_size_change() {
        let mut c = CoordCache::new();
        c.observe(1280, 720);
        c.record("ap_plus_button", cc(100, 100));
        assert_eq!(c.entries_len(), 1);
        c.observe(1920, 1080);
        assert_eq!(c.entries_len(), 0);
        assert_eq!(c.stats().invalidations, 1);
    }

    #[test]
    fn observe_same_size_is_noop() {
        let mut c = CoordCache::new();
        c.observe(1280, 720);
        c.record("ap_plus_button", cc(100, 100));
        c.observe(1280, 720);
        assert_eq!(c.entries_len(), 1);
        assert_eq!(c.stats().invalidations, 0);
    }

    #[test]
    fn small_roi_clamps_at_origin() {
        // 角ボタン: cx=10, cy=10 でも underflow せず x=0, y=0 にクランプ。
        let r = small_roi(cc(10, 10), 40, 40, 24, 1280, 720);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
        assert!(r.w > 0);
        assert!(r.h > 0);
    }

    #[test]
    fn small_roi_clamps_at_far_edge() {
        let r = small_roi(cc(1279, 719), 40, 40, 24, 1280, 720);
        assert!(r.x < 1280);
        assert!(r.y < 720);
        assert!(r.x + r.w <= 1280);
        assert!(r.y + r.h <= 720);
    }

    #[test]
    fn small_roi_handles_zero_client() {
        let r = small_roi(cc(100, 100), 40, 40, 24, 0, 0);
        assert_eq!(r.w, 0);
        assert_eq!(r.h, 0);
    }
}
