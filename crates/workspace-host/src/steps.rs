//! Durable, bounded record of each run's work steps. The service holds a running turn's
//! steps in memory and writes them here only when they start or change state.
use crate::*;
use std::collections::HashMap;

/// Steps of runs that finished longer ago than this are pruned when the store opens.
const STEP_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

impl Host {
    /// The run's stored steps changed after `after`; `run_id: None` means the session's latest run.
    pub fn steps(&self, session_id: &str, run_id: Option<&str>, after: u64) -> Result<StepPage> {
        let mut page = self.run_page(session_id, run_id)?;
        if let Some(run_id) = &page.run_id {
            page.steps = self.list_data(
                "SELECT data FROM steps WHERE run_id=?1 AND revision>?2 ORDER BY json_extract(data,'$.seq')",
                params![run_id, after as i64],
            )?;
        }
        page.revision = page.steps.iter().map(|s| s.revision).fold(after, u64::max);
        Ok(page)
    }
    /// A run's timing and limits, without its steps.
    pub(crate) fn run_page(&self, session_id: &str, run_id: Option<&str>) -> Result<StepPage> {
        self.session(session_id)?;
        let run = self
            .db
            .query_row(
                &format!("SELECT id,started_at,finished_at,COALESCE(json_extract(detail,'$.omitted_steps'),0) FROM provider_runs WHERE session_id=?1 AND (id=?2 OR (?2 IS NULL AND {IS_TURN_RUN})) ORDER BY started_at DESC,rowid DESC LIMIT 1"),
                params![session_id, run_id],
                |r| {
                    Ok(StepPage {
                        run_id: Some(r.get(0)?),
                        started_at: Some(r.get(1)?),
                        finished_at: r.get(2)?,
                        running: r.get::<_, Option<i64>>(2)?.is_none(),
                        omitted_steps: r.get::<_, i64>(3)? as usize,
                        ..Default::default()
                    })
                },
            )
            .optional()?;
        ensure!(
            run.is_some() || run_id.is_none(),
            "Run not found for this session"
        );
        Ok(run.unwrap_or_default())
    }
    pub fn run_summaries(&self, session_id: &str, run_ids: &[String]) -> Result<Vec<RunSummary>> {
        ensure!(run_ids.len() <= MAX_PAGE, "Ask for at most {MAX_PAGE} runs");
        self.session(session_id)?;
        Ok(self
            .db
            .prepare(
                "SELECT r.id,r.started_at,r.finished_at,
                    COUNT(s.id) FILTER (WHERE json_extract(s.data,'$.kind') NOT IN ('narration','thinking')),
                    COUNT(s.id) FILTER (WHERE json_extract(s.data,'$.state')='failed')
                 FROM provider_runs r LEFT JOIN steps s ON s.run_id=r.id
                 WHERE r.session_id=?1 AND r.id IN (SELECT value FROM json_each(?2))
                 GROUP BY r.id",
            )?
            .query_map(params![session_id, serde_json::to_string(run_ids)?], |r| {
                Ok(RunSummary {
                    run_id: r.get(0)?,
                    started_at: r.get(1)?,
                    finished_at: r.get(2)?,
                    actions: r.get::<_, i64>(3)? as usize,
                    failed: r.get::<_, i64>(4)? as usize,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn save_step(&self, session_id: &str, step: &Step) -> Result<()> {
        self.db.execute(
            "INSERT INTO steps(session_id,run_id,id,revision,data) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(run_id,id) DO UPDATE SET revision=excluded.revision,data=excluded.data",
            params![session_id, step.run_id, step.id, step.revision as i64, encode(step)?],
        )?;
        Ok(())
    }
    /// The highest revision stored for a session; new steps continue from it.
    pub(crate) fn step_revision(&self, session_id: &str) -> Result<u64> {
        Ok(self.db.query_row(
            "SELECT COALESCE(MAX(revision),0) FROM steps WHERE session_id=?1",
            [session_id],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }
    /// The latest plan of each session's latest run, archived sessions included, as stored. A
    /// run whose latest plan step has no counts, or whose steps were pruned, has no entry.
    pub(crate) fn latest_plans(&self) -> Result<HashMap<String, PlanCounts>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT s.session_id,s.data FROM provider_runs r JOIN steps s ON s.run_id=r.id
             WHERE r.rowid=(SELECT l.rowid FROM provider_runs l WHERE l.session_id=r.session_id AND {IS_TURN_RUN} ORDER BY l.started_at DESC,l.rowid DESC LIMIT 1)
               AND json_extract(s.data,'$.kind')='plan'
             ORDER BY s.revision"
        ))?;
        let steps = statement.query_map([], |row| Ok((row.get(0)?, decode::<Step>(row, 1)?)))?;
        let mut plans = HashMap::new();
        for step in steps {
            let (session, step): (String, Step) = step?;
            match step.plan {
                Some(counts) => plans.insert(session, counts),
                None => plans.remove(&session),
            };
        }
        Ok(plans)
    }
    /// A step still running when the host stopped can no longer finish; old runs are pruned.
    /// Only runs the host never finished can hold running steps, so only those are scanned.
    pub(crate) fn recover_steps(&self) -> Result<()> {
        self.db.execute(
            "UPDATE steps SET data=json_set(data,'$.state','interrupted') WHERE run_id IN (SELECT id FROM provider_runs WHERE finished_at IS NULL) AND json_extract(data,'$.state')='running'",
            [],
        )?;
        self.db.execute(
            "DELETE FROM steps WHERE run_id IN (SELECT id FROM provider_runs WHERE finished_at<?1)",
            [now() - STEP_RETENTION_MS],
        )?;
        Ok(())
    }
}

/// Holds a step to the display limits and records how much detail was dropped.
pub(crate) fn bound(step: &mut Step) {
    let line = step
        .title
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    step.title = clip(line.trim(), MAX_STEP_TITLE);
    if let Some(note) = &mut step.note {
        *note = clip(note.trim(), MAX_STEP_NOTE);
    }
    let Some(detail) = &mut step.detail else {
        return;
    };
    if detail.len() <= MAX_STEP_DETAIL {
        return;
    }
    let keep_head = matches!(
        step.kind,
        StepKind::Narration | StepKind::Thinking | StepKind::Plan
    );
    if keep_head {
        let cut = floor_boundary(detail, MAX_STEP_DETAIL);
        step.omitted += detail.len() - cut;
        detail.truncate(cut);
    } else {
        let cut = ceil_boundary(detail, detail.len() - MAX_STEP_DETAIL);
        step.omitted += cut;
        detail.replace_range(..cut, "");
    }
}

pub(crate) fn clip(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(chars - 1).collect();
    clipped.push('…');
    clipped
}
/// The nearest character boundary at or before `index`.
pub(crate) fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
fn ceil_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(kind: StepKind, title: &str, detail: Option<String>) -> Step {
        Step {
            run_id: "run".into(),
            id: "id".into(),
            parent_id: None,
            kind,
            state: StepState::Running,
            title: title.into(),
            note: None,
            detail,
            paths: vec![],
            plan: None,
            diffs: vec![],
            omitted: 0,
            seq: 0,
            revision: 1,
            started_at: 0,
            finished_at: None,
        }
    }

    #[test]
    fn titles_keep_one_bounded_line() {
        let mut s = step(
            StepKind::Command,
            &format!("\n  {}\nsecond", "x".repeat(300)),
            None,
        );
        bound(&mut s);
        assert_eq!(s.title.chars().count(), MAX_STEP_TITLE);
        assert!(s.title.ends_with('…') && !s.title.contains('\n'));
    }

    #[test]
    fn command_output_keeps_its_tail_and_counts_what_was_dropped() {
        let output = format!("{}END", "é".repeat(3000));
        let mut s = step(StepKind::Command, "cargo test", Some(output.clone()));
        bound(&mut s);
        let detail = s.detail.unwrap();
        assert!(detail.len() <= MAX_STEP_DETAIL && detail.ends_with("END"));
        assert_eq!(s.omitted + detail.len(), output.len());
    }

    #[test]
    fn narration_keeps_its_head() {
        let text = format!("START{}", "a".repeat(10_000));
        let mut s = step(StepKind::Narration, "START", Some(text));
        bound(&mut s);
        assert!(s.detail.unwrap().starts_with("START"));
        assert_eq!(s.omitted, 10_005 - MAX_STEP_DETAIL);
    }

    #[test]
    fn restart_interrupts_running_steps_and_prunes_old_runs() {
        let home = tempfile::tempdir().unwrap();
        let (session, old_run, new_run) = {
            let mut host = Host::open_set_up(home.path()).unwrap();
            host.create_project("Steps").unwrap();
            let session = host.sessions().unwrap().remove(0).id;
            let old_finish = now() - STEP_RETENTION_MS - 1;
            for (run, finished) in [("old", Some(old_finish)), ("new", None)] {
                host.db
                    .execute(
                        "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',?3,?4,'{}')",
                        params![run, session, now(), finished],
                    )
                    .unwrap();
                let mut s = step(StepKind::Command, "cargo test", None);
                s.run_id = run.into();
                host.save_step(&session, &s).unwrap();
            }
            (session, "old", "new")
        };
        let host = Host::open_set_up(home.path()).unwrap();
        assert!(
            host.steps(&session, Some(old_run), 0)
                .unwrap()
                .steps
                .is_empty()
        );
        let page = host.steps(&session, Some(new_run), 0).unwrap();
        assert_eq!(page.steps[0].state, StepState::Interrupted);
        assert_eq!(host.step_revision(&session).unwrap(), 1);
    }

    #[test]
    fn a_compaction_is_never_a_sessions_latest_turn() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_project("Steps").unwrap();
        let session = host.sessions().unwrap().remove(0).id;
        for (run, started, detail) in [
            ("turn", 1000, "{}"),
            ("compaction", 2000, r#"{"kind":"compaction"}"#),
        ] {
            host.db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',?3,?3,?4)",
                    params![run, session, started, detail],
                )
                .unwrap();
        }
        let mut plan = step(StepKind::Plan, "Plan", None);
        (plan.run_id, plan.revision) = ("turn".into(), 1);
        plan.plan = Some(PlanCounts {
            done: 3,
            total: 6,
            current: None,
        });
        host.save_step(&session, &plan).unwrap();

        let page = host.steps(&session, None, 0).unwrap();

        assert_eq!(page.run_id.as_deref(), Some("turn"));
        assert_eq!(page.steps.len(), 1);
        assert_eq!(host.latest_plans().unwrap()[&session].done, 3);
    }

    #[test]
    fn latest_plans_read_each_sessions_latest_run_archived_or_not() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        // One project per case; each project's coordinator is the session.
        let mut sessions = vec![];
        for name in ["two runs", "no plan", "no counts", "archived", "pruned"] {
            let project = host.create_project(name).unwrap();
            let all = host.sessions().unwrap();
            sessions.push(
                all.into_iter()
                    .find(|s| s.project_id == project.id)
                    .unwrap(),
            );
        }
        let run = |session: &Session, run: &str, started: i64, finished: i64| {
            host.db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',?3,?4,'{}')",
                    params![run, session.id, started, finished],
                )
                .unwrap();
        };
        let plan = |session: &Session, run: &str, id: &str, revision: u64, done: Option<u32>| {
            let mut s = step(StepKind::Plan, "Plan", None);
            (s.run_id, s.id, s.revision) = (run.into(), id.into(), revision);
            s.plan = done.map(|done| PlanCounts {
                done,
                total: 6,
                current: None,
            });
            host.save_step(&session.id, &s).unwrap();
        };
        let [two_runs, no_plan, no_counts, archived, pruned] = &sessions[..] else {
            unreachable!()
        };
        // Only the latest run counts, and within it the plan step recorded last.
        run(two_runs, "first", 1, 2);
        plan(two_runs, "first", "plan", 1, Some(5));
        run(two_runs, "second", 3, 4);
        plan(two_runs, "second", "native", 2, Some(1));
        plan(two_runs, "second", "set_plan", 3, Some(2));
        // A latest run with no plan hides an earlier run's plan.
        run(no_plan, "planned", 1, 2);
        plan(no_plan, "planned", "plan", 1, Some(4));
        run(no_plan, "planless", 3, 4);
        // A plan step stored before counts is no plan.
        run(no_counts, "old", 1, 2);
        plan(no_counts, "old", "plan", 1, None);
        run(archived, "review", 1, 2);
        plan(archived, "review", "plan", 1, Some(4));
        let mut hidden = (*archived).clone();
        hidden.archived = true;
        host.db
            .execute(
                "UPDATE sessions SET data=?2 WHERE id=?1",
                params![hidden.id, encode(&hidden).unwrap()],
            )
            .unwrap();
        run(pruned, "ancient", 1, 2);
        plan(pruned, "ancient", "plan", 1, Some(3));
        let done = |plans: &HashMap<String, PlanCounts>, session: &Session| {
            plans.get(&session.id).map(|plan| plan.done)
        };
        let plans = host.latest_plans().unwrap();
        assert_eq!(done(&plans, two_runs), Some(2));
        assert_eq!(done(&plans, no_plan), None);
        assert_eq!(done(&plans, no_counts), None);
        assert_eq!(done(&plans, archived), Some(4));
        assert_eq!(done(&plans, pruned), Some(3));
        // Every run above finished long ago, so reopening the store prunes their steps and
        // no agent keeps a checklist.
        drop(host);
        let host = Host::open_set_up(home.path()).unwrap();
        assert!(host.latest_plans().unwrap().is_empty());
    }

    #[test]
    fn run_summaries_count_actions_and_failures_and_skip_unknown_runs() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_project("Steps").unwrap();
        let session = host.sessions().unwrap().remove(0).id;
        for run in ["busy", "quiet"] {
            host.db
                .execute(
                    "INSERT INTO provider_runs(id,session_id,messages,started_at,finished_at,detail) VALUES (?1,?2,'[]',1000,4000,'{}')",
                    params![run, session],
                )
                .unwrap();
        }
        for (id, kind, state) in [
            ("say", StepKind::Narration, StepState::Succeeded),
            ("ls", StepKind::Command, StepState::Succeeded),
            ("cat", StepKind::Command, StepState::Failed),
        ] {
            let mut s = step(kind, id, None);
            (s.run_id, s.id, s.state) = ("busy".into(), id.into(), state);
            host.save_step(&session, &s).unwrap();
        }
        let summaries = host
            .run_summaries(&session, &["busy".into(), "quiet".into(), "unknown".into()])
            .unwrap();
        let counts: Vec<_> = summaries
            .iter()
            .map(|s| (s.run_id.as_str(), s.actions, s.failed))
            .collect();
        assert_eq!(counts, [("busy", 2, 1), ("quiet", 0, 0)]);
        assert_eq!(summaries[0].finished_at, Some(4000));
    }

    #[test]
    fn small_steps_are_unchanged() {
        let mut s = step(StepKind::Read, "notes.txt", Some("alpha".into()));
        bound(&mut s);
        assert_eq!((s.detail.as_deref(), s.omitted), (Some("alpha"), 0));
    }
}
