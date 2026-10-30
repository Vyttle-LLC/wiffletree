//! Whether a coordinator's reply to a turn the human did not start tells the human anything new.
//! The host compares what it records about the project with what it recorded at the
//! coordinator's last reply shown in full, and stores the result on the reply for the desktop
//! to fold or to head with what changed. The reply's text never changes.
use crate::*;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Adds the replies' result column and the coordinators' baselines. Runs before the
/// message-revision trigger, which watches the column.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let present: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='reply_update'",
        [],
        |r| r.get(0),
    )?;
    if present == 0 {
        db.execute_batch("ALTER TABLE messages ADD COLUMN reply_update TEXT")?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS reply_baselines (session_id TEXT PRIMARY KEY REFERENCES sessions(id), facts TEXT NOT NULL);",
    )?;
    Ok(())
}

/// What the host records about a coordinator's project, for comparing one reply with another.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProjectFacts {
    tickets: Vec<TicketFacts>,
    /// Open questions for the human, by id, with their prompt's first line.
    questions: BTreeMap<String, String>,
    /// The project's agents other than the coordinator, by id.
    agents: BTreeMap<String, AgentFacts>,
}
impl ProjectFacts {
    /// Whether Git could not read an open ticket's branch head, so its commits are unknown.
    fn head_unknown(&self) -> bool {
        (self.tickets.iter()).any(|ticket| ticket.state.is_open() && ticket.head.is_none())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TicketFacts {
    id: String,
    title: String,
    state: TicketState,
    /// The branch's head while the ticket is open; `None` when Git could not say.
    head: Option<String>,
    cycle: Option<CycleFacts>,
    findings: BTreeMap<String, EntryStatus>,
    pull_request: Option<PullRequestFacts>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CycleFacts {
    cycle: u32,
    outcome: VerificationOutcome,
    rounds: usize,
    /// Each verifier's latest result, by focus.
    results: Vec<(String, VerifierResult)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PullRequestFacts {
    number: u64,
    state: PrState,
    checks: CheckState,
    unresolved_threads: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AgentFacts {
    name: String,
    /// Paused or disconnected.
    stopped: bool,
    held: bool,
}

impl Host {
    /// The facts of `coordinator`'s project as the store has them now.
    pub(crate) fn project_facts(&self, coordinator: &Session) -> Result<ProjectFacts> {
        let mut tickets = vec![];
        for ticket in self.tickets()? {
            if ticket.coordinator_id != coordinator.id {
                continue;
            }
            let head = match ticket.is_open() {
                true => self.branch_head(&ticket),
                false => None,
            };
            tickets.push(TicketFacts {
                head,
                cycle: ticket.verification.as_ref().map(|v| CycleFacts {
                    cycle: v.cycle,
                    outcome: v.outcome,
                    rounds: v.rounds.len(),
                    results: (v.latest_results().into_iter())
                        .map(|run| (run.focus.clone(), run.result))
                        .collect(),
                }),
                findings: (ticket.ledger.iter())
                    .map(|entry| (entry.id.clone(), entry.status))
                    .collect(),
                pull_request: ticket.pull_request.as_ref().map(|pr| PullRequestFacts {
                    number: pr.number,
                    state: pr.state,
                    checks: pr.checks,
                    unresolved_threads: pr.unresolved_threads,
                }),
                id: ticket.id,
                title: ticket.title,
                state: ticket.state,
            });
        }
        let questions = (self.open_attention(&coordinator.project_id)?.into_iter())
            .filter(|item| !is_check_in(&item.operation_id))
            .map(|item| {
                let prompt = item.prompt.lines().next().unwrap_or_default().to_owned();
                (item.id, prompt)
            })
            .collect();
        let mut agents = BTreeMap::new();
        for session in self.sessions()? {
            if session.project_id != coordinator.project_id
                || session.id == coordinator.id
                || session.archived
            {
                continue;
            }
            let held: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE recipient=?1 AND receipt='held')",
                [&session.id],
                |r| r.get(0),
            )?;
            let stopped = matches!(session.status, Status::Paused | Status::Disconnected);
            let facts = AgentFacts {
                name: session.name,
                stopped,
                held,
            };
            agents.insert(session.id, facts);
        }
        Ok(ProjectFacts {
            tickets,
            questions,
            agents,
        })
    }

    fn branch_head(&self, ticket: &Ticket) -> Option<String> {
        let repository = self.ticket_repository(ticket).ok()?;
        let branch = format!("refs/heads/{}", ticket.branch);
        let args = ["rev-parse", "--verify", "--quiet", branch.as_str()];
        let head = runtime::git_output(Path::new(&repository.path), &args).ok()?;
        Some(head.trim().to_owned()).filter(|head| !head.is_empty())
    }

    /// How a coordinator's reply at the end of a turn compares with its last full reply, and
    /// the new baseline when the reply shows in full. `input` is the turn's message ids and
    /// `news` whether the coordinator marked the reply as news. Only a turn the human did not
    /// start, whose input reported no failure and that is not news, can come out unchanged.
    pub(crate) fn reply_update(
        &mut self,
        coordinator: &Session,
        input: &[String],
        news: bool,
    ) -> Result<Option<ReplyUpdate>> {
        let messages: Vec<Message> = input
            .iter()
            .filter_map(|id| self.message(id).ok())
            .collect();
        let started_by_human = messages.is_empty() || messages.iter().any(Message::from_human);
        let trouble =
            (messages.iter()).any(|m| MessageKind::of(&m.id, &m.body).trouble().is_some());
        let facts = self.project_facts(coordinator)?;
        let baseline: Option<ProjectFacts> = self
            .db
            .query_row(
                "SELECT facts FROM reply_baselines WHERE session_id=?1",
                [&coordinator.id],
                |r| decode(r, 0),
            )
            .optional()?;
        let update = match baseline {
            Some(baseline) if !started_by_human => {
                let changes = changes(&baseline, &facts);
                // An unread head proves nothing, so the reply shows in full.
                let unknown = baseline.head_unknown() || facts.head_unknown();
                match changes.is_empty() {
                    false => Some(ReplyUpdate::Changed { changes }),
                    true if trouble || news || unknown => None,
                    true => Some(ReplyUpdate::Unchanged),
                }
            }
            _ => None,
        };
        if update != Some(ReplyUpdate::Unchanged) {
            self.db.execute(
                "INSERT INTO reply_baselines VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET facts=excluded.facts",
                params![coordinator.id, encode(&facts)?],
            )?;
        }
        Ok(update)
    }
}

/// Each difference from `old` to `new`, in words, tickets first.
fn changes(old: &ProjectFacts, new: &ProjectFacts) -> Vec<String> {
    let mut changes = vec![];
    for ticket in &new.tickets {
        let Some(before) = old.tickets.iter().find(|t| t.id == ticket.id) else {
            changes.push(format!("{} created", ticket.title));
            continue;
        };
        ticket_changes(before, ticket, &mut changes);
    }
    let opened = (new.questions.iter()).filter(|(id, _)| !old.questions.contains_key(*id));
    for (_, prompt) in opened {
        changes.push(format!("New question for you: {prompt}"));
    }
    let settled = (old.questions.keys()).filter(|id| !new.questions.contains_key(*id));
    match settled.count() {
        0 => {}
        1 => changes.push("1 question settled".into()),
        n => changes.push(format!("{n} questions settled")),
    }
    for (id, agent) in &new.agents {
        let name = &agent.name;
        let Some(before) = old.agents.get(id) else {
            changes.push(format!("{name} started"));
            continue;
        };
        if before.stopped != agent.stopped {
            changes.push(format!(
                "{name} {}",
                if agent.stopped { "stopped" } else { "resumed" }
            ));
        }
        if before.held != agent.held {
            changes.push(match agent.held {
                true => format!("{name} holds input"),
                false => format!("{name} no longer holds input"),
            });
        }
    }
    for (id, agent) in &old.agents {
        if !new.agents.contains_key(id) {
            changes.push(format!("{} archived", agent.name));
        }
    }
    changes
}

fn ticket_changes(old: &TicketFacts, new: &TicketFacts, changes: &mut Vec<String>) {
    let title = &new.title;
    if old.state != new.state {
        changes.push(format!("{title}: {}", new.state.name().replace('_', " ")));
    }
    if let (Some(before), Some(head)) = (&old.head, &new.head)
        && before != head
    {
        changes.push(format!("{title} committed {}", short_sha(head)));
    }
    match (&old.cycle, &new.cycle) {
        (_, None) => {}
        (Some(before), Some(cycle)) if before.cycle == cycle.cycle => {
            if before.rounds != cycle.rounds {
                changes.push(format!("{title}: review round {} started", cycle.rounds));
            }
            for (focus, result) in &cycle.results {
                let was = before.results.iter().find(|(f, _)| f == focus);
                if was.is_none_or(|(_, was)| was != result) && *result != VerifierResult::Pending {
                    changes.push(format!("{title}: {focus} {}", result.label()));
                }
            }
            if before.outcome != cycle.outcome {
                changes.push(format!(
                    "{title}: review cycle {} {}",
                    cycle.cycle,
                    word(&cycle.outcome)
                ));
            }
        }
        (_, Some(cycle)) => changes.push(format!("{title}: review cycle {} started", cycle.cycle)),
    }
    // Grouped by their new status, such as "F4, F5 noted".
    let mut findings: Vec<(EntryStatus, Vec<&str>)> = vec![];
    for (id, status) in &new.findings {
        if old.findings.get(id) != Some(status) {
            match findings.iter_mut().find(|(s, _)| s == status) {
                Some((_, ids)) => ids.push(id),
                None => findings.push((*status, vec![id])),
            }
        }
    }
    for (status, ids) in findings {
        changes.push(format!("{title}: {} {}", ids.join(", "), status.label()));
    }
    match (&old.pull_request, &new.pull_request) {
        (None, Some(pr)) => changes.push(format!("{title}: PR #{} opened", pr.number)),
        (Some(before), Some(pr)) if before != pr => {
            let mut parts = vec![];
            if before.state != pr.state {
                parts.push(word(&pr.state));
            }
            if before.checks != pr.checks {
                parts.push(format!("checks {}", word(&pr.checks)));
            }
            if before.unresolved_threads != pr.unresolved_threads {
                parts.push(format!("{} unresolved threads", pr.unresolved_threads));
            }
            if before.number != pr.number {
                parts.push("replaced".into());
            }
            changes.push(format!("{title}: PR #{} {}", pr.number, parts.join(", ")));
        }
        _ => {}
    }
}

/// A state's stored name as words, such as "failure" or "ready for testing".
fn word(value: &impl Serialize) -> String {
    let name = serde_json::to_value(value).unwrap_or_default();
    name.as_str().unwrap_or_default().replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::{Fixture, fixture};

    fn reply(f: &mut Fixture, input: &[String], news: bool) -> Option<ReplyUpdate> {
        let coordinator = f.coordinator.clone();
        f.host.reply_update(&coordinator, input, news).unwrap()
    }
    /// A check-in for the coordinator, the input of a turn the human did not start.
    fn check_in(f: &mut Fixture) -> Vec<String> {
        let id = format!("check-in-{}", new_id());
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        let body = check_in_body("Implementer has run 30 minutes");
        f.host
            .send(&id, Some(&implementer), &coordinator, &body)
            .unwrap();
        vec![id]
    }
    /// A fixture whose coordinator has replied once in full, after the human's message.
    fn baselined() -> Fixture {
        let mut f = fixture();
        let coordinator = f.coordinator.id.clone();
        f.host.send("human", None, &coordinator, "Go").unwrap();
        assert_eq!(reply(&mut f, &["human".into()], false), None);
        f
    }
    fn changed(update: Option<ReplyUpdate>) -> Vec<String> {
        match update {
            Some(ReplyUpdate::Changed { changes }) => changes,
            other => panic!("not changed: {other:?}"),
        }
    }

    #[test]
    fn a_background_reply_with_nothing_new_is_unchanged_and_keeps_the_baseline() {
        let mut f = baselined();
        for _ in 0..2 {
            let input = check_in(&mut f);
            assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
        }
    }

    #[test]
    fn the_first_background_reply_without_a_baseline_shows_in_full() {
        let mut f = fixture();
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), None);
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
    }

    #[test]
    fn each_kind_of_fact_names_its_change_and_becomes_the_new_baseline() {
        let mut f = baselined();
        let title = f.ticket().title;
        // The branch's head.
        let head = f.commit("c.txt", "three\n");
        let input = check_in(&mut f);
        assert_eq!(
            changed(reply(&mut f, &input, false)),
            [format!("{title} committed {}", short_sha(&head))]
        );
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
        // The ticket's state, a new cycle, a finding and a PR at once.
        let mut ticket = f.ticket();
        ticket.state = TicketState::Verifying;
        ticket.ledger.push(LedgerEntry {
            id: "F1".into(),
            finding: serde_json::from_value(json!({"severity":"non_blocking","location":"a.txt:1","summary":"s","trigger":"t","evidence":"e"})).unwrap(),
            focus: "Correctness".into(),
            cycle: 2,
            round: 1,
            status: EntryStatus::Noted,
            reason: None,
        });
        f.host.save_ticket(&ticket).unwrap();
        f.review(VerificationOutcome::Passed);
        let mut ticket = f.ticket();
        ticket.pull_request = Some(serde_json::from_value(json!({"number":12,"url":"u","state":"open","draft":false,"base":"main","head":"h","merge_state":"clean","checks":"pending","unresolved_threads":0,"comments":{}})).unwrap());
        f.host.save_ticket(&ticket).unwrap();
        let input = check_in(&mut f);
        let changes = changed(reply(&mut f, &input, false));
        for expected in [
            "review cycle 2 started".to_owned(),
            "F1 noted".into(),
            "PR #12 opened".into(),
        ] {
            assert!(
                changes.iter().any(|c| c == &format!("{title}: {expected}")),
                "{expected} in {changes:?}"
            );
        }
        // The PR's checks.
        let mut ticket = f.ticket();
        ticket.pull_request.as_mut().unwrap().checks = CheckState::Failure;
        f.host.save_ticket(&ticket).unwrap();
        let input = check_in(&mut f);
        assert_eq!(
            changed(reply(&mut f, &input, false)),
            [format!("{title}: PR #12 checks failure")]
        );
        // An open question.
        let coordinator = f.coordinator.id.clone();
        f.host
            .request_attention(&coordinator, "local", "q1", "Ship it?\nMore", &[])
            .unwrap();
        let input = check_in(&mut f);
        assert_eq!(
            changed(reply(&mut f, &input, false)),
            ["New question for you: Ship it?"]
        );
        // An agent stopped, and one that started.
        let implementer = f.implementer.clone();
        f.host.set_status(&implementer.id, Status::Paused).unwrap();
        let input = check_in(&mut f);
        assert_eq!(
            changed(reply(&mut f, &input, false)),
            [format!("{} stopped", implementer.name)]
        );
        let ticket = f.ticket.clone();
        let reviewer = f
            .host
            .assign_ticket(&ticket, Role::Reviewer, Provider::Claude, "Review", None)
            .unwrap();
        let input = check_in(&mut f);
        let changes = changed(reply(&mut f, &input, false));
        assert!(changes.contains(&format!("{} started", reviewer.name)));
    }

    #[test]
    fn a_turn_the_human_started_never_folds_and_resets_the_baseline() {
        let mut f = baselined();
        f.commit("c.txt", "three\n");
        let coordinator = f.coordinator.id.clone();
        f.host
            .send("answer:q", None, &coordinator, "Human answer to q: yes")
            .unwrap();
        let input = vec![check_in(&mut f).remove(0), "answer:q".into()];
        assert_eq!(reply(&mut f, &input, false), None);
        // The commit is in the new baseline, so the next check-in finds nothing new.
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
        // A turn with no input, such as a retry, is the human's too.
        assert_eq!(reply(&mut f, &[], false), None);
    }

    #[test]
    fn failed_or_blocked_input_and_news_keep_an_unchanged_reply_full() {
        let mut f = baselined();
        let (implementer, coordinator) = (f.implementer.id.clone(), f.coordinator.id.clone());
        for (id, kind) in [("r1", ReportKind::Failed), ("r2", ReportKind::Blocked)] {
            let body = report_body(kind, "Implementer", "Tests fail");
            f.host
                .send(id, Some(&implementer), &coordinator, &body)
                .unwrap();
            assert_eq!(reply(&mut f, &[id.into()], false), None);
        }
        let error = turn_error_body("Implementer", "exit 1");
        let id = turn_result_id("run-1");
        f.host
            .send(&id, Some(&implementer), &coordinator, &error)
            .unwrap();
        assert_eq!(reply(&mut f, &[id], false), None);
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, true), None);
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
    }

    #[test]
    fn an_unreadable_branch_head_never_reads_as_unchanged() {
        let mut f = baselined();
        // The repository is unavailable, so the head cannot be read.
        let moved = f.repository.with_extension("away");
        std::fs::rename(&f.repository, &moved).unwrap();
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), None);
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), None);
        // Back with a new commit: the baseline knew no head, so this proves nothing either.
        std::fs::rename(&moved, &f.repository).unwrap();
        f.commit("c.txt", "three\n");
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), None);
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
    }

    #[test]
    fn the_result_and_the_baseline_survive_a_restart() {
        let mut f = baselined();
        let coordinator = f.coordinator.clone();
        let changed = ReplyUpdate::Changed {
            changes: vec!["Fix contrast committed 684cd0f".into()],
        };
        (f.host)
            .append_reply(&coordinator, "run-1", "Still testing.", Some(&changed))
            .unwrap();
        let (home, spare) = (f.host.home.clone(), tempfile::tempdir().unwrap());
        drop(std::mem::replace(
            &mut f.host,
            Host::open(spare.path()).unwrap(),
        ));
        f.host = Host::open(home).unwrap();
        let stored = f.host.message(&reply_id("run-1")).unwrap();
        assert_eq!(
            (stored.body.as_str(), stored.update),
            ("Still testing.", Some(changed))
        );
        let input = check_in(&mut f);
        assert_eq!(reply(&mut f, &input, false), Some(ReplyUpdate::Unchanged));
    }
}
