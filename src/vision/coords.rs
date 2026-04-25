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
/// クライアント 0×0 (ウィンドウ最小化等) では空矩形を即返し、後続の算術を回避する。
pub fn roi_to_rect(roi: &RoiPct, client_w: u32, client_h: u32) -> Rect {
    if client_w == 0 || client_h == 0 {
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
    let w = w.min(client_w - x);
    let h = h.min(client_h - y);
    Rect { x, y, w, h }
}

/// クライアント領域全体を覆う Rect。
pub fn full_rect(client_w: u32, client_h: u32) -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: client_w,
        h: client_h,
    }
}
