use thiserror::Error;

pub type Result<T> = std::result::Result<T, BotError>;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("window not found: {0}")]
    WindowNotFound(String),

    #[error("capture failed: {0}")]
    CaptureFailed(String),

    #[error("template wait timeout: {template} for {elapsed_ms}ms (best score: {best_score:.4})")]
    TemplateWaitTimeout {
        template: String,
        elapsed_ms: u64,
        best_score: f32,
    },

    #[error("reisseki guard failed: zero-state template did not match — refusing to click 'use' (best score: {best_score:.4})")]
    ReissekiGuardFailed { best_score: f32 },

    #[error("input send failed: {0}")]
    InputFailed(String),

    #[error("template not found in library: {0}")]
    TemplateNotFound(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("other: {0}")]
    Other(String),
}

impl BotError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
