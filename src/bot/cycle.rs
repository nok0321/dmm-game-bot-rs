use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveTime, TimeZone, Utc};

use crate::domain::step::StepLog;
use crate::error::{BotError, Result};

#[derive(Debug, Clone)]
pub struct CycleReport {
    pub started_at: DateTime<FixedOffset>,
    pub completed_at: DateTime<FixedOffset>,
    pub steps: Vec<StepLog>,
    pub success: bool,
    pub error: Option<String>,
}

pub fn jst_offset() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid JST offset")
}

pub fn now_jst() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&jst_offset())
}

pub fn parse_cutoff_hh_mm(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .map_err(|e| BotError::Config(format!("invalid daily_cutoff_jst {:?}: {}", s, e)))
}

/// 与えられた `start` 時刻 (JST) を基準に、その時刻より「後」の
/// 直近の `cutoff` (時刻 HH:MM) を返す。`start` がすでに当日の cutoff を超えていれば翌日の cutoff。
pub fn next_cutoff_after(
    start: DateTime<FixedOffset>,
    cutoff: NaiveTime,
) -> DateTime<FixedOffset> {
    let offset = *start.offset();
    let date = start.date_naive();
    let candidate_naive = date.and_time(cutoff);
    let candidate: DateTime<FixedOffset> = match offset.from_local_datetime(&candidate_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => {
            // FixedOffset では理論上発生しない。安全側に start を返す。
            start
        }
    };
    if candidate > start {
        candidate
    } else {
        candidate + ChronoDuration::days(1)
    }
}
