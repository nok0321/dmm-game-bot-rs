use crate::config::RoiPct;
use crate::vision::matcher::Rect;

/// クライアント領域内座標 → スクリーン座標。
pub fn client_to_screen(
    window_screen_x: i32,
    window_screen_y: i32,
    client_x: i32,
    client_y: i32,
) -> (i32, i32) {
    (window_screen_x + client_x, window_screen_y + client_y)
}

/// 比率指定の ROI をクライアント領域サイズに対する具体ピクセル矩形に変換。
/// クライアント 0×0 (ウィンドウ最小化等) や非有限 RoiPct (NaN/Inf) では空矩形を返し、
/// `Config::validate` を通過した後に動的に作る ROI でも安全側へ倒れるようにする。
pub fn roi_to_rect(roi: &RoiPct, client_w: u32, client_h: u32) -> Rect {
    if client_w == 0 || client_h == 0 {
        return Rect { x: 0, y: 0, w: 0, h: 0 };
    }
    if !roi.x_pct.is_finite() || !roi.y_pct.is_finite()
        || !roi.w_pct.is_finite() || !roi.h_pct.is_finite()
    {
        return Rect { x: 0, y: 0, w: 0, h: 0 };
    }
    let x = (roi.x_pct.clamp(0.0, 1.0) * client_w as f32).round() as u32;
    let y = (roi.y_pct.clamp(0.0, 1.0) * client_h as f32).round() as u32;
    let w = (roi.w_pct.clamp(0.0, 1.0) * client_w as f32).round() as u32;
    let h = (roi.h_pct.clamp(0.0, 1.0) * client_h as f32).round() as u32;
    let w = w.max(1);
    let h = h.max(1);
    let x = x.min(client_w.saturating_sub(1));
    let y = y.min(client_h.saturating_sub(1));
    // x/y が client_w/h 以下である保証 (saturating_sub 後の min 句) のもとで
    // 引き算可能だが、座標キャッシュ等で動的 Rect を作る将来パスにも備えて
    // saturating_sub に統一する (u32 underflow → 巨大 ROI → crop_imm panic 防止)。
    let w = w.min(client_w.saturating_sub(x));
    let h = h.min(client_h.saturating_sub(y));
    Rect { x, y, w, h }
}

