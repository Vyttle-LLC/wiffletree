//! How the desktop words times, counts and names.
use chrono::{TimeZone, Utc};
use chrono_tz::Tz;

/// A moment in `timezone`, as "Oct 17, 9:30 AM EDT".
pub(super) fn local_time(at: i64, timezone: &str) -> Option<String> {
    let zone: Tz = timezone.parse().ok()?;
    Some(
        Utc.timestamp_millis_opt(at)
            .single()?
            .with_timezone(&zone)
            .format("%b %-d, %-I:%M %p %Z")
            .to_string(),
    )
}
