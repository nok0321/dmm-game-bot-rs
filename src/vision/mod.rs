pub mod template;
pub mod matcher;
pub mod coords;
pub mod coord_cache;

pub use template::{Template, TemplateLibrary};
pub use matcher::{Match, Matcher, Rect};
pub use coord_cache::{small_roi, CachedCenter, CoordCache, CoordCacheStats, CACHEABLE_TEMPLATES};
