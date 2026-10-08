//! Coordinators' durable timers. The host owns the cadence; each fire is a message the owning
//! session sends itself through the normal queue, so turn limits, live mode and held input apply.
use crate::*;

const MINUTE_MS: i64 = 60_000;
/// Shortest `every`, so a timer cannot become a wake storm.
pub const MIN_SCHEDULE_INTERVAL_MS: i64 = 5 * MINUTE_MS;
/// How far ahead `at` and `until` may reach.
const MAX_HORIZON_MS: i64 = 90 * 24 * 60 * MINUTE_MS;
/// A fire enqueued or delivered this long after its slot (the host was stopped, say) is late.
const LATE_AFTER_MS: i64 = MINUTE_MS;
/// How long a timer whose fire could not be enqueued (its owner's queue is full, say) waits
/// before trying again, so a lasting failure never spins the actor.
const FIRE_RETRY_MS: i64 = MINUTE_MS;
const MAX_LABEL_BYTES: usize = 80;
const MAX_PROMPT_BYTES: usize = 4096;
const MAX_ACTIVE_PER_SESSION: i64 = 20;
/// Turn input ids of timer fires; live.rs and the scheduler rely on this prefix.
pub(crate) const FIRE_PREFIX: &str = "timer:";
const COLUMNS: &str =
    "id,project_id,session_id,label,prompt,first_at,every_ms,until,next_fire_at,created_at";

pub(crate) fn rfc3339(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}
/// `30m`, `2h`, `1d` or `90s`, at most 90 days, so later arithmetic on it cannot overflow.
fn parse_duration(value: &str) -> Result<i64> {
    let invalid = || format!("Invalid duration {value:?}; use e.g. 30m, 2h or 1d");
    let mut chars = value.trim().chars();
    let unit = match chars.next_back() {
        Some('s') => 1000,
        Some('m') => MINUTE_MS,
        Some('h') => 60 * MINUTE_MS,
        Some('d') => 24 * 60 * MINUTE_MS,
        _ => bail!(invalid()),
    };
    let count: i64 = chars
        .as_str()
        .parse()
        .ok()
        .filter(|c| *c > 0)
        .with_context(invalid)?;
    count
        .checked_mul(unit)
        .filter(|ms| *ms <= MAX_HORIZON_MS)
        .with_context(|| format!("{value} is longer than 90 days"))
}
/// An RFC 3339 time or an offset from now such as `+90m`.
fn parse_time(value: &str, now: i64) -> Result<i64> {
    let at = match value.trim().strip_prefix('+') {
        Some(offset) => now + parse_duration(offset)?,
        None => chrono::DateTime::parse_from_rfc3339(value.trim())
            .with_context(|| {
                format!("Invalid time {value:?}; use RFC 3339 or an offset like +90m")
            })?
            .timestamp_millis(),
    };
    ensure!(at > now, "{value} is not in the future");
    ensure!(
        at <= now + MAX_HORIZON_MS,
        "{value} is more than 90 days away"
    );
    Ok(at)
}
/// The one lateness rule, as SQL over `schedule_fires f` for a fire delivered at the
/// `delivered_at` parameter: it was late when enqueued (it absorbed slots or came over a
/// minute after its slot), or it is being delivered over a minute after its slot.
fn late_at(delivered_at: &str) -> String {
    format!("(f.late=1 OR {delivered_at}-f.due_at>{LATE_AFTER_MS})")
}
fn schedule_row(row: &Row<'_>) -> rusqlite::Result<Schedule> {
    Ok(Schedule {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        label: row.get(3)?,
        prompt: row.get(4)?,
        first_at: row.get(5)?,
        every_ms: row.get(6)?,
        until: row.get(7)?,
        next_fire_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}
/// The fire's message. It names the timer so it never reads as the human's message; whether
/// it is late is only known at delivery (see `fire_status`), so the stored body never changes.
fn fire_body(schedule: &Schedule, due_at: i64) -> String {
    format!(
        "[timer] {}\nYour timer {}, scheduled for {}. This is your own scheduled prompt, not a message from the human.\n\n{}",
        schedule.label,
        schedule.id,
        rfc3339(due_at),
        schedule.prompt
    )
}
/// A short description of when the timer fires, for tool results and the resume note.
pub(crate) fn cadence(schedule: &Schedule) -> String {
    let every = schedule
        .every_ms
        .map_or("once".into(), |ms| format!("every {}", duration_label(ms)));
    match schedule.until {
        Some(until) => format!("{every} until {}", rfc3339(until)),
        None => every,
    }
}
fn summary(schedule: &Schedule) -> Value {
    json!({"id":schedule.id,"label":schedule.label,"cadence":cadence(schedule),
        "next_fire_at":schedule.next_fire_at.map(rfc3339)})
}

/// The outcome of one `fire_due_schedules` pass.
#[derive(Debug, PartialEq, Eq)]
pub struct TimerPass {
    /// When the next timer is due or may retry.
    pub next: Option<i64>,
    /// Whether any timer fired or folded a slot, so clients should refresh.
    pub changed: bool,
}

impl Host {
    fn schedule(&self, id: &str) -> Result<Schedule> {
        self.db
            .query_row(
                &format!("SELECT {COLUMNS} FROM schedules WHERE id=?1"),
                [id],
                schedule_row,
            )
            .context("Timer not found")
    }
    /// Timers that will still fire, for one session or (with `None`) the whole workspace.
    pub fn active_schedules(&self, session: Option<&str>) -> Result<Vec<Schedule>> {
        Ok(self.db.prepare(&format!("SELECT {COLUMNS} FROM schedules WHERE next_fire_at IS NOT NULL AND (?1 IS NULL OR session_id=?1) ORDER BY next_fire_at"))?
            .query_map([session], schedule_row)?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn schedule_tool(
        &mut self,
        session: &Session,
        name: &str,
        args: &Value,
        now: i64,
    ) -> Result<Value> {
        ensure!(
            !session.role.is_worker(),
            "Only coordinators can use timers; ask your coordinator to schedule a follow-up"
        );
        match name {
            "schedule" => Ok(summary(&self.create_schedule(session, args, now)?)),
            "unschedule" => {
                let id = args["id"].as_str().context("Missing id")?;
                let schedule = self.schedule(id)?;
                ensure!(
                    schedule.session_id == session.id,
                    "Timer belongs to another session"
                );
                Self::stop_schedules_in(
                    &self.db,
                    std::slice::from_ref(&session.id),
                    Some(id),
                    "unscheduled",
                )?;
                Ok(json!({"id":id,"stopped":true}))
            }
            "list_schedules" => Ok(json!(
                self.active_schedules(Some(&session.id))?
                    .iter()
                    .map(summary)
                    .collect::<Vec<_>>()
            )),
            _ => bail!("Unknown timer tool"),
        }
    }
    fn create_schedule(&mut self, session: &Session, args: &Value, now: i64) -> Result<Schedule> {
        // Archiving stops a session's timers, so an archived owner (or project) cannot add one.
        ensure!(
            !session.archived,
            "{} is archived; restore it before scheduling",
            session.name
        );
        let string = |key: &str| args[key].as_str().filter(|v| !v.trim().is_empty());
        let label = string("label").context("Missing label")?;
        let prompt = string("prompt").context("Missing prompt")?;
        text(label, MAX_LABEL_BYTES)?;
        text(prompt, MAX_PROMPT_BYTES)?;
        let every_ms = string("every").map(parse_duration).transpose()?;
        if let Some(every) = every_ms {
            ensure!(
                every >= MIN_SCHEDULE_INTERVAL_MS,
                "every must be at least {}",
                duration_label(MIN_SCHEDULE_INTERVAL_MS)
            );
        }
        let first_at = match (string("at"), every_ms) {
            (Some(at), _) => parse_time(at, now)?,
            (None, Some(every)) => now + every,
            (None, None) => bail!("Give at, every or both"),
        };
        let until = string("until").map(|u| parse_time(u, now)).transpose()?;
        ensure!(
            until.is_none_or(|u| u >= first_at),
            "until is before the first fire"
        );
        let active: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM schedules WHERE session_id=?1 AND next_fire_at IS NOT NULL",
            [&session.id],
            |r| r.get(0),
        )?;
        ensure!(
            active < MAX_ACTIVE_PER_SESSION,
            "You already have {MAX_ACTIVE_PER_SESSION} active timers; unschedule one first"
        );
        let schedule = Schedule {
            id: new_id(),
            project_id: session.project_id.clone(),
            session_id: session.id.clone(),
            label: label.trim().into(),
            prompt: prompt.into(),
            first_at,
            every_ms,
            until,
            next_fire_at: Some(first_at),
            created_at: now,
        };
        self.db.execute(
            &format!("INSERT INTO schedules({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
            params![
                schedule.id,
                schedule.project_id,
                schedule.session_id,
                schedule.label,
                schedule.prompt,
                schedule.first_at,
                schedule.every_ms,
                schedule.until,
                schedule.next_fire_at,
                schedule.created_at
            ],
        )?;
        Self::event(
            &self.db,
            &session.project_id,
            Some(&session.id),
            "schedule_created",
            &format!("{} {}: {}", schedule.id, cadence(&schedule), schedule.label),
        )?;
        Ok(schedule)
    }
    /// Stops the sessions' timers (or just `only`) and withdraws fires they have not received yet.
    pub(crate) fn stop_schedules_in(
        db: &Connection,
        sessions: &[String],
        only: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        for session in sessions {
            let stopped = db.prepare("UPDATE schedules SET next_fire_at=NULL,stopped=?3 WHERE session_id=?1 AND (?2 IS NULL OR id=?2) AND stopped IS NULL RETURNING id,project_id")?
                .query_map(params![session, only, reason], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, project) in stopped {
                db.execute("UPDATE messages SET receipt='cancelled' WHERE receipt='queued' AND id IN (SELECT message_id FROM schedule_fires WHERE schedule_id=?1)", [&id])?;
                Self::event(
                    db,
                    &project,
                    Some(session),
                    "schedule_stopped",
                    &format!("{id}: {reason}"),
                )?;
            }
        }
        Ok(())
    }
    /// Fires every timer whose slot has come and says when to look again: the earliest slot
    /// still ahead, or a failed timer's retry time, never a moment already past.
    /// Each timer gets at most one undelivered fire: slots that pass while one waits, or while
    /// the host was stopped, fold into it as a late fire. The next slot is always the first one
    /// after `now` on the original cadence, so nothing drifts and missed slots never replay.
    /// A fire that fails rolls back and waits FIRE_RETRY_MS before its next attempt.
    pub fn fire_due_schedules(&mut self, now: i64) -> Result<TimerPass> {
        self.fire_retries.retain(|_, retry| *retry > now);
        let mut changed = false;
        let due = self
            .db
            .prepare(&format!(
                "SELECT {COLUMNS} FROM schedules WHERE next_fire_at<=?1 ORDER BY next_fire_at"
            ))?
            .query_map([now], schedule_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for schedule in due {
            if self.fire_retries.contains_key(&schedule.id) {
                continue;
            }
            let fired = self.atomically(|host| host.fire(&schedule, now));
            changed |= fired.is_ok();
            if let Err(error) = fired {
                eprintln!(
                    "Timer {}: {error:#}; retrying in {}",
                    schedule.id,
                    duration_label(FIRE_RETRY_MS)
                );
                self.fire_retries
                    .insert(schedule.id.clone(), now + FIRE_RETRY_MS);
            }
        }
        let next = self
            .db
            .prepare("SELECT id,next_fire_at FROM schedules WHERE next_fire_at IS NOT NULL")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(id, at)| {
                self.fire_retries
                    .get(&id)
                    .map_or(at, |retry| at.max(*retry))
            })
            .min();
        Ok(TimerPass { next, changed })
    }
    fn fire(&mut self, schedule: &Schedule, now: i64) -> Result<()> {
        let due_at = schedule.next_fire_at.context("Timer is not active")?;
        let last_slot = schedule.until.map_or(now, |until| until.min(now));
        let (slots, next) = match schedule.every_ms {
            Some(every) => {
                let slots = (last_slot - due_at) / every + 1;
                // A slot beyond the representable range means the timer is done, never a panic.
                let next = slots
                    .checked_mul(every)
                    .and_then(|offset| due_at.checked_add(offset))
                    .filter(|n| schedule.until.is_none_or(|u| *n <= u));
                (slots, next)
            }
            None => (1, None),
        };
        let waiting = self.db.query_row("SELECT f.message_id,f.missed FROM schedule_fires f JOIN messages m ON m.id=f.message_id WHERE f.schedule_id=?1 AND m.receipt IN ('queued','held')", [&schedule.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).optional()?;
        let detail = match waiting {
            Some((message, missed)) => {
                let missed = missed + slots;
                self.db.execute(
                    "UPDATE schedule_fires SET missed=?2,late=1 WHERE message_id=?1",
                    params![message, missed],
                )?;
                format!("{} folded into waiting fire {message}", schedule.id)
            }
            None => {
                let missed = slots - 1;
                let late = missed > 0 || now - due_at > LATE_AFTER_MS;
                let message = format!("{FIRE_PREFIX}{}:{due_at}", schedule.id);
                self.send(
                    &message,
                    Some(&schedule.session_id),
                    &schedule.session_id,
                    &fire_body(schedule, due_at),
                )?;
                self.db.execute(
                    "INSERT INTO schedule_fires VALUES (?1,?2,?3,?4,?5)",
                    params![message, schedule.id, due_at, missed, late],
                )?;
                format!("{message}{}", if late { " (late)" } else { "" })
            }
        };
        self.db.execute(
            "UPDATE schedules SET next_fire_at=?2 WHERE id=?1",
            params![schedule.id, next],
        )?;
        Self::event(
            &self.db,
            &schedule.project_id,
            Some(&schedule.session_id),
            "schedule_fired",
            &detail,
        )
    }
    /// How a fire stands as it is delivered: on time, or late and how many slots folded into it.
    /// `None` for messages that are not timer fires.
    /// How the fire stands when delivered at `delivered_at`; lateness follows `late_at`.
    pub fn fire_status(&self, message_id: &str, delivered_at: i64) -> Result<Option<String>> {
        let fire = self
            .db
            .query_row(
                &format!(
                    "SELECT f.missed,{} FROM schedule_fires f WHERE f.message_id=?1",
                    late_at("?2")
                ),
                params![message_id, delivered_at],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
            )
            .optional()?;
        Ok(fire.map(|fire| match fire {
            (_, false) => "Timer status: on time".into(),
            (0, true) => "Timer status: late".into(),
            (n, true) => format!(
                "Timer status: late; {n} later slot(s) passed while it waited and are folded into this fire"
            ),
        }))
    }
    /// Fires of this session's timers that are late when delivered at `delivered_at` (by
    /// `late_at`) and that it has not seen yet, as `(timer, label, due, missed)`: those enqueued
    /// since `since` and any still waiting, such as one queued on time before a stop.
    pub(crate) fn late_fires(
        &self,
        session: &str,
        since: i64,
        delivered_at: i64,
    ) -> Result<Vec<(String, String, i64, i64)>> {
        Ok(self.db.prepare(&format!("SELECT s.id,s.label,f.due_at,f.missed FROM schedule_fires f JOIN schedules s ON s.id=f.schedule_id JOIN messages m ON m.id=f.message_id WHERE s.session_id=?1 AND {} AND (m.created_at>=?2 OR m.receipt IN ('queued','held')) ORDER BY f.due_at", late_at("?3")))?
            .query_map(params![session, since, delivered_at], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_times_parse_strictly() {
        assert_eq!(parse_duration("30m").unwrap(), 30 * MINUTE_MS);
        assert_eq!(parse_duration("2h").unwrap(), 120 * MINUTE_MS);
        assert_eq!(parse_duration("90d").unwrap(), MAX_HORIZON_MS);
        // Non-ASCII units and counts beyond 90 days are validation errors, not panics.
        for bad in [
            "",
            "m",
            "0m",
            "-5m",
            "5x",
            "1.5h",
            "5分",
            "分",
            "5mé",
            "91d",
            "9223372036854775s",
            "9223372036854775807d",
        ] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
        assert!(parse_time("+9223372036854775s", 1).is_err());
        assert!(parse_time("+5分", 1).is_err());
        let now = 1_791_400_000_000;
        assert_eq!(parse_time("+3h", now).unwrap(), now + 180 * MINUTE_MS);
        assert_eq!(
            parse_time("2026-10-07T20:00:00Z", now).unwrap(),
            1_791_403_200_000
        );
        assert!(parse_time("2026-10-07T19:00:00Z", now).is_err());
        assert!(parse_time("+91d", now).is_err());
        assert!(parse_time("tomorrow", now).is_err());
    }

    #[test]
    fn a_stored_cadence_too_large_to_advance_finishes_instead_of_overflowing() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        host.create_project("Overflow").unwrap();
        let session = host.sessions().unwrap().remove(0);
        let args = json!({"label":"check","prompt":"Check","every":"5m"});
        host.schedule_tool(&session, "schedule", &args, 1_000)
            .unwrap();
        // A row written before every was bounded: its next slot is past i64::MAX.
        host.db
            .execute(
                "UPDATE schedules SET next_fire_at=?1,every_ms=?2",
                params![i64::MAX - 10, i64::MAX / 2],
            )
            .unwrap();
        let pass = host.fire_due_schedules(i64::MAX - 5).unwrap();
        assert_eq!(
            pass,
            TimerPass {
                next: None,
                changed: true
            }
        );
        assert!(host.active_schedules(None).unwrap().is_empty());
    }
}
