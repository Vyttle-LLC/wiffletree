//! Request accounting and account-wide quota observations remain separate.
use crate::*;
use chrono::{Days, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::functions::FunctionFlags;
use std::collections::BTreeMap;

fn optional_count(value: Option<&Value>) -> Option<u64> {
    match value {
        None | Some(Value::Null) => Some(0),
        Some(value) => value.as_u64(),
    }
}
pub fn normalize(provider: Provider, usage: &Value) -> Option<TokenTotals> {
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = optional_count(usage.get("output_tokens"))?;
    let read = optional_count(
        usage
            .get("cache_read_input_tokens")
            .or_else(|| usage.get("cached_input_tokens")),
    )?;
    let write = optional_count(
        usage
            .get("cache_creation_input_tokens")
            .or_else(|| usage.get("cache_write_input_tokens")),
    )?;
    let input = if provider == Provider::Claude {
        input.checked_add(read)?.checked_add(write)?
    } else {
        input
    };
    let reasoning = optional_count(usage.get("reasoning_output_tokens"))?;
    if input.checked_add(output)? > i64::MAX as u64
        || read > input
        || write > input
        || reasoning > output
    {
        return None;
    }
    Some(TokenTotals {
        input,
        output,
        cache_read: read,
        cache_write: write,
        reasoning,
        requests: 1,
        partial_requests: 0,
    })
}
fn day_start(zone: Tz, date: NaiveDate) -> Result<i64> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .context("Invalid reporting date")?;
    // Some zones advance the clock at midnight; the day starts at its first valid minute.
    for minute in 0..=1440 {
        if let Some(time) = zone
            .from_local_datetime(&(midnight + chrono::Duration::minutes(minute)))
            .earliest()
        {
            return Ok(time.timestamp_millis());
        }
    }
    anyhow::bail!("Reporting day is unavailable in this timezone")
}

impl Host {
    pub fn record_usage(
        &self,
        run: &str,
        request: &str,
        model: Option<&str>,
        usage: &Value,
        complete: bool,
        timestamp: i64,
    ) -> Result<()> {
        let provider = self.run_provider(run)?;
        let Some(mut counts) = normalize(provider, usage) else {
            return Ok(());
        };
        text(request, 256)?;
        let (session_id, detail, started): (String, String, i64) = self.db.query_row(
            "SELECT session_id,detail,started_at FROM provider_runs WHERE id=?1",
            [run],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let timestamp = if provider == Provider::Codex {
            started
        } else {
            timestamp
        };
        let session = self.session(&session_id)?;
        let profile: Value = serde_json::from_str(&detail).unwrap_or(Value::Null);
        let mut owner = session.clone();
        for _ in 0..32 {
            if matches!(
                owner.role,
                Role::TaskOrchestrator | Role::ProjectOrchestrator
            ) {
                break;
            }
            owner = self.session(
                owner
                    .parent_id
                    .as_deref()
                    .context("Usage session has no coordinator")?,
            )?;
        }
        ensure!(
            matches!(
                owner.role,
                Role::TaskOrchestrator | Role::ProjectOrchestrator
            ),
            "Invalid usage ancestry"
        );
        let tx = self.db.unchecked_transaction()?;
        let mut complete = complete;
        if provider == Provider::Codex {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM usage_requests WHERE provider=?1 AND request_id=?2)",
                params![tag(&provider)?, request],
                |r| r.get(0),
            )?;
            if exists {
                return Ok(());
            }
            match self.codex_delta(&tx, &session, run, &profile, &counts)? {
                Some((delta, continuous)) => {
                    counts = delta;
                    complete &= continuous;
                }
                None => {
                    tx.commit()?;
                    return Ok(());
                }
            }
        }
        tx.execute("INSERT INTO usage_requests(provider,request_id,run_id,session_id,project_id,coordinator_id,model,recorded_at,input,output,cache_read,cache_write,reasoning,complete,native)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
            ON CONFLICT(provider,request_id) DO UPDATE SET
                input=MAX(input,excluded.input),output=MAX(output,excluded.output),
                cache_read=MAX(cache_read,excluded.cache_read),cache_write=MAX(cache_write,excluded.cache_write),
                reasoning=MAX(reasoning,excluded.reasoning),complete=MAX(complete,excluded.complete),native=excluded.native
            WHERE usage_requests.run_id=excluded.run_id", params![tag(&provider)?, request, run, session_id,
                session.project_id, owner.id, model.or_else(||profile["profile"]["model"].as_str()).unwrap_or("unreported"),
                timestamp, counts.input as i64, counts.output as i64, counts.cache_read as i64,
                counts.cache_write as i64, counts.reasoning as i64, complete, encode(usage)?])?;
        tx.commit()?;
        Ok(())
    }
    fn codex_delta(
        &self,
        tx: &Connection,
        session: &Session,
        run: &str,
        profile: &Value,
        current: &TokenTotals,
    ) -> Result<Option<(TokenTotals, bool)>> {
        let runtime = self.session_runtime(&session.id)?;
        let thread = profile["provider_thread_id"]
            .as_str()
            .or(runtime.provider_session_id.as_deref())
            .unwrap_or(&session.id);
        let scope = format!("codex:{thread}");
        let previous: Option<(String, TokenTotals)> = tx
            .query_row(
                "SELECT run_id,counts FROM usage_counters WHERE scope=?1",
                [&scope],
                |r| Ok((r.get(0)?, decode(r, 1)?)),
            )
            .optional()?;
        if previous.as_ref().is_some_and(|(id, _)| id == run) {
            return Ok(None);
        }
        tx.execute("INSERT INTO usage_counters VALUES(?1,?2,?3) ON CONFLICT(scope) DO UPDATE SET run_id=excluded.run_id,counts=excluded.counts",params![scope,run,encode(current)?])?;
        if let Some((prior_run, prior)) = previous {
            let delta = (|| {
                Some(TokenTotals {
                    input: current.input.checked_sub(prior.input)?,
                    output: current.output.checked_sub(prior.output)?,
                    cache_read: current.cache_read.checked_sub(prior.cache_read)?,
                    cache_write: current.cache_write.checked_sub(prior.cache_write)?,
                    reasoning: current.reasoning.checked_sub(prior.reasoning)?,
                    requests: 1,
                    partial_requests: 0,
                })
            })();
            let Some(delta) = delta.filter(|c| {
                c.cache_read <= c.input && c.cache_write <= c.input && c.reasoning <= c.output
            }) else {
                return Ok(None);
            };
            let preceding:Option<String>=tx.query_row("SELECT id FROM provider_runs WHERE session_id=?1 AND rowid<(SELECT rowid FROM provider_runs WHERE id=?2) ORDER BY rowid DESC LIMIT 1",params![session.id,run],|r|r.get(0)).optional()?;
            Ok(Some((delta, preceding.as_deref() == Some(&prior_run))))
        } else {
            let first: String = tx.query_row(
                "SELECT id FROM provider_runs WHERE session_id=?1 ORDER BY rowid LIMIT 1",
                [&session.id],
                |r| r.get(0),
            )?;
            let fresh = profile
                .get("provider_session_id")
                .map(Value::is_null)
                .unwrap_or(first == run);
            // An unknown resumed baseline cannot be assigned to this turn's calendar day.
            Ok(fresh.then(|| (current.clone(), true)))
        }
    }
    fn run_provider(&self, run: &str) -> Result<Provider> {
        let (session, detail): (String, Option<String>) = self.db.query_row(
            "SELECT session_id,detail FROM provider_runs WHERE id=?1",
            [run],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if let Some(provider) = detail
            .and_then(|detail| serde_json::from_str::<Value>(&detail).ok())
            .and_then(|value| serde_json::from_value(value["profile"]["provider"].clone()).ok())
        {
            return Ok(provider);
        }
        Ok(self.session(&session)?.provider)
    }
    pub(crate) fn backfill_usage(&self) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO usage_meta VALUES('tracking_since',?1)",
            [now()],
        )?;
        if self.db.query_row(
            "SELECT COUNT(*) FROM usage_meta WHERE key='legacy_backfill'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0
        {
            return Ok(());
        }
        let mut cursor = 0;
        loop {
            let rows = self.db.prepare("SELECT rowid,id,started_at,detail FROM provider_runs WHERE rowid>?1 ORDER BY rowid LIMIT 250")?
                .query_map([cursor],|row| Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,Option<String>>(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if rows.is_empty() {
                break;
            }
            for (rowid, id, started, detail) in rows {
                cursor = rowid;
                if let Some(data) =
                    detail.and_then(|detail| serde_json::from_str::<Value>(&detail).ok())
                    && self.run_provider(&id)? == Provider::Codex
                {
                    self.record_usage(
                        &id,
                        &format!("turn:{id}"),
                        None,
                        &data["result"]["usage"],
                        true,
                        started,
                    )?;
                }
                // Legacy Claude result totals have ambiguous resume scope; never sum them as new turns.
            }
        }
        self.db.execute(
            "INSERT OR IGNORE INTO usage_meta VALUES('legacy_backfill',1)",
            [],
        )?;
        Ok(())
    }
    pub fn quotas(&self) -> Result<Vec<QuotaReading>> {
        [Provider::Claude, Provider::Codex]
            .into_iter()
            .map(|provider| {
                self.db
                    .query_row(
                        "SELECT data FROM quota_latest WHERE provider=?1",
                        [tag(&provider)?],
                        |row| decode(row, 0),
                    )
                    .optional()?
                    .map(Ok)
                    .unwrap_or_else(|| {
                        Ok(QuotaReading {
                            provider,
                            account: None,
                            observed_at: 0,
                            source: if provider == Provider::Claude {
                                "Claude stream"
                            } else {
                                "Codex app-server"
                            }
                            .into(),
                            windows: vec![],
                            error: Some(
                                if provider == Provider::Claude {
                                    "Quota is reported during live Claude turns when available."
                                } else {
                                    "Quota has not been refreshed."
                                }
                                .into(),
                            ),
                        })
                    })
            })
            .collect()
    }
    pub fn record_quota(&self, mut reading: QuotaReading) -> Result<()> {
        ensure!(
            reading.observed_at >= 0 && reading.observed_at <= now() + 60_000,
            "Invalid quota observation time"
        );
        text(&reading.source, 128)?;
        let previous = self
            .quotas()?
            .into_iter()
            .find(|q| q.provider == reading.provider)
            .unwrap();
        if reading.error.is_some()
            && reading.windows.is_empty()
            && !previous.windows.is_empty()
            && (reading.account.is_none() || reading.account == previous.account)
        {
            let error = reading.error.take();
            reading = previous;
            reading.error = error;
        } else if reading.account == previous.account {
            for old in previous.windows {
                if let Some(new) = reading.windows.iter_mut().find(|w| w.key == old.key) {
                    if new.used_percent.is_none()
                        && old.used_percent.is_some()
                        && new.resets_at == old.resets_at
                    {
                        new.used_percent = old.used_percent;
                        new.observed_at = old.observed_at;
                    }
                } else if reading.provider == Provider::Claude {
                    reading.windows.push(old);
                }
            }
        }
        let tx = self.db.unchecked_transaction()?;
        for window in &reading.windows {
            ensure!(
                window.observed_at >= 0 && window.observed_at <= reading.observed_at,
                "Invalid quota window time"
            );
            text(&window.key, 128)?;
            text(&window.label, 128)?;
            ensure!(
                window.used_percent.is_none_or(|n| n.is_finite() && n >= 0.),
                "Invalid quota percentage"
            );
            if let Some(used) = window.used_percent {
                tx.execute("INSERT INTO quota_history VALUES(?1,?2,?3,?4,?5,?6,?7)
                    ON CONFLICT(provider,account,window_key,observed_at) DO UPDATE SET used=excluded.used,resets_at=excluded.resets_at,label=excluded.label",
                    params![tag(&reading.provider)?,reading.account.as_deref().unwrap_or("local-cli"),window.key,
                        window.observed_at / 60_000 * 60_000,window.resets_at,used,window.label])?;
            }
        }
        tx.execute("INSERT INTO quota_latest VALUES(?1,?2) ON CONFLICT(provider) DO UPDATE SET data=excluded.data",
            params![tag(&reading.provider)?,encode(&reading)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn usage_report(
        &self,
        days: u16,
        project: Option<&str>,
        provider: Option<Provider>,
        timezone: &str,
    ) -> Result<UsageReport> {
        self.usage_report_at(days, project, provider, timezone, now())
    }
    fn usage_report_at(
        &self,
        days: u16,
        project: Option<&str>,
        provider: Option<Provider>,
        timezone: &str,
        timestamp: i64,
    ) -> Result<UsageReport> {
        ensure!(
            (1..=30).contains(&days),
            "Usage period must contain 1–30 days"
        );
        let zone: Tz = timezone.parse().context("Unknown reporting timezone")?;
        if let Some(project) = project {
            self.project(project)?;
        }
        self.db.create_scalar_function(
            "usage_day",
            1,
            FunctionFlags::SQLITE_DETERMINISTIC,
            move |ctx| {
                let timestamp = ctx.get::<i64>(0)?;
                Ok(Utc
                    .timestamp_millis_opt(timestamp)
                    .single()
                    .map(|date| date.with_timezone(&zone).format("%Y-%m-%d").to_string()))
            },
        )?;
        let today = Utc
            .timestamp_millis_opt(timestamp)
            .single()
            .context("Invalid report time")?
            .with_timezone(&zone)
            .date_naive();
        let first = today
            .checked_sub_days(Days::new(u64::from(days - 1)))
            .context("Invalid date range")?;
        let start = day_start(zone, first)?;
        let provider_tag = provider.map(|p| tag(&p)).transpose()?;
        let tracking: i64 = self.db.query_row(
            "SELECT value FROM usage_meta WHERE key='tracking_since'",
            [],
            |r| r.get(0),
        )?;
        let columns = "COALESCE(SUM(input),0),COALESCE(SUM(output),0),COALESCE(SUM(cache_read),0),COALESCE(SUM(cache_write),0),COALESCE(SUM(reasoning),0),COUNT(*),COALESCE(SUM(1-complete),0)";
        let filter = "recorded_at>=?1 AND recorded_at<=?2 AND (?3 IS NULL OR project_id=?3) AND (?4 IS NULL OR provider=?4)";
        let totals = self.db.query_row(
            &format!("SELECT {columns} FROM usage_requests WHERE {filter}"),
            params![start, timestamp, project, provider_tag],
            |r| row_totals(r, 0),
        )?;
        let rows=self.db.prepare(&format!("SELECT usage_day(recorded_at),provider,{columns} FROM usage_requests WHERE {filter} GROUP BY usage_day(recorded_at),provider"))?
            .query_map(params![start,timestamp,project,provider_tag],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,row_totals(r,2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut measured: BTreeMap<(String, String), TokenTotals> = rows
            .into_iter()
            .map(|(date, provider, counts)| ((date, provider), counts))
            .collect();
        let mut daily = Vec::new();
        let mut dates = Vec::new();
        for offset in 0..days {
            let date = first
                .checked_add_days(Days::new(u64::from(offset)))
                .unwrap();
            let label = date.format("%Y-%m-%d").to_string();
            dates.push(label.clone());
            let day_end = day_start(zone, date.succ_opt().context("Invalid reporting date")?)?;
            for p in [Provider::Claude, Provider::Codex]
                .into_iter()
                .filter(|p| provider.is_none_or(|selected| *p == selected))
            {
                let counts = measured
                    .remove(&(label.clone(), tag(&p)?))
                    .unwrap_or_default();
                daily.push(DailyTokens {
                    date: label.clone(),
                    provider: p,
                    covered: day_end > tracking || counts.requests > 0,
                    counts,
                });
            }
        }
        let work=self.db.prepare(&format!("SELECT coordinator_id,session_id=coordinator_id,{columns} FROM usage_requests WHERE {filter} GROUP BY coordinator_id,session_id=coordinator_id"))?
            .query_map(params![start,timestamp,project,provider_tag],|r|Ok((r.get::<_,String>(0)?,r.get::<_,bool>(1)?,row_totals(r,2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut coordinators: BTreeMap<String, CoordinatorUsage> = BTreeMap::new();
        for (id, own, counts) in work {
            if !coordinators.contains_key(&id) {
                let session = self.session(&id)?;
                coordinators.insert(
                    id.clone(),
                    CoordinatorUsage {
                        id: id.clone(),
                        name: session.name,
                        project: self.project(&session.project_id)?.name,
                        own: TokenTotals::default(),
                        workers: TokenTotals::default(),
                    },
                );
            }
            let entry = coordinators.get_mut(&id).unwrap();
            if own {
                entry.own.add(&counts);
            } else {
                entry.workers.add(&counts);
            }
        }
        let mut coordinators: Vec<_> = coordinators.into_values().collect();
        coordinators.sort_by_key(|c| std::cmp::Reverse(c.own.total() + c.workers.total()));
        let unreported_runs=self.db.query_row("SELECT COUNT(*) FROM provider_runs r JOIN sessions s ON s.id=r.session_id WHERE r.started_at>=?1 AND r.started_at<=?2
            AND (?3 IS NULL OR s.project_id=?3) AND (?4 IS NULL OR COALESCE(CASE WHEN json_valid(r.detail) THEN json_extract(r.detail,'$.profile.provider') END,json_extract(s.data,'$.provider'))=?4)
            AND NOT EXISTS(SELECT 1 FROM usage_requests u WHERE u.run_id=r.id)",params![start,timestamp,project,provider_tag],|r|r.get(0))?;
        let mut trends = Vec::new();
        for reading in self
            .quotas()?
            .into_iter()
            .filter(|r| provider.is_none_or(|p| r.provider == p))
        {
            for window in reading.windows {
                let account = reading.account.as_deref().unwrap_or("local-cli");
                let values=self.db.prepare("SELECT usage_day(observed_at),SUM(increase) FROM (
                    SELECT observed_at,CASE WHEN resets_at=previous_reset AND used>=previous_used AND usage_day(observed_at)=usage_day(previous_at)
                        THEN used-previous_used END increase FROM (
                        SELECT observed_at,resets_at,used,LAG(resets_at) OVER w previous_reset,LAG(used) OVER w previous_used,LAG(observed_at) OVER w previous_at
                        FROM quota_history WHERE provider=?1 AND account=?2 AND window_key=?3 AND observed_at>=?4 AND observed_at<=?5
                        WINDOW w AS (ORDER BY observed_at))) GROUP BY usage_day(observed_at)")?
                    .query_map(params![tag(&reading.provider)?,account,window.key,start,timestamp],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<f64>>(1)?)))?
                    .collect::<rusqlite::Result<BTreeMap<_,_>>>()?;
                let last=self.db.prepare("SELECT observed_at,resets_at,used FROM quota_history WHERE provider=?1 AND account=?2 AND window_key=?3 AND observed_at<=?4 ORDER BY observed_at DESC LIMIT 2")?
                    .query_map(params![tag(&reading.provider)?,account,window.key,timestamp],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,f64>(2)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let rate = if last.len() == 2
                    && last[0].1 == last[1].1
                    && last[0].1.is_some()
                    && last[0].2 >= last[1].2
                    && last[0].0 - last[1].0 <= 15 * 60_000
                    && timestamp - last[0].0 <= 15 * 60_000
                    && last[0].1.is_some_and(|reset| reset > timestamp)
                {
                    Some((last[0].2 - last[1].2) * 3_600_000. / (last[0].0 - last[1].0) as f64)
                } else {
                    None
                };
                trends.push(QuotaTrend {
                    provider: reading.provider,
                    account: reading.account.clone(),
                    key: window.key,
                    label: window.label,
                    points_per_hour: rate,
                    daily: dates
                        .iter()
                        .map(|date| DailyQuota {
                            date: date.clone(),
                            increase: values.get(date).copied().flatten(),
                        })
                        .collect(),
                });
            }
        }
        Ok(UsageReport {
            timezone: timezone.into(),
            start_date: first.to_string(),
            end_date: today.to_string(),
            tracking_since: tracking,
            totals,
            unreported_runs,
            daily,
            coordinators,
            quota_trends: trends,
        })
    }
}
fn row_totals(row: &Row<'_>, offset: usize) -> rusqlite::Result<TokenTotals> {
    Ok(TokenTotals {
        input: row.get(offset)?,
        output: row.get(offset + 1)?,
        cache_read: row.get(offset + 2)?,
        cache_write: row.get(offset + 3)?,
        reasoning: row.get(offset + 4)?,
        requests: row.get(offset + 5)?,
        partial_requests: row.get(offset + 6)?,
    })
}

pub fn local_time(at: i64, timezone: &str) -> Option<String> {
    let zone: Tz = timezone.parse().ok()?;
    Some(
        Utc.timestamp_millis_opt(at)
            .single()?
            .with_timezone(&zone)
            .format("%b %-d, %-I:%M %p %Z")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(date: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(date)
            .unwrap()
            .timestamp_millis()
    }
    fn run(host: &Host, session: &Session, id: &str, timestamp: i64, usage: Value) {
        host.db.execute("INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,outcome,detail) VALUES(?1,?2,'[]',?3,?3,'completed',?4)",
            params![id,session.id,timestamp,json!({"profile":{"provider":session.provider,"model":"fixture"},"result":{"usage":usage}}).to_string()]).unwrap();
    }
    fn root(host: &mut Host, name: &str) -> Session {
        let project = host.create_project(name).unwrap();
        host.sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.project_id == project.id)
            .unwrap()
    }
    fn quota(at: i64, used: Option<f64>, reset: i64) -> QuotaReading {
        QuotaReading {
            provider: Provider::Codex,
            account: Some("fixture".into()),
            observed_at: at,
            source: "fixture".into(),
            error: None,
            windows: vec![QuotaWindow {
                key: "weekly".into(),
                label: "Weekly".into(),
                used_percent: used,
                observed_at: at,
                duration_mins: Some(10080),
                resets_at: Some(reset),
                status: None,
            }],
        }
    }
    #[test]
    fn counts_request_updates_once_and_keeps_interrupted_input_on_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let session = root(&mut host, "Project");
        let time = now() - 1000;
        run(&host, &session, "run", time, Value::Null);
        let input = json!({"input_tokens":100,"cache_read_input_tokens":50,"cache_creation_input_tokens":20,"output_tokens":0});
        host.record_usage("run", "request", None, &input, false, time)
            .unwrap();
        let final_usage = json!({"input_tokens":100,"cache_read_input_tokens":50,"cache_creation_input_tokens":20,"output_tokens":30});
        host.record_usage("run", "request", None, &final_usage, true, time + 10)
            .unwrap();
        host.record_usage("run", "request", None, &input, false, time + 20)
            .unwrap();
        run(&host, &session, "resume", time + 20, Value::Null);
        host.record_usage("resume", "request", None, &final_usage, true, time + 20)
            .unwrap();
        host.record_usage("resume", "interrupted", None, &input, false, time + 20)
            .unwrap();
        // A CLI that failed before initializing can be reconfigured; history keeps the run's provider.
        let mut changed = session.clone();
        changed.provider = Provider::Codex;
        host.db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![session.id, encode(&changed).unwrap()],
            )
            .unwrap();
        run(&host, &session, "unreported", time, Value::Null);
        assert_eq!(
            host.usage_report(7, None, Some(Provider::Claude), "America/New_York")
                .unwrap()
                .unreported_runs,
            1
        );
        drop(host);
        let host = Host::open(directory.path()).unwrap();
        let report = host
            .usage_report(7, None, None, "America/New_York")
            .unwrap();
        assert_eq!(report.totals.input, 340);
        assert_eq!(report.totals.output, 30);
        assert_eq!(report.totals.requests, 2);
        assert_eq!(report.totals.partial_requests, 1);
        assert_eq!(report.coordinators[0].own.total(), 370);
        assert_eq!(report.unreported_runs, 1);
    }
    #[test]
    fn codex_resume_subtracts_persisted_counters_and_rolls_back_failed_writes() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let mut session = root(&mut host, "Codex");
        session.provider = Provider::Codex;
        host.db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![session.id, encode(&session).unwrap()],
            )
            .unwrap();
        let time = now() - 1000;
        for (id, input, output, cache) in [("first", 100, 10, 20), ("second", 150, 25, 30)] {
            run(&host, &session, id, time, Value::Null);
            host.record_usage(
                id,
                id,
                None,
                &json!({"input_tokens":input,"output_tokens":output,"cached_input_tokens":cache}),
                true,
                time,
            )
            .unwrap();
        }
        host.record_usage(
            "second",
            "second",
            None,
            &json!({"input_tokens":150,"output_tokens":25,"cached_input_tokens":30}),
            true,
            time,
        )
        .unwrap();
        drop(host);
        let host = Host::open(directory.path()).unwrap();
        run(&host, &session, "third", time, Value::Null);
        host.db.execute_batch("CREATE TRIGGER reject_usage BEFORE INSERT ON usage_requests BEGIN SELECT RAISE(ABORT,'disk failure'); END;").unwrap();
        let usage = json!({"input_tokens":200,"output_tokens":40,"cached_input_tokens":40});
        assert!(
            host.record_usage("third", "third", None, &usage, true, time)
                .is_err()
        );
        host.db.execute_batch("DROP TRIGGER reject_usage;").unwrap();
        host.record_usage("third", "third", None, &usage, true, time)
            .unwrap();
        let report = host
            .usage_report(7, None, None, "America/New_York")
            .unwrap();
        assert_eq!(report.totals.input, 200);
        assert_eq!(report.totals.output, 40);
        assert_eq!(report.totals.cache_read, 40);
        assert_eq!(report.totals.requests, 3);
    }
    #[test]
    fn unknown_resumed_baseline_and_counter_reset_never_become_fresh_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let mut session = root(&mut host, "Codex");
        session.provider = Provider::Codex;
        host.db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![session.id, encode(&session).unwrap()],
            )
            .unwrap();
        let time = now() - 1000;
        for (id, input) in [
            ("unknown-baseline", 1000),
            ("known-delta", 1030),
            ("reset", 5),
            ("after-reset", 20),
        ] {
            run(&host, &session, id, time, Value::Null);
            host.db.execute("UPDATE provider_runs SET detail=json_set(detail,'$.provider_session_id','existing-thread','$.provider_thread_id','existing-thread') WHERE id=?1",[id]).unwrap();
            host.record_usage(
                id,
                id,
                None,
                &json!({"input_tokens":input,"output_tokens":0}),
                true,
                time,
            )
            .unwrap();
        }
        let report = host
            .usage_report(7, None, None, "America/New_York")
            .unwrap();
        assert_eq!(report.totals.input, 45);
        assert_eq!(report.totals.requests, 2);
        assert_eq!(report.unreported_runs, 2);
    }
    #[test]
    fn coordinator_own_and_workers_reconcile_without_rolling_up_twice() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let main = root(&mut host, "Project");
        let other = root(&mut host, "Other");
        let time = now() - 1000;
        // Fixture hierarchy without Git worktrees: the reporting path depends only on stored ancestry.
        let coordinator = Session {
            id: new_id(),
            parent_id: Some(main.id.clone()),
            name: "Repository coordinator".into(),
            role: Role::TaskOrchestrator,
            ..main.clone()
        };
        let worker = Session {
            id: new_id(),
            parent_id: Some(coordinator.id.clone()),
            provider: Provider::Codex,
            role: Role::Implementer,
            name: "Worker".into(),
            ..main.clone()
        };
        for session in [&coordinator, &worker] {
            host.db
                .execute(
                    "INSERT INTO sessions VALUES(?1,?2,?3,?4,?5)",
                    params![
                        session.id,
                        session.project_id,
                        session.parent_id,
                        tag(&session.role).unwrap(),
                        encode(session).unwrap()
                    ],
                )
                .unwrap();
        }
        for (session, id, input) in [
            (&main, "main", 10),
            (&coordinator, "coordinator", 20),
            (&worker, "worker", 30),
            (&other, "other", 40),
        ] {
            run(&host, session, id, time, Value::Null);
            host.record_usage(
                id,
                id,
                None,
                &json!({"input_tokens":input,"output_tokens":5}),
                true,
                time,
            )
            .unwrap();
        }
        let all = host
            .usage_report(7, None, None, "America/New_York")
            .unwrap();
        assert_eq!(all.totals.total(), 120);
        assert_eq!(all.coordinators.len(), 3);
        assert_eq!(
            all.coordinators
                .iter()
                .map(|c| c.own.total() + c.workers.total())
                .sum::<u64>(),
            all.totals.total()
        );
        let project = host
            .usage_report(7, Some(&main.project_id), None, "America/New_York")
            .unwrap();
        assert_eq!(project.totals.total(), 75);
        let codex = host
            .usage_report(7, None, Some(Provider::Codex), "America/New_York")
            .unwrap();
        assert_eq!(codex.totals.total(), 35);
    }
    #[test]
    fn migration_backfills_codex_once_but_does_not_sum_legacy_claude_session_totals() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let claude = root(&mut host, "Claude");
        let mut codex = root(&mut host, "Codex");
        codex.provider = Provider::Codex;
        host.db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![codex.id, encode(&codex).unwrap()],
            )
            .unwrap();
        for (session, id) in [(&claude, "claude"), (&codex, "codex")] {
            run(
                &host,
                session,
                id,
                now() - 1000,
                json!({"input_tokens":100,"output_tokens":10,"cached_input_tokens":20}),
            );
        }
        host.db
            .execute("DELETE FROM usage_meta WHERE key='legacy_backfill'", [])
            .unwrap();
        drop(host);
        for _ in 0..2 {
            let host = Host::open(directory.path()).unwrap();
            let report = host
                .usage_report(7, None, None, "America/New_York")
                .unwrap();
            assert_eq!(report.totals.input, 100);
            assert_eq!(report.totals.output, 10);
            assert_eq!(report.totals.cache_read, 20);
            assert_eq!(report.totals.requests, 1);
            assert_eq!(report.unreported_runs, 1);
        }
    }
    #[test]
    fn local_days_handle_dst_and_prior_days_without_coverage() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let session = root(&mut host, "Project");
        let now = at("2026-11-01T20:00:00Z");
        host.db
            .execute(
                "UPDATE usage_meta SET value=?1 WHERE key='tracking_since'",
                [at("2026-11-01T04:00:00Z")],
            )
            .unwrap();
        for (id, time) in [
            ("before", at("2026-11-01T03:59:59Z")),
            ("midnight", at("2026-11-01T04:00:00Z")),
            ("first-1am", at("2026-11-01T05:30:00Z")),
            ("second-1am", at("2026-11-01T06:30:00Z")),
        ] {
            run(&host, &session, id, time, Value::Null);
            host.record_usage(
                id,
                id,
                None,
                &json!({"input_tokens":10,"output_tokens":1}),
                true,
                time,
            )
            .unwrap();
        }
        let today = host
            .usage_report_at(1, None, None, "America/New_York", now)
            .unwrap();
        assert_eq!(today.totals.requests, 3);
        let week = host
            .usage_report_at(7, None, None, "America/New_York", now)
            .unwrap();
        assert_eq!(week.totals.requests, 4);
        assert!(!week.daily[0].covered);
        assert_eq!(
            week.daily
                .iter()
                .find(|d| d.date == "2026-10-31" && d.provider == Provider::Claude)
                .unwrap()
                .counts
                .requests,
            1
        );
        assert!(host.usage_report(0, None, None, "UTC").is_err());
        assert!(host.usage_report(7, None, None, "invalid").is_err());
    }
    #[test]
    fn calendar_days_with_a_midnight_clock_change_do_not_panic() {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open(directory.path()).unwrap();
        let session = root(&mut host, "Project");
        for (id, time) in [
            ("before", at("2026-09-06T03:59:59Z")),
            ("after", at("2026-09-06T04:00:00Z")),
        ] {
            run(&host, &session, id, time, Value::Null);
            host.record_usage(
                id,
                id,
                None,
                &json!({"input_tokens":10,"output_tokens":1}),
                true,
                time,
            )
            .unwrap();
        }
        let report = host
            .usage_report_at(
                1,
                None,
                None,
                "America/Santiago",
                at("2026-09-06T20:00:00Z"),
            )
            .unwrap();
        assert_eq!(report.totals.requests, 1);
    }
    #[test]
    fn quota_trends_skip_resets_and_failures_keep_the_age_of_last_good_reading() {
        let directory = tempfile::tempdir().unwrap();
        let host = Host::open(directory.path()).unwrap();
        let end = at("2026-10-01T16:00:00Z");
        let reset = end + 3_600_000;
        for (time, used, reset) in [
            (end - 15 * 60_000, 20., reset),
            (end - 10 * 60_000, 25., reset),
            (end - 5 * 60_000, 2., reset + 3_600_000),
            (end, 3., reset + 3_600_000),
        ] {
            host.record_quota(quota(time, Some(used), reset)).unwrap();
        }
        host.record_quota(quota(end + 60_000, None, reset + 3_600_000))
            .unwrap();
        let mut failed = quota(end, None, reset);
        failed.windows.clear();
        failed.error = Some("Offline".into());
        host.record_quota(failed).unwrap();
        let saved = host
            .quotas()
            .unwrap()
            .into_iter()
            .find(|r| r.provider == Provider::Codex)
            .unwrap();
        assert_eq!(saved.windows[0].used_percent, Some(3.));
        assert_eq!(saved.windows[0].observed_at, end);
        assert_eq!(saved.error.as_deref(), Some("Offline"));
        let report = host
            .usage_report_at(7, None, None, "UTC", end + 1000)
            .unwrap();
        assert!((report.quota_trends[0].points_per_hour.unwrap() - 12.).abs() < 0.00001);
        // At midnight, only same-day intervals count; reset never adds a negative or invented delta.
        let increase: f64 = report.quota_trends[0]
            .daily
            .iter()
            .filter_map(|d| d.increase)
            .sum();
        assert_eq!(increase, 6.);
        let stale = host
            .usage_report_at(7, None, None, "UTC", end + 16 * 60_000)
            .unwrap();
        assert_eq!(stale.quota_trends[0].points_per_hour, None);
        let mut invalid = quota(end, Some(f64::NAN), reset);
        assert!(host.record_quota(invalid.clone()).is_err());
        invalid.windows[0].used_percent = Some(-1.);
        assert!(host.record_quota(invalid).is_err());
        let mut other = quota(end, None, reset);
        other.account = Some("other-account".into());
        other.windows.clear();
        other.error = Some("No limits for this account".into());
        host.record_quota(other).unwrap();
        let saved = host
            .quotas()
            .unwrap()
            .into_iter()
            .find(|r| r.provider == Provider::Codex)
            .unwrap();
        assert_eq!(saved.account.as_deref(), Some("other-account"));
        assert!(saved.windows.is_empty());
    }
    #[test]
    fn invalid_counts_and_missing_usage_are_unknown_not_zero_requests() {
        assert!(normalize(Provider::Codex, &json!({})).is_none());
        assert!(normalize(Provider::Claude, &json!({"input_tokens":-1})).is_none());
        assert!(
            normalize(
                Provider::Codex,
                &json!({"input_tokens":10,"output_tokens":-1})
            )
            .is_none()
        );
        assert!(
            normalize(
                Provider::Codex,
                &json!({"input_tokens":10,"cached_input_tokens":20})
            )
            .is_none()
        );
        assert!(
            normalize(
                Provider::Claude,
                &json!({"input_tokens":u64::MAX,"cache_read_input_tokens":10})
            )
            .is_none()
        );
    }
}
