//! How the desktop words times, counts and labels, kept together so the same thing reads the
//! same wherever it shows.
mod tickets;
pub(super) use tickets::*;

use crate::ui::now;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use workspace_core::Status;

/// How long ago, "Just now", "5m ago", "3h ago" or "2d ago".
pub(super) fn age(at: i64) -> String {
    let minutes = (now() - at).max(0) / 60_000;
    if minutes == 0 {
        "Just now".into()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if minutes < 1440 {
        format!("{}h ago", minutes / 60)
    } else {
        format!("{}d ago", minutes / 1440)
    }
}

/// Time until a future moment, "in 25m"; "now" once it has come.
pub(super) fn from_now(at: i64) -> String {
    let minutes = ((at - now()).max(0) as u64).div_ceil(60_000);
    match minutes {
        0 => "now".into(),
        1..60 => format!("in {minutes}m"),
        60..1440 => format!("in {}h {:02}m", minutes / 60, minutes % 60),
        _ => format!("in {}d", minutes / 1440),
    }
}

/// When an account limit resets, "resets in 1h 5m", or "reset due" once it has.
pub(super) fn resets_in(at: i64) -> String {
    let minutes = (at - now()) / 60_000;
    if minutes <= 0 {
        "reset due".to_owned()
    } else {
        format!("resets in {}h {}m", minutes / 60, minutes % 60)
    }
}

/// Elapsed time of a running turn, "4:18".
pub(super) fn clock(started_at: Option<i64>) -> String {
    let seconds = started_at.map_or(0, |at| (now() - at).max(0) / 1000);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// How long a finished turn took, "42s", "9m 12s" or "1h 04m".
pub(super) fn duration(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// How long a process group has run: "12 s", "41 min", "1 h 16".
pub(super) fn running_time(started_micros: i64, now_millis: i64) -> String {
    let seconds = (now_millis - started_micros / 1000).max(0) / 1000;
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h {:02}", seconds / 3600, seconds % 3600 / 60),
    }
}

/// Milliseconds since the epoch on this Mac's clock.
pub(super) fn local(at: i64) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(at).map(|at| at.with_timezone(&Local))
}

/// "14:32".
pub(super) fn time_of_day(at: DateTime<Local>) -> String {
    at.format("%H:%M").to_string()
}

/// "14 Oct 09:12".
pub(super) fn date_and_time(at: DateTime<Local>) -> String {
    at.format("%-d %b %H:%M").to_string()
}

/// "today 14:41", or "12 Oct 14:41" on another day.
pub(super) fn day_and_time(at: i64) -> String {
    let Some(at) = local(at) else {
        return String::new();
    };
    match at.date_naive() == Local::now().date_naive() {
        true => format!("today {}", time_of_day(at)),
        false => date_and_time(at),
    }
}

/// "Today 08:00", "Yesterday 17:30" or "13 Oct 10:15".
pub(super) fn when(at: DateTime<Local>, today: NaiveDate) -> String {
    match at.date_naive() {
        day if day == today => format!("Today {}", time_of_day(at)),
        day if Some(day) == today.pred_opt() => format!("Yesterday {}", time_of_day(at)),
        _ => date_and_time(at),
    }
}

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

/// `one` for a count of one, otherwise `many`.
pub(super) fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// "1 change", "3 changes".
pub(super) fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// Turns a host identifier such as `ready_for_testing` into a readable label.
pub(super) fn humanize(identifier: &str) -> String {
    let text = identifier.replace('_', " ");
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Reasoning effort as shown in menus; provider identifiers such as `xhigh` are not words.
pub(super) fn effort_label(effort: &str) -> String {
    match effort {
        "xhigh" => "Extra high".into(),
        other => humanize(other),
    }
}

/// The host calls a session whose turn was cut short "disconnected"; to the person who pressed
/// Stop or Pause, it was interrupted.
pub(super) fn status_label(status: Status) -> &'static str {
    match status {
        Status::Disconnected => "Interrupted",
        other => other.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_identifiers_read_as_labels() {
        assert_eq!(humanize("ready_for_testing"), "Ready for testing");
        assert_eq!(humanize("accepted"), "Accepted");
        assert_eq!(humanize(""), "");
        assert_eq!(effort_label("xhigh"), "Extra high");
        assert_eq!(effort_label("medium"), "Medium");
    }

    #[test]
    fn running_time_reads_in_seconds_minutes_then_hours() {
        let started = 1_000_000_000_000_000;
        let at = |seconds: i64| started / 1000 + seconds * 1000;
        assert_eq!(running_time(started, at(12)), "12 s");
        assert_eq!(running_time(started, at(41 * 60 + 5)), "41 min");
        assert_eq!(running_time(started, at(76 * 60)), "1 h 16");
        assert_eq!(running_time(started, at(125 * 60)), "2 h 05");
    }

    #[test]
    fn moments_are_dated_today_yesterday_or_by_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let at = |day, hour, minute| {
            Local
                .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
                .unwrap()
        };
        assert_eq!(when(at(16, 8, 0), today), "Today 08:00");
        assert_eq!(when(at(15, 17, 30), today), "Yesterday 17:30");
        assert_eq!(when(at(13, 10, 15), today), "13 Oct 10:15");
        let now = chrono::Utc::now().timestamp_millis();
        assert!(day_and_time(now).starts_with("today "));
        assert!(!day_and_time(now - 3 * 24 * 60 * 60_000).starts_with("today "));
    }

    #[test]
    fn counts_name_one_thing_or_many() {
        assert_eq!(counted(1, "change"), "1 change");
        assert_eq!(counted(0, "change"), "0 changes");
        assert_eq!(counted(3, "change"), "3 changes");
        assert_eq!(plural(1, "process", "processes"), "process");
        assert_eq!(plural(2, "process", "processes"), "processes");
    }
}
