use std::collections::HashMap;
use std::path::Path;

use image::{GrayImage, ImageReader};

use crate::config::{RoiPct, TemplateConfig};
use crate::error::{BotError, Result};
use crate::vision::coords::{full_rect, roi_to_rect};
use crate::vision::matcher::Rect;

#[derive(Debug)]
pub struct Template {
    pub name: String,
    pub image: GrayImage,
    pub width: u32,
    pub height: u32,
    pub threshold: f32,
    pub roi: Option<RoiPct>,
}

impl Template {
    pub fn load_from_file(
        name: &str,
        path: &Path,
        threshold: f32,
        roi: Option<RoiPct>,
    ) -> Result<Self> {
        let img = ImageReader::open(path)
            .map_err(|e| BotError::other(format!("open template {}: {}", path.display(), e)))?
            .decode()?;
        let gray = img.to_luma8();
        let (w, h) = (gray.width(), gray.height());
        Ok(Self {
            name: name.to_string(),
            image: gray,
            width: w,
            height: h,
            threshold,
            roi,
        })
    }

    /// テンプレの ROI を画面サイズに対する具体ピクセル矩形に解決する。
    /// `roi` が `None` の場合は画面全体を返す。
    pub fn resolve_roi(&self, screen_w: u32, screen_h: u32) -> Rect {
        match self.roi {
            Some(r) => roi_to_rect(&r, screen_w, screen_h),
            None => full_rect(screen_w, screen_h),
        }
    }
}

pub struct TemplateLibrary {
    templates: HashMap<String, Template>,
}

impl TemplateLibrary {
    pub fn load_from_dir(
        dir: &Path,
        configs: &HashMap<String, TemplateConfig>,
    ) -> Result<Self> {
        if !dir.exists() {
            return Err(BotError::Config(format!(
                "templates dir does not exist: {}",
                dir.display()
            )));
        }

        let mut templates = HashMap::new();
        for (name, cfg) in configs {
            let path = dir.join(&cfg.file);
            if !path.exists() {
                return Err(BotError::Config(format!(
                    "template file not found: {} (for '{}')",
                    path.display(),
                    name
                )));
            }
            let tpl = Template::load_from_file(name, &path, cfg.threshold, cfg.roi)?;
            tracing::info!(
                "loaded template '{}' (size={}x{}, threshold={:.4}, roi={:?})",
                name,
                tpl.width,
                tpl.height,
                tpl.threshold,
                tpl.roi
            );
            templates.insert(name.clone(), tpl);
        }
        Ok(Self { templates })
    }

    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    pub fn require(&self, name: &str) -> Result<&Template> {
        self.templates
            .get(name)
            .ok_or_else(|| BotError::TemplateNotFound(name.to_string()))
    }

    pub fn names(&self) -> Vec<&str> {
        self.templates.keys().map(|s| s.as_str()).collect()
    }
}
