use image::GrayImage;
use imageproc::template_matching::{
    find_extremes, match_template_parallel, MatchTemplateMethod,
};

use crate::vision::template::Template;

#[derive(Debug, Clone)]
pub struct Match {
    pub score: f32,
    /// クライアント領域全体での中心座標。
    pub center_x: u32,
    pub center_y: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Matcher;

impl Matcher {
    pub fn new() -> Self {
        Self
    }

    /// ROI 限定探索。第二戻り値は閾値未満も含めた best score (デバッグ用)。
    pub fn find_in_rect(
        &self,
        screen: &GrayImage,
        template: &Template,
        roi: Rect,
    ) -> (Option<Match>, f32) {
        let screen_w = screen.width();
        let screen_h = screen.height();

        let roi_x = roi.x.min(screen_w.saturating_sub(1));
        let roi_y = roi.y.min(screen_h.saturating_sub(1));
        let roi_w = roi.w.min(screen_w.saturating_sub(roi_x));
        let roi_h = roi.h.min(screen_h.saturating_sub(roi_y));

        tracing::debug!(
            "match '{}': search rect ({}, {}) {}x{} on screen {}x{}",
            template.name,
            roi_x,
            roi_y,
            roi_w,
            roi_h,
            screen_w,
            screen_h
        );

        if roi_w < template.width || roi_h < template.height {
            return (None, 0.0);
        }

        // ROI が画面全体を覆う場合は crop コピーを省く (ホットパスのアロケ削減)。
        let is_full = roi_x == 0 && roi_y == 0 && roi_w == screen_w && roi_h == screen_h;
        let result = if is_full {
            match_template_parallel(
                screen,
                &template.image,
                MatchTemplateMethod::CrossCorrelationNormalized,
            )
        } else {
            let sub =
                image::imageops::crop_imm(screen, roi_x, roi_y, roi_w, roi_h).to_image();
            match_template_parallel(
                &sub,
                &template.image,
                MatchTemplateMethod::CrossCorrelationNormalized,
            )
        };

        let extremes = find_extremes(&result);
        let max_value = extremes.max_value;
        let (mx, my) = extremes.max_value_location;

        if max_value >= template.threshold {
            let center_in_roi_x = mx + template.width / 2;
            let center_in_roi_y = my + template.height / 2;
            let m = Match {
                score: max_value,
                center_x: roi_x + center_in_roi_x,
                center_y: roi_y + center_in_roi_y,
            };
            (Some(m), max_value)
        } else {
            (None, max_value)
        }
    }
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}
