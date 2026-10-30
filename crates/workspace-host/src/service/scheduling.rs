//! Admission inputs and ticket occupancy, separate from provider execution.
use super::*;

type Running = HashMap<String, (Session, Option<String>)>;
#[derive(Default)]
pub(super) struct Admission {
    dirty: bool,
    pub(super) wake_at: Option<i64>,
    running: Option<Running>,
}
impl Admission {
    pub(super) fn observe(&mut self, event: &Event) {
        match event {
            Event::Wake(at) if self.wake_at != Some(*at) => {}
            Event::Overlaps { .. }
            | Event::GitHub(_)
            | Event::SkillMerged(..)
            | Event::CreationInspected(..) => {}
            Event::Command(command, _) if command.is_read() => {}
            Event::Tool { name, .. }
                if matches!(name.as_str(), "workspace_context" | "set_plan") => {}
            Event::Provider {
                event: ProviderEvent::Finished { .. },
                ..
            } => self.dirty = true,
            Event::Provider { .. } => {}
            _ => self.dirty = true,
        }
    }
    pub(super) fn take_due(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
    pub(super) fn started(&mut self, session: &Session, ticket: Option<String>) {
        if let Some(running) = &mut self.running {
            running.insert(session.id.clone(), (session.clone(), ticket));
        }
    }
}
fn eligible(session: &Session, active: bool, held: bool) -> bool {
    !session.archived
        && !matches!(session.status, Status::Paused | Status::Disconnected)
        && !active
        && !held
}
fn round_available(ticket: &Ticket, members: &[Session], running: &Running) -> bool {
    members.iter().all(|member| {
        eligible(member, running.contains_key(&member.id), false)
            && !busy(ticket, member, running.values())
    })
}
fn busy<'a>(
    ticket: &Ticket,
    session: &Session,
    running: impl Iterator<Item = &'a (Session, Option<String>)>,
) -> bool {
    if session.role == Role::Keeper {
        return false;
    }
    let reader = |session: &Session| {
        session.role == Role::Reviewer
            || ticket.running_cycle().is_some_and(|cycle| {
                cycle
                    .rounds
                    .iter()
                    .flat_map(|r| &r.verifiers)
                    .any(|v| v.session_id == session.id)
            })
    };
    running.into_iter().any(|(active, active_ticket)| {
        active_ticket.as_deref() == Some(&ticket.id)
            && active.role != Role::Keeper
            && !(reader(session) && reader(active))
    })
}
impl Actor {
    pub(super) fn schedule(&mut self) -> Result<()> {
        #[cfg(test)]
        {
            self.schedule_runs += 1;
        }
        self.scheduling.running = Some(
            self.active
                .keys()
                .map(|id| {
                    Ok((
                        id.clone(),
                        (
                            self.host.session(id)?,
                            self.host.session_runtime(id)?.ticket_id,
                        ),
                    ))
                })
                .collect::<Result<_>>()?,
        );
        // What a pass sends, such as a held turn's report or a check-in, may wake a session
        // the pass already went by; nothing else may come to admit it.
        let result = loop {
            let sent = self.last_message()?;
            if let Err(error) = self.admit() {
                break Err(error);
            }
            if self.last_message()? == sent {
                break Ok(());
            }
        };
        self.scheduling.running = None;
        self.prepared_worktrees.clear();
        result
    }
    fn last_message(&self) -> Result<Option<i64>> {
        Ok(self
            .host
            .db
            .query_row("SELECT MAX(sequence) FROM messages", [], |r| r.get(0))?)
    }
    fn admit(&mut self) -> Result<()> {
        self.check_in_long_turns(now());
        self.settle_leftover_notes()?;
        // Timers fire into the queue whether or not the project is live.
        match self.host.fire_due_schedules(now()) {
            Ok(pass) => {
                if let Some(next) = pass.next {
                    self.wake(next);
                }
                // Fires and next-fire times show in the client even when no turn starts.
                if pass.changed {
                    self.state_changed();
                }
            }
            Err(e) => eprintln!("Workspace timers: {e:#}"),
        }
        // A queued compaction runs before any input is taken.
        for session in self.due_compactions()? {
            // One session's compaction must never stop the others from being scheduled.
            if let Err(error) = self.start_compaction(session) {
                eprintln!("Workspace compaction: {error:#}");
            }
        }
        self.retry_after_limits(now())?;
        let in_rounds = self.admit_rounds()?;
        // Quiet messages ride along with the next turn but never start one. Each recipient's
        // oldest due message stands for it, so a long queue cannot hide other sessions.
        let messages=self.host.db.prepare(&format!("SELECT m.*,MIN(m.sequence) AS first FROM messages m JOIN live_projects p ON p.project_id=m.project_id AND p.enabled=1 JOIN sessions s ON s.id=m.recipient WHERE m.receipt='queued' AND m.quiet=0 AND COALESCE(json_extract(s.data,'$.archived'),0)=0 AND {} AND NOT EXISTS (SELECT 1 FROM messages h WHERE h.recipient=m.recipient AND h.receipt='held') GROUP BY m.recipient ORDER BY s.role<>'project_orchestrator',first", is_turn_input("m")))?.query_map([],Host::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for message in messages {
            if in_rounds.contains(&message.recipient) {
                continue;
            }
            let session = self.host.session(&message.recipient)?;
            if !eligible(&session, self.active.contains_key(&session.id), false) {
                continue;
            }
            let Some(input) = self.due_input(&session.id)? else {
                continue;
            };
            let input = self.before_unshown_reports(input);
            if input.is_empty() {
                continue;
            }
            let runtime = self.host.session_runtime(&session.id)?;
            if let Some(ticket) = &runtime.ticket_id
                && self.worktree_busy(ticket, &session)?
            {
                continue;
            }
            if let Some(reason) = self.hold_reason(&session)? {
                // One session's hold must never stop the others from being scheduled.
                if let Err(error) = self.hold_turn(&session, &input, &reason) {
                    eprintln!("Holding {}: {error:#}", session.id);
                }
                continue;
            }
            // The input stays queued for the first turn after the compaction settles.
            if runtime.compaction.is_some() {
                continue;
            }
            if let Err(error) = self.start_turn(session.clone(), input) {
                self.start_failed(&session, &message, &error)?;
            }
        }
        Ok(())
    }
    /// Queues again the held input of each turn that failed on a usage limit, once that limit
    /// has reset: at the reset when it was known, else at the first reading since the failure
    /// that no longer says it is reached. The retry is the human's Retry, so the input carries
    /// its interrupted-turn note. A session paused meanwhile waits for the human.
    pub(super) fn retry_after_limits(&mut self, at: i64) -> Result<()> {
        for mut runtime in self.host.runtimes()? {
            let Some(retry) = runtime.limit_retry.clone() else {
                continue;
            };
            let session = self.host.session(&runtime.session_id)?;
            if session.status != Status::Disconnected || self.active.contains_key(&session.id) {
                continue;
            }
            if let Some(spent) = self.host.limit_reached(session.provider, at)? {
                // A later reading may move the reset; the conversation shows the latest.
                if spent.until != retry.resets_at {
                    runtime.limit_retry = Some(LimitRetry {
                        resets_at: spent.until,
                        ..retry
                    });
                    self.host.save_runtime(&runtime)?;
                    self.state_changed();
                }
                if let Some(until) = spent.until {
                    self.wake(until);
                }
                continue;
            }
            // Only a usable reading says the limit reset; a failed refresh says nothing.
            let read_since = self.host.quotas()?.iter().any(|q| {
                q.provider == session.provider
                    && q.observed_at > retry.failed_at
                    && q.error.is_none()
                    && !q.windows.is_empty()
            });
            if retry.resets_at.is_some_and(|reset| reset <= at) || read_since {
                self.host.reconcile_session(&session.id, true)?;
                Host::event(
                    &self.host.db,
                    &session.project_id,
                    Some(&session.id),
                    "limit_retried",
                    "Held input retried after the usage limit reset",
                )?;
                self.state_changed();
            }
        }
        Ok(())
    }
    /// Sessions whose queued compaction starts now: whenever a turn of theirs could, except
    /// that it needs no input, no free worktree and no admitted round.
    pub(super) fn due_compactions(&mut self) -> Result<Vec<Session>> {
        let live = self.host.live_projects()?;
        let mut due = vec![];
        for session in self.host.queued_compactions()? {
            if eligible(
                &session,
                self.active.contains_key(&session.id) || self.compacting.contains_key(&session.id),
                self.host.held_input(&session.id)? > 0,
            ) && live.contains(&session.project_id)
                && self.hold_reason(&session)?.is_none()
            {
                due.push(session);
            }
        }
        Ok(due)
    }

    /// A ticket's worktree has one writer at a time. Readers run together: reviewers, and the
    /// testers verifying the ticket in its running cycle. PR keeper agents work elsewhere.
    pub(super) fn worktree_busy(&self, ticket_id: &str, session: &Session) -> Result<bool> {
        if session.role == Role::Keeper {
            return Ok(false);
        }
        if self.host.checkout_reservations.contains(ticket_id) {
            return Ok(true);
        }
        let ticket = self.host.ticket(ticket_id)?;
        if let Some(running) = &self.scheduling.running {
            return Ok(busy(&ticket, session, running.values()));
        }
        let running = self
            .active
            .keys()
            .map(|id| {
                Ok((
                    self.host.session(id)?,
                    self.host.session_runtime(id)?.ticket_id,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(busy(&ticket, session, running.iter()))
    }
    /// Starts the queued cycles that now have a slot, then each verification round that can
    /// start. A round's verifiers start together or not at all, so they are never scheduled one
    /// by one; returns them all.
    pub(super) fn admit_rounds(&mut self) -> Result<HashSet<String>> {
        if self.host.start_queued_cycles()? {
            self.state_changed();
        }
        let mut members_of_rounds = HashSet::new();
        for (ticket, members) in self.waiting_rounds()? {
            self.admit_round(&ticket, &members)?;
            members_of_rounds.extend(members.into_iter().map(|s| s.id));
        }
        Ok(members_of_rounds)
    }
    /// Each running cycle's current round whose verifiers still have their round message queued.
    pub(super) fn waiting_rounds(&self) -> Result<Vec<(Ticket, Vec<Session>)>> {
        let mut rounds = vec![];
        for ticket in self.host.running_verifications()? {
            let Some(round) = ticket.running_cycle().and_then(Verification::current_round) else {
                continue;
            };
            let mut members = vec![];
            for run in &round.verifiers {
                if self
                    .host
                    .message(&run.message_id)
                    .is_ok_and(|m| m.receipt == Receipt::Queued)
                {
                    members.push(self.host.session(&run.session_id)?);
                }
            }
            if !members.is_empty() {
                rounds.push((ticket, members));
            }
        }
        Ok(rounds)
    }
    /// Starts every member of a round, or none of them.
    pub(super) fn admit_round(&mut self, ticket: &Ticket, members: &[Session]) -> Result<()> {
        if !self.host.live_projects()?.contains(&members[0].project_id) {
            return Ok(());
        }
        if self
            .scheduling
            .running
            .as_ref()
            .is_some_and(|running| !round_available(ticket, members, running))
        {
            return Ok(());
        }
        // Each member's hold is asked once: an answer that changed between two questions could
        // send a member with a compaction on to `start_turn`.
        let mut holds = vec![];
        for member in members {
            if member.archived
                || matches!(member.status, Status::Paused | Status::Disconnected)
                || self.host.held_input(&member.id)? > 0
                || self.active.contains_key(&member.id)
                || self.worktree_busy(&ticket.id, member)?
            {
                return Ok(());
            }
            let hold = self.hold_reason(member)?;
            // A compaction comes before the member's turn, so the round waits whole. A member
            // whose turn is held anyway is held below instead of waiting for a compaction that
            // cannot start either.
            let compaction = self.host.session_runtime(&member.id)?.compaction;
            if hold.is_none() && compaction.is_some() {
                return Ok(());
            }
            holds.push(hold);
        }
        let mut inputs = vec![];
        for (member, hold) in members.iter().zip(holds) {
            match self.due_input(&member.id)? {
                Some(input) => inputs.push((member.clone(), input, hold)),
                None => return Ok(()),
            }
        }
        for (member, input, hold) in inputs {
            // A held verifier never runs on a substitute, and the rest of the round runs. One
            // with no model reports blocked, so the round ends and wakes the coordinator; one
            // waiting for a reset reports nothing and runs after it, keeping its cycle open.
            if let Some(reason) = hold {
                if let Err(error) = self.hold_turn(&member, &input, &reason) {
                    eprintln!("Holding {}: {error:#}", member.id);
                }
                continue;
            }
            let first = input[0].clone();
            if let Err(error) = self.start_turn(member.clone(), input) {
                self.start_failed(&member, &first, &error)?;
            }
        }
        Ok(())
    }

    pub(super) fn due_input(&mut self, recipient: &str) -> Result<Option<Vec<Message>>> {
        let input = self.host.db.prepare(&format!("SELECT * FROM messages WHERE recipient=?1 AND receipt='queued' AND {} ORDER BY sequence LIMIT ?2", is_turn_input("messages")))?.query_map(params![recipient, MAX_TURN_MESSAGES],Host::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        // Urgency covers the whole queue, not just the bounded input.
        let urgent: bool = self.host.db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM messages WHERE recipient=?1 AND receipt='queued' AND quiet=0 AND {} AND NOT ({IS_PROGRESS_REPORT}))", is_turn_input("messages")), [recipient], |r| r.get(0))?;
        let due = input
            .first()
            .map_or(0, |m| m.created_at + PROGRESS_BATCH_MS);
        if urgent || now() >= due {
            return Ok(Some(input));
        }
        self.wake(due);
        Ok(None)
    }
    /// The input up to the first report of a reporter whose turn still hides the ticket state it
    /// reported (see `Host::shown_ticket`), so its parent wakes to that report only when that
    /// state shows. Input is delivered in order, so what came after the report waits with it;
    /// the reporter's turn ending admits them.
    fn before_unshown_reports(&self, input: Vec<Message>) -> Vec<Message> {
        let reporters = self.host.mid_turn_reports.values().map(|r| &r.reporter);
        let prefixes: Vec<String> = reporters.map(|r| format!("report:{r}:")).collect();
        input
            .into_iter()
            .take_while(|m| !prefixes.iter().any(|prefix| m.id.starts_with(prefix)))
            .collect()
    }
    /// Arms one wake-up for `at` unless an earlier one is already armed.
    pub(super) fn wake(&mut self, at: i64) {
        if self.scheduling.wake_at.is_some_and(|armed| armed <= at) {
            return;
        }
        self.scheduling.wake_at = Some(at);
        let sender = self.sender.clone();
        let wait = (at - now()).clamp(0, MAX_WAKE_SLEEP_MS) as u64;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let _ = sender.send_blocking(Event::Wake(at));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_and_streaming_do_not_admit_but_finishes_mutations_and_timers_do() {
        let mut admission = Admission::default();
        let (reply, _) = async_channel::bounded(1);
        admission.observe(&Event::Command(Command::Snapshot, reply));
        admission.observe(&Event::Provider {
            session: "a".into(),
            run: "r".into(),
            event: ProviderEvent::Reply("text".into()),
        });
        assert!(!admission.take_due());
        admission.observe(&Event::Provider {
            session: "a".into(),
            run: "r".into(),
            event: ProviderEvent::Finished {
                error: None,
                usage: Value::Null,
                cancelled: false,
            },
        });
        assert!(admission.take_due());
        assert!(!admission.take_due());
        admission.wake_at = Some(7);
        admission.observe(&Event::Wake(6));
        assert!(!admission.take_due());
        admission.observe(&Event::Wake(7));
        assert!(admission.take_due());
    }
    #[test]
    fn a_round_is_admitted_whole_and_readers_share_a_checkout_but_not_with_a_writer() {
        let f = publish::tests::fixture();
        let ticket = f.ticket();
        let mut first = f.implementer.clone();
        first.id = "one".into();
        first.role = Role::Reviewer;
        first.status = Status::Ready;
        let mut second = first.clone();
        second.id = "two".into();
        let mut running = Running::new();
        assert!(round_available(
            &ticket,
            &[first.clone(), second.clone()],
            &running
        ));
        second.status = Status::Paused;
        assert!(!round_available(
            &ticket,
            &[first.clone(), second.clone()],
            &running
        ));
        second.status = Status::Ready;
        let mut reader = first.clone();
        reader.id = "other".into();
        running.insert(reader.id.clone(), (reader.clone(), Some(ticket.id.clone())));
        assert!(round_available(
            &ticket,
            &[first.clone(), second.clone()],
            &running
        ));
        reader.role = Role::Implementer;
        running.insert(reader.id.clone(), (reader, Some(ticket.id.clone())));
        assert!(!round_available(&ticket, &[first, second], &running));
    }
}
