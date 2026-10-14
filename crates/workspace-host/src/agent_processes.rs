//! What each agent started. Every process group seen among an agent's providers' descendants is
//! recorded with its members in `process_groups` and `process_group_members`, against the agent
//! and across its turns. A row without `ended_at` is a group the host believes is running. A
//! group that exits on its own, or a provider's group stopped at its turn's end, is deleted,
//! so the tables hold what runs now and what the host stopped, when and why.
//!
//! What an agent starts keeps running between its turns. The host stops it when the agent is
//! archived or stopped, at a quit and at startup, always through `processes::stop_group`.
use crate::processes::{self, Member, ProcessInfo, Sighting, Stop};
use crate::*;
use std::collections::HashMap;

mod panel;

/// Why the host stopped an agent's processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StopReason {
    Archived,
    /// For an archive that then failed: the agent is still there and its processes are not.
    ArchiveFailed,
    Stopped,
    Quit,
    Startup,
    /// The human pressed Stop in the Processes panel; the agent keeps working.
    StoppedByHuman,
}
impl StopReason {
    /// As the record keeps it in `process_groups.ended`.
    fn tag(self) -> &'static str {
        match self {
            Self::Archived => "archived",
            Self::ArchiveFailed => "archive_failed",
            Self::Stopped => "stopped",
            Self::Quit => "quit",
            Self::Startup => "startup",
            Self::StoppedByHuman => "stopped_by_human",
        }
    }
    /// Completes "stopped when …" and "still running after …".
    fn occasion(self, agent: &str) -> String {
        match self {
            Self::Archived => format!("{agent} was archived"),
            Self::ArchiveFailed => format!("{agent} was to be archived, which then failed"),
            Self::Stopped => format!("{agent} was stopped"),
            Self::Quit => "Wiffletree quit".into(),
            Self::Startup => "Wiffletree restarted".into(),
            Self::StoppedByHuman => "the human pressed Stop in the Processes panel".into(),
        }
    }
}

/// Why a group that still runs was left alone.
const UNCONFIRMED: &str =
    "Wiffletree could not confirm it is still that agent's process, so it did not stop it.";
/// Why a group the host signalled is not signalled again.
const SIGNALLED_ONCE: &str = "Wiffletree signals a group only while a process it recorded for that agent is still running in it, so it did not signal the rest again.";

/// What a stop stopped of one agent's own processes: the records it ended and, for the notice,
/// each group with its processes.
pub(crate) struct Stopped {
    session_id: String,
    records: Vec<String>,
    groups: Vec<Group>,
}

/// A process group the record says is running.
struct Recorded {
    id: String,
    session_id: String,
    run_id: String,
    group: i64,
    /// The run's own provider group, not something the agent started with a tool.
    provider: bool,
    /// A conversation notice has named it.
    noticed: bool,
    members: Vec<Member>,
}

/// A process group's id and the processes in it, as a notice names them.
type Group = (i64, Vec<ProcessInfo>);

fn count(processes: usize) -> String {
    match processes {
        1 => "1 process".into(),
        many => format!("{many} processes"),
    }
}

#[derive(Clone, Copy)]
enum Tense {
    Present,
    Past,
}

/// Names the processes' programs as a sentence, the most common first: "All 8 are the program
/// `yes`." or "2 are the program `node` and 1 is `esbuild`." A program's name is never left to
/// stand beside a bare number, which reads as nothing when the program is called `yes`.
fn programs(processes: &[&ProcessInfo], tense: Tense) -> String {
    let mut names: Vec<(&str, usize)> = vec![];
    for process in processes {
        match names.iter_mut().find(|(name, _)| *name == process.command) {
            Some((_, count)) => *count += 1,
            None => names.push((&process.command, 1)),
        }
    }
    names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let (is, are) = match tense {
        Tense::Present => ("is", "are"),
        Tense::Past => ("was", "were"),
    };
    if let [(name, count)] = names[..] {
        return match count {
            1 => format!("It {is} the program `{name}`."),
            2 => format!("Both {are} the program `{name}`."),
            _ => format!("All {count} {are} the program `{name}`."),
        };
    }
    let mut parts = names.iter().enumerate().map(|(index, (name, count))| {
        let verb = if *count == 1 { is } else { are };
        let program = if index == 0 { "the program " } else { "" };
        format!("{count} {verb} {program}`{name}`")
    });
    let last = parts.next_back().unwrap_or_default();
    let mut sentence = parts.collect::<Vec<_>>().join(", ");
    sentence.push_str(&format!(" and {last}."));
    sentence
}

/// A process start time as the human's clock shows it, such as 14:27:00.
fn clock(started: i64) -> String {
    chrono::DateTime::from_timestamp_micros(started)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

/// The notice a turn leaves when it ends with processes still running, one entry per group.
fn left_running_notice(agent: &str, groups: &[Group]) -> String {
    let processes: Vec<&ProcessInfo> = groups.iter().flat_map(|(_, members)| members).collect();
    let started = groups
        .iter()
        .map(|(group, members)| {
            let first = members.iter().map(|m| m.started).min().unwrap_or_default();
            format!("started at {} · process group {group}", clock(first))
        })
        .collect::<Vec<_>>()
        .join("; ");
    let keep = match processes.len() {
        1 => "it keeps",
        _ => "they keep",
    };
    format!(
        "{agent} left {} running. {}\nS{} · {keep} running until {agent} is archived.",
        count(processes.len()),
        programs(&processes, Tense::Present),
        // The line opens with the first entry.
        started.trim_start_matches('s')
    )
}

/// The notice the host leaves when it has stopped an agent's processes.
fn stopped_notice(agent: &str, groups: &[Group], reason: StopReason) -> String {
    let processes: Vec<&ProcessInfo> = groups.iter().flat_map(|(_, members)| members).collect();
    let ids = groups
        .iter()
        .map(|(group, _)| group.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let label = match groups.len() {
        1 => "Process group",
        _ => "Process groups",
    };
    format!(
        "Stopped {} {agent} had left running. {}\n{label} {ids} · stopped when {}.",
        count(processes.len()),
        programs(&processes, Tense::Past),
        reason.occasion(agent)
    )
}

/// Stops the groups together: each stop can take the grace period.
fn stop_together(recorded: &[Recorded]) -> Vec<Stop> {
    std::thread::scope(|scope| {
        let stops: Vec<_> = recorded
            .iter()
            .map(|r| scope.spawn(|| processes::stop_group(r.group, &r.members)))
            .collect();
        stops
            .into_iter()
            .map(|stop| stop.join().unwrap_or(Stop::Running))
            .collect()
    })
}

/// Creates the record's tables and carries over what an earlier version kept on each run: one
/// process group and its leader's start time. A run that could still have a live process, one
/// left unfinished or finished without its process having been seen to end, becomes a record
/// under the run's own id, so an inbox note that names the run still names it. A run whose
/// note the human dismissed is left out: its process is not the agent's. The old columns are
/// cleared, so this happens once.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(include_str!("processes.sql"))?;
    let tx = db.unchecked_transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO process_groups(id,session_id,run_id,process_group,provider,seen_at)
         SELECT id,session_id,id,process_group,1,started_at FROM provider_runs
         WHERE process_group IS NOT NULL AND (finished_at IS NULL
           OR ((outcome='interrupted' OR (outcome='failed' AND json_extract(detail,'$.result.error')=?1))
               AND json_extract(detail,'$.process_gone') IS NULL))
         AND NOT EXISTS (SELECT 1 FROM attention a WHERE a.operation_id=?2||provider_runs.id
                         AND json_extract(a.data,'$.answer') IS NOT NULL)",
        params![service::QUIT_DURING_TURN, LEFTOVER_PROCESS],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO process_group_members(group_id,pid,started,command)
         SELECT r.id,r.process_group,r.process_started,'' FROM provider_runs r
         JOIN process_groups g ON g.id=r.id WHERE r.process_started IS NOT NULL",
        [],
    )?;
    tx.execute(
        "UPDATE provider_runs SET process_group=NULL,process_started=NULL WHERE process_group IS NOT NULL",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

impl Host {
    /// The groups recorded as running: one session's or everyone's, with or without the
    /// providers' own.
    fn recorded_processes(&self, session: Option<&str>, tools_only: bool) -> Result<Vec<Recorded>> {
        let mut recorded = self
            .db
            .prepare(
                "SELECT id,session_id,run_id,process_group,provider,noticed_at IS NOT NULL FROM process_groups
                 WHERE ended_at IS NULL AND (?1 IS NULL OR session_id=?1) AND (provider=0 OR NOT ?2) ORDER BY rowid",
            )?
            .query_map(params![session, tools_only], |r| {
                Ok(Recorded {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    run_id: r.get(2)?,
                    group: r.get(3)?,
                    provider: r.get(4)?,
                    noticed: r.get(5)?,
                    members: vec![],
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut members = self
            .db
            .prepare("SELECT pid,started FROM process_group_members WHERE group_id=?1")?;
        for group in &mut recorded {
            group.members = members
                .query_map([&group.id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(recorded)
    }
    fn record_members(&self, record: &str, processes: &[ProcessInfo]) -> Result<()> {
        for process in processes {
            self.db.execute(
                "INSERT INTO process_group_members(group_id,pid,started,command) VALUES (?1,?2,?3,?4)
                 ON CONFLICT DO UPDATE SET command=excluded.command",
                params![record, process.pid, process.started, process.command],
            )?;
        }
        Ok(())
    }
    /// Makes the record's members the processes that are in the group now. While a run is live
    /// its members only gather, exited ones included; between turns the record is exact.
    fn replace_members(&self, record: &str, processes: &[ProcessInfo]) -> Result<()> {
        self.db.execute(
            "DELETE FROM process_group_members WHERE group_id=?1",
            [record],
        )?;
        self.record_members(record, processes)
    }
    /// Records a run's own provider process, which leads its group. A group this agent had
    /// recorded under the same id has emptied, or the id could not have been given out again.
    /// The run itself keeps the group's id in its detail, as a fact about the run that
    /// outlasts the record's row; nothing is ever signalled by it.
    pub(crate) fn record_provider_process(
        &mut self,
        session: &str,
        run: &str,
        group: u32,
        started: Option<i64>,
    ) -> Result<()> {
        self.atomically(|host| {
            host.db.execute(
                "DELETE FROM process_groups WHERE session_id=?1 AND process_group=?2 AND ended_at IS NULL",
                params![session, group],
            )?;
            let id = new_id();
            host.db.execute(
                "INSERT INTO process_groups(id,session_id,run_id,process_group,provider,seen_at) VALUES (?1,?2,?3,?4,1,?5)",
                params![id, session, run, group, now()],
            )?;
            if let Some(started) = started {
                host.db.execute(
                    "INSERT INTO process_group_members(group_id,pid,started,command) VALUES (?1,?2,?3,'')",
                    params![id, group, started],
                )?;
            }
            host.db.execute(
                "UPDATE provider_runs SET detail=json_set(detail,'$.process_group',?2) WHERE id=?1",
                params![run, group],
            )?;
            Ok(())
        })
    }
    /// Records what the watcher saw of a run's tree: each process under its group, and each
    /// group whose every process has exited no longer.
    pub(crate) fn record_sightings(
        &mut self,
        session: &str,
        run: &str,
        sightings: &[Sighting],
    ) -> Result<()> {
        self.atomically(|host| {
            for sighting in sightings {
                match sighting {
                    Sighting::Member(process) => {
                        let record = host.record_for_sighting(session, run, process)?;
                        host.record_members(&record, std::slice::from_ref(process))?;
                    }
                    Sighting::GroupExited(group) => {
                        host.db.execute(
                            "DELETE FROM process_groups WHERE session_id=?1 AND process_group=?2 AND ended_at IS NULL",
                            params![session, group],
                        )?;
                    }
                }
            }
            Ok(())
        })
    }
    /// Records everything a running run's provider has started that is alive at a look made
    /// now. The run's own thread passes sightings on only as its loop wakes, and one it has
    /// taken may still be on its way here: too late for a stop that must not return while a
    /// tool started a moment ago still runs.
    pub(crate) fn record_tree_now(&mut self, session: &str, run: &str) -> Result<()> {
        let followed = processes::followed_now(run);
        let seen: Vec<Sighting> = followed.into_iter().map(Sighting::Member).collect();
        self.record_sightings(session, run, &seen)
    }
    /// The open row a sighting of `process` belongs to. An open row with the same group id is
    /// the same group only while a member it recorded is still alive in it, by the rule that
    /// confirms a group before it is signalled. Otherwise that group emptied unseen and its id
    /// was given out again: the row is forgotten, and a fresh one is opened for the sighting's
    /// run with no notice yet, so neither the earlier run nor an earlier notice is taken over.
    fn record_for_sighting(
        &mut self,
        session: &str,
        run: &str,
        process: &ProcessInfo,
    ) -> Result<String> {
        let open = self
            .db
            .query_row(
                "SELECT id,run_id,provider FROM process_groups WHERE session_id=?1 AND process_group=?2 AND ended_at IS NULL",
                params![session, process.group],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)),
            )
            .optional()?;
        let mut provider = false;
        if let Some((record, its_run, its_provider)) = open {
            let members: Vec<Member> = self
                .db
                .prepare("SELECT pid,started FROM process_group_members WHERE group_id=?1")?
                .query_map([&record], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let group = i64::from(process.group);
            if processes::confirmed_members(group, &members).is_some() {
                return Ok(record);
            }
            self.forget_process_group(&record)?;
            // The run's own provider group, seen again after its provider exited, stays that.
            provider = its_provider && its_run == run;
        }
        let id = new_id();
        self.db.execute(
            "INSERT INTO process_groups(id,session_id,run_id,process_group,provider,seen_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, session, run, process.group, provider, now()],
        )?;
        Ok(id)
    }
    /// A run has ended and its provider thread has stopped the provider's group.
    pub(crate) fn provider_process_ended(&mut self, run: &str) -> Result<()> {
        self.db.execute(
            "DELETE FROM process_groups WHERE run_id=?1 AND provider=1 AND ended_at IS NULL",
            [run],
        )?;
        Ok(())
    }
    /// Forgets a group that no longer runs, and settles the inbox note it may have raised.
    fn forget_process_group(&mut self, record: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM process_groups WHERE id=?1", [record])?;
        self.settle_process_note(record, "The process exited")
    }
    fn settle_process_note(&mut self, record: &str, answer: &str) -> Result<()> {
        let note = self
            .db
            .query_row(
                "SELECT id FROM attention WHERE operation_id=?1 AND json_extract(data,'$.answer') IS NULL",
                [format!("{LEFTOVER_PROCESS}{record}")],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        if let Some(note) = note {
            self.resolve_attention(&note, answer)?;
        }
        Ok(())
    }
    /// Raises the inbox note for a group that still runs, once: one nobody could confirm, or
    /// one the host `signalled` whose later members outlived that. While the note of a
    /// provider's group is open, its session starts no turn.
    fn note_process_still_running(
        &mut self,
        recorded: &Recorded,
        reason: StopReason,
        signalled: bool,
    ) -> Result<()> {
        let operation = format!("{LEFTOVER_PROCESS}{}", recorded.id);
        let raised: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM attention WHERE session_id=?1 AND operation_id=?2)",
            params![recorded.session_id, operation],
            |r| r.get(0),
        )?;
        if raised {
            return Ok(());
        }
        let name = self.session(&recorded.session_id)?.name;
        let group = recorded.group;
        let occasion = reason.occasion(&name);
        let found = match (recorded.provider, signalled) {
            (true, true) => format!(
                "Wiffletree signalled provider process group {group} from its interrupted turn at startup, but part of it is still running. {SIGNALLED_ONCE}"
            ),
            (true, false) => format!(
                "provider process {group} from its interrupted turn is still running. {UNCONFIRMED}"
            ),
            (false, true) => format!(
                "Wiffletree signalled process group {group} when {occasion}, but part of it is still running. {SIGNALLED_ONCE}"
            ),
            (false, false) => {
                format!("process group {group} is still running after {occasion}. {UNCONFIRMED}")
            }
        };
        let held = match recorded.provider {
            true => format!(" The agent starts no turn while process group {group} runs."),
            false => String::new(),
        };
        let prompt = format!(
            "{name}: {found}{held} Stop the process if it is the agent's, or dismiss this note if it is not."
        );
        self.request_attention(&recorded.session_id, "local", &operation, &prompt, &[])?;
        Ok(())
    }
    /// Settles every leftover-process note whose group has exited, whether or not its session
    /// has a turn due, and forgets the group. Returns whether a note was settled, and whether
    /// one is still open beside a group that runs.
    pub(crate) fn settle_process_notes(&mut self) -> Result<(bool, bool)> {
        let noted = self
            .db
            .prepare(
                "SELECT g.id,g.process_group FROM attention a JOIN process_groups g ON g.id=substr(a.operation_id,length(?1)+1)
                 WHERE substr(a.operation_id,1,length(?1))=?1 AND json_extract(a.data,'$.answer') IS NULL AND g.ended_at IS NULL",
            )?
            .query_map([LEFTOVER_PROCESS], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, i64)>>>()?;
        let (mut settled, mut running) = (false, false);
        for (record, group) in noted {
            if processes::recorded_group_runs(group) {
                running = true;
            } else {
                self.forget_process_group(&record)?;
                settled = true;
            }
        }
        Ok((settled, running))
    }
    /// Adds a notice from Wiffletree to the agent's conversation. It rides along with the
    /// agent's next turn; an archived agent has none, so its notice is stored as complete.
    fn process_notice(&mut self, session: &Session, id: &str, body: &str) -> Result<()> {
        if session.archived {
            self.db.execute(
                "INSERT OR IGNORE INTO messages(id,project_id,sender,recipient,body,receipt,created_at,quiet) VALUES (?1,?2,NULL,?3,?4,'completed',?5,1)",
                params![id, session.project_id, session.id, body, now()],
            )?;
        } else {
            self.send_quietly(id, None, &session.id, body)?;
        }
        Ok(())
    }
    /// Stops the recorded groups under the confirmation rule and records what became of each:
    /// one that was stopped keeps its row with the time and the reason, one that had gone is
    /// forgotten, and one that still runs raises the inbox note. Returns the runs whose own
    /// provider group the host stopped, in whole or as far as it could confirm it, and what it
    /// stopped of each agent's own processes, for `notice_stopped`.
    fn stop_recorded(
        &mut self,
        recorded: Vec<Recorded>,
        reason: StopReason,
    ) -> Result<(Vec<String>, Vec<Stopped>)> {
        let stops = stop_together(&recorded);
        let mut provider_runs = vec![];
        let mut stopped: Vec<Stopped> = vec![];
        for (recorded, stop) in recorded.into_iter().zip(stops) {
            match stop {
                Stop::Gone => self.forget_process_group(&recorded.id)?,
                Stop::Running => self.note_process_still_running(&recorded, reason, false)?,
                // What the host confirmed was stopped. The rest stays recorded as running,
                // with a note that says what happened.
                Stop::Signalled => {
                    self.note_process_still_running(&recorded, reason, true)?;
                    if recorded.provider {
                        provider_runs.push(recorded.run_id);
                    }
                }
                Stop::Stopped(processes) => {
                    self.atomically(|host| {
                        host.replace_members(&recorded.id, &processes)?;
                        host.db.execute(
                            "UPDATE process_groups SET ended_at=?2,ended=?3 WHERE id=?1",
                            params![recorded.id, now(), reason.tag()],
                        )?;
                        host.settle_process_note(&recorded.id, "Wiffletree stopped the process")
                    })?;
                    // A run's provider was stopped only if its own group was: a tool the run
                    // left behind says nothing about the provider.
                    if recorded.provider {
                        provider_runs.push(recorded.run_id);
                        continue;
                    }
                    let agent = stopped
                        .iter()
                        .position(|s| s.session_id == recorded.session_id);
                    let agent = agent.unwrap_or_else(|| {
                        stopped.push(Stopped {
                            session_id: recorded.session_id.clone(),
                            records: vec![],
                            groups: vec![],
                        });
                        stopped.len() - 1
                    });
                    stopped[agent].records.push(recorded.id);
                    stopped[agent].groups.push((recorded.group, processes));
                }
            }
        }
        Ok((provider_runs, stopped))
    }
    /// Adds each agent's notice of what was stopped and why. The processes are stopped whether
    /// or not a notice can be stored, so a failure is only logged.
    fn notice_stopped(&mut self, stopped: &[Stopped], reason: StopReason) {
        for stopped in stopped {
            let noticed = self.session(&stopped.session_id).and_then(|session| {
                let body = stopped_notice(&session.name, &stopped.groups, reason);
                let id = format!("{PROCESS_NOTICE_PREFIX}stopped:{}", new_id());
                self.process_notice(&session, &id, &body)
            });
            if let Err(error) = noticed {
                eprintln!("Process notice for {}: {error:#}", stopped.session_id);
            }
        }
    }
    fn recorded_for(&self, sessions: &[String]) -> Result<Vec<Recorded>> {
        let mut recorded = vec![];
        for session in sessions {
            recorded.extend(self.recorded_processes(Some(session), true)?);
        }
        Ok(recorded)
    }
    /// Stops everything these agents started with their tools, across all their turns.
    pub(crate) fn stop_processes(&mut self, sessions: &[String], reason: StopReason) -> Result<()> {
        self.stop_processes_except(sessions, reason, &[])
    }
    /// As `stop_processes`, leaving alone what the runs in `spared` started: a stop lets a
    /// compaction that has started finish, and what its provider started is part of it.
    pub(crate) fn stop_processes_except(
        &mut self,
        sessions: &[String],
        reason: StopReason,
        spared: &[String],
    ) -> Result<()> {
        let mut recorded = self.recorded_for(sessions)?;
        recorded.retain(|recorded| !spared.contains(&recorded.run_id));
        if !recorded.is_empty() {
            let (_, stopped) = self.stop_recorded(recorded, reason)?;
            self.notice_stopped(&stopped, reason);
        }
        Ok(())
    }
    /// Stops what these agents started, for an archive that is about to be made. Whether it
    /// then succeeds decides what the notice and the record say; see `archive_settled`.
    pub(crate) fn stop_for_archive(&mut self, sessions: &[String]) -> Result<Vec<Stopped>> {
        let recorded = self.recorded_for(sessions)?;
        if recorded.is_empty() {
            return Ok(vec![]);
        }
        Ok(self.stop_recorded(recorded, StopReason::Archived)?.1)
    }
    /// Says what an archive's stop did, once the archive has been made or has failed. A stop
    /// cannot be undone: when the archive failed after it, the record says so instead of
    /// `archived`, and the agent, still there, reads the notice at its next turn.
    pub(crate) fn archive_settled(&mut self, stopped: &[Stopped], archived: bool) {
        let reason = match archived {
            true => StopReason::Archived,
            false => StopReason::ArchiveFailed,
        };
        if !archived {
            let records = stopped.iter().flat_map(|stopped| &stopped.records);
            for record in records {
                let marked = self.db.execute(
                    "UPDATE process_groups SET ended=?2 WHERE id=?1",
                    params![record, reason.tag()],
                );
                if let Err(error) = marked {
                    eprintln!("Process record {record}: {error:#}");
                }
            }
        }
        self.notice_stopped(stopped, reason);
    }
    /// The agents with a tool process recorded as running.
    pub(crate) fn agents_with_processes(&self) -> Result<Vec<String>> {
        Ok(self
            .db
            .prepare("SELECT DISTINCT session_id FROM process_groups WHERE ended_at IS NULL AND provider=0")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Stops every group the last host left recorded as running, providers' own included:
    /// nothing an agent started runs while no host supervises it. Returns when each run's
    /// provider group was stopped, for the runs whose group was.
    pub(crate) fn stop_processes_left_running(&mut self) -> Result<HashMap<String, i64>> {
        let recorded = self.recorded_processes(None, false)?;
        let (provider_runs, stopped) = self.stop_recorded(recorded, StopReason::Startup)?;
        self.notice_stopped(&stopped, StopReason::Startup);
        let at = now();
        Ok(provider_runs.into_iter().map(|run| (run, at)).collect())
    }
    /// After a turn that ended normally: forgets the agent's groups that have exited, and adds
    /// one notice naming those still running that no notice has named.
    pub(crate) fn note_processes_left_running(
        &mut self,
        session: &Session,
        run: &str,
    ) -> Result<()> {
        let mut left = vec![];
        for recorded in self.recorded_processes(Some(&session.id), true)? {
            match processes::confirmed_members(recorded.group, &recorded.members) {
                Some(processes) => {
                    // What is in the group now is what a later stop will be confirmed by.
                    self.atomically(|host| host.replace_members(&recorded.id, &processes))?;
                    if !recorded.noticed {
                        left.push((recorded, processes));
                    }
                }
                None if !processes::recorded_group_runs(recorded.group) => {
                    self.forget_process_group(&recorded.id)?;
                }
                // It runs and cannot be confirmed: the host says nothing it does not know.
                None => {}
            }
        }
        if left.is_empty() {
            return Ok(());
        }
        let groups: Vec<_> = left
            .iter()
            .map(|(recorded, processes)| (recorded.group, processes.clone()))
            .collect();
        let body = left_running_notice(&session.name, &groups);
        self.atomically(|host| {
            for (recorded, _) in &left {
                host.db.execute(
                    "UPDATE process_groups SET noticed_at=?2 WHERE id=?1",
                    params![recorded.id, now()],
                )?;
            }
            let id = format!("{PROCESS_NOTICE_PREFIX}left:{run}");
            host.process_notice(session, &id, &body)
        })
    }
    /// The provider process group of one of the session's finished runs that still runs, which
    /// holds the session's next turn: two providers must never run on one conversation. A
    /// group that has exited is forgotten and its note settled. What the agent started with
    /// its tools never holds its turn.
    pub(crate) fn leftover_provider_process(&mut self, session: &str) -> Result<Option<i64>> {
        let leftovers = self
            .db
            .prepare(
                "SELECT g.id,g.process_group FROM process_groups g JOIN provider_runs r ON r.id=g.run_id
                 WHERE g.session_id=?1 AND g.provider=1 AND g.ended_at IS NULL AND r.finished_at IS NOT NULL",
            )?
            .query_map([session], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, i64)>>>()?;
        for (record, group) in leftovers {
            if processes::recorded_group_runs(group) {
                return Ok(Some(group));
            }
            self.forget_process_group(&record)?;
        }
        Ok(None)
    }
    /// The process groups recorded as running for the agents of a ticket.
    pub(crate) fn ticket_process_groups(&self, ticket: &str) -> Result<Vec<i64>> {
        Ok(self
            .db
            .prepare(
                "SELECT g.process_group FROM process_groups g JOIN runtimes t ON t.session_id=g.session_id
                 WHERE json_extract(t.data,'$.ticket_id')=?1 AND g.ended_at IS NULL",
            )?
            .query_map([ticket], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: i32, command: &str, started: i64) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent: 1,
            group: 43477,
            started,
            command: command.into(),
        }
    }

    #[test]
    fn the_notices_name_the_count_the_commands_the_group_and_the_start() {
        let started = 1_791_901_620_000_000;
        let yes: Vec<_> = (0..8)
            .map(|n| process(100 + n, "yes", started + i64::from(n)))
            .collect();
        let at = clock(started);
        assert_eq!(
            left_running_notice("Tester", &[(43477, yes.clone())]),
            format!(
                "Tester left 8 processes running. All 8 are the program `yes`.\nStarted at {at} · process group 43477 · they keep running until Tester is archived."
            )
        );
        assert_eq!(
            stopped_notice("Tester", &[(43477, yes.clone())], StopReason::Archived),
            "Stopped 8 processes Tester had left running. All 8 were the program `yes`.\nProcess group 43477 · stopped when Tester was archived."
        );

        let server = vec![process(7, "node", started + 60_000_000)];
        let later = clock(started + 60_000_000);
        assert_eq!(
            left_running_notice("Tester", &[(7, server.clone())]),
            format!(
                "Tester left 1 process running. It is the program `node`.\nStarted at {later} · process group 7 · it keeps running until Tester is archived."
            )
        );
        assert_eq!(
            left_running_notice("Tester", &[(43477, yes.clone()), (7, server.clone())]),
            format!(
                "Tester left 9 processes running. 8 are the program `yes` and 1 is `node`.\nStarted at {at} · process group 43477; started at {later} · process group 7 · they keep running until Tester is archived."
            )
        );
        // A program's name never stands beside a bare number, however many programs there are.
        let build = [
            process(1, "node", started),
            process(2, "tsc", started),
            process(3, "node", started),
            process(4, "esbuild", started),
        ];
        let build: Vec<&ProcessInfo> = build.iter().collect();
        assert_eq!(
            programs(&build, Tense::Present),
            "2 are the program `node`, 1 is `esbuild` and 1 is `tsc`."
        );
        assert_eq!(
            programs(&build[..3], Tense::Past),
            "2 were the program `node` and 1 was `tsc`."
        );
        assert_eq!(
            programs(&[build[0], build[2]], Tense::Past),
            "Both were the program `node`."
        );
        assert_eq!(
            stopped_notice("Tester", &[(43477, yes), (7, server)], StopReason::Quit),
            "Stopped 9 processes Tester had left running. 8 were the program `yes` and 1 was `node`.\nProcess groups 43477, 7 · stopped when Wiffletree quit."
        );
    }
}
