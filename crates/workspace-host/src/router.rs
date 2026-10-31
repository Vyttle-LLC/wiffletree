//! The router's host rules: each message from the assistant's composer is held unsent while
//! the router decides off the host thread, then sent where its answer says, or held until the
//! user answers when it is unsure. Nothing reaches a thread, and so no brain, before then.
use crate::*;
use workspace_core::assistant::router::{
    CurrentThread, Destination, OpenThread, Question, RouterAnswer, RouterInput, RouterSpace,
    last_messages,
};
use workspace_core::assistant::{
    HeldMessage, MovedMessage, RoutedLine, Thread, is_notice, title_from_message,
};

/// A router run for the service to start off the host thread.
pub struct RouterRun {
    pub message_id: String,
    pub input: RouterInput,
    pub profile: ModelProfile,
}

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS held_messages (id TEXT PRIMARY KEY, data TEXT NOT NULL);",
    )?;
    Ok(())
}

impl Host {
    pub fn held_messages(&self) -> Result<Vec<HeldMessage>> {
        self.list_data("SELECT data FROM held_messages ORDER BY rowid", [])
    }
    fn held_message(&self, id: &str) -> Result<Option<HeldMessage>> {
        Ok(self
            .db
            .query_row("SELECT data FROM held_messages WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?)
    }
    fn hold(&self, held: &HeldMessage) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO held_messages VALUES (?1,?2)",
            params![held.id, encode(held)?],
        )?;
        Ok(())
    }

    /// Sends a message from the assistant's composer, sent in thread `from` or an empty tab,
    /// where the router decides. It is held while the router runs; with one Space and no
    /// thread to continue or switch to, it goes to a new thread without one. Sending the same
    /// id again changes nothing.
    pub fn route_message(&mut self, id: &str, from: Option<&str>, body: &str) -> Result<()> {
        ensure!(!body.trim().is_empty(), "Write a message first");
        // A new thread the message starts takes its id.
        ensure!(
            Path::new(id).file_name() == Some(id.as_ref())
                && !id.starts_with('.')
                && id.len() <= 64,
            "A message id is a plain name"
        );
        let sent = self.message(id).is_ok();
        if sent || self.held_message(id)?.is_some() {
            return Ok(());
        }
        let input = self.router_input(from, body)?;
        let mut held = HeldMessage {
            id: id.into(),
            origin: from.map(str::to_owned),
            body: body.into(),
            created_at: now(),
            question: None,
            title: String::new(),
        };
        if let Some(destination) = input.forced() {
            return self.deliver(&held, &destination, None);
        }
        match self.router_profile() {
            Ok(profile) => {
                self.router_runs.push(RouterRun {
                    message_id: id.into(),
                    input,
                    profile,
                });
            }
            Err(why) => {
                let threshold = self.shared().router_threshold();
                held.question = Some(input.route(Err(why), threshold).question);
            }
        }
        self.hold(&held)?;
        Self::event(&self.db, "", from, "message_held", id)
    }
    /// The Router row's pick, or why the router does not run: it never waits.
    fn router_profile(&self) -> Result<ModelProfile, String> {
        match self.resolve_job(&Job::Router, false, false) {
            Ok(pick) if !pick.waits => Ok(pick.profile),
            Ok(_) => Err("The router did not run: every choice on its row is spent.".into()),
            Err(error) => Err(format!("The router did not run: {error:#}")),
        }
    }
    pub(crate) fn take_router_runs(&mut self) -> Vec<RouterRun> {
        std::mem::take(&mut self.router_runs)
    }
    /// Acts on a router run's answer, or on why it has none, by the host rules.
    pub(crate) fn finish_router_run(
        &mut self,
        run: RouterRun,
        answer: Result<RouterAnswer, String>,
    ) -> Result<()> {
        let Some(mut held) = self.held_message(&run.message_id)? else {
            // The user sent it somewhere already.
            return Ok(());
        };
        if let Ok(answer) = &answer {
            held.title = answer.title.clone();
        }
        let mut routed = run.input.route(answer, self.shared().router_threshold());
        if let Some(destination) = &routed.send {
            match self.deliver(&held, destination, Some(&routed.question.choices)) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let why = "Where the router sent it changed while it decided.";
                    routed.question.why = why.into();
                }
            }
        }
        held.question = Some(routed.question);
        self.hold(&held)?;
        Self::event(&self.db, "", None, "router_unsure", &held.id)
    }
    /// Messages a stopped host was routing are asked about, since their router runs ended
    /// with it.
    pub(crate) fn ask_about_interrupted_routes(&mut self) -> Result<()> {
        let why = "The router stopped when Wiffletree quit.";
        for mut held in self.held_messages()? {
            if held.question.is_some() {
                continue;
            }
            let origin = (held.origin.as_deref())
                .filter(|id| self.thread(id).is_ok_and(|t| t.project_id.is_none()));
            let question = match self.router_input(origin, &held.body) {
                Ok(input) => {
                    let threshold = self.shared().router_threshold();
                    input.route(Err(why.into()), threshold).question
                }
                Err(_) => Question {
                    why: why.into(),
                    choices: vec![],
                },
            };
            held.question = Some(question);
            self.hold(&held)?;
        }
        Ok(())
    }
    /// Sends a held message where the user chose.
    pub fn answer_held(&mut self, id: &str, destination: &Destination) -> Result<()> {
        let held = (self.held_message(id)?).context("That message was already sent")?;
        let choices = (held.question.as_ref()).map_or(vec![], |q| q.choices.clone());
        self.deliver(&held, destination, Some(&choices))
    }
    /// Sends a held message to `destination` and lets it go. One that went elsewhere than the
    /// thread it was sent from leaves a line there, and one from an empty tab in the thread it
    /// went to, with Change offering `choices`; with `None`, nothing was decided and there is
    /// no line.
    fn deliver(
        &mut self,
        held: &HeldMessage,
        destination: &Destination,
        choices: Option<&[Destination]>,
    ) -> Result<()> {
        self.atomically(|host| {
            let (to, new) = host.send_to(destination, &held.id, &held.body, &held.title)?;
            let holder = held.origin.clone().unwrap_or(to.clone());
            if let Some(choices) = choices
                && (held.origin.is_none() || holder != to)
            {
                let line = RoutedLine {
                    message_id: held.id.clone(),
                    body: held.body.clone(),
                    created_at: held.created_at,
                    to,
                    new,
                    choices: choices.to_vec(),
                };
                host.add_routed_line(&holder, line)?;
            }
            host.db
                .execute("DELETE FROM held_messages WHERE id=?1", [&held.id])?;
            Ok(())
        })
    }
    /// Sends `body` as message `id` to a thread, or starts one with it, which takes the
    /// message's id, so the view that sent it can find it; the thread's id and whether it is
    /// new.
    fn send_to(
        &mut self,
        destination: &Destination,
        id: &str,
        body: &str,
        title: &str,
    ) -> Result<(String, bool)> {
        match destination {
            Destination::Thread { thread_id } => {
                self.thread(thread_id)?;
                self.send(id, None, thread_id, body)?;
                Ok((thread_id.clone(), false))
            }
            Destination::NewThread { space_id } => {
                let title = match title.trim() {
                    "" => title_from_message(body),
                    title => title.to_owned(),
                };
                let first = assistant::FirstMessage {
                    id,
                    body,
                    files: &[],
                };
                let thread = self.begin_thread(id, space_id, &title, first, None)?;
                Ok((thread.id, true))
            }
        }
    }
    fn add_routed_line(&mut self, origin: &str, line: RoutedLine) -> Result<()> {
        // A thread deleted while its message was held keeps no line.
        let Ok(mut from) = self.thread(origin) else {
            return Ok(());
        };
        from.routed.push(line);
        self.save_thread(&from)
    }
    fn take_routed_line(&mut self, origin: &str, message_id: &str) -> Result<RoutedLine> {
        let mut from = self.thread(origin)?;
        let at = (from.routed.iter()).position(|l| l.message_id == message_id);
        let line = from
            .routed
            .remove(at.context("That message did not leave this thread")?);
        self.save_thread(&from)?;
        Ok(line)
    }

    /// The thread a routed message's line in `holder` says it went to.
    pub(crate) fn routed_to(&self, holder: &str, message_id: &str) -> Result<String> {
        let from = self.thread(holder)?;
        let line = (from.routed.iter()).find(|l| l.message_id == message_id);
        Ok(line.context("That message has no line here")?.to.clone())
    }
    /// Change on a routed message's line in `holder`: marks the message and its reply in the
    /// thread it went to as moved, cancels it there if it has not been delivered yet, and sends
    /// it to `destination` as a new message. The line stays in the thread it was sent from, or,
    /// for a message from an empty tab, moves with it. The service stops the turn it started
    /// first, if that still runs.
    pub fn change_route(
        &mut self,
        holder: &str,
        message_id: &str,
        destination: &Destination,
    ) -> Result<()> {
        let went_to = self.routed_to(holder, message_id)?;
        if *destination
            == (Destination::Thread {
                thread_id: went_to.clone(),
            })
        {
            return Ok(());
        }
        let from_empty_tab = went_to == holder;
        self.atomically(|host| {
            let line = host.take_routed_line(holder, message_id)?;
            let id = new_id();
            let (to, new) = host.send_to(destination, &id, &line.body, "")?;
            if let Ok(mut left) = host.thread(&went_to) {
                left.moved.push(MovedMessage {
                    message_id: message_id.into(),
                    to: to.clone(),
                });
                host.save_thread(&left)?;
            }
            host.db.execute(
                "UPDATE messages SET receipt='cancelled' WHERE id=?1 AND receipt='queued'",
                [message_id],
            )?;
            let holder = if from_empty_tab { to.as_str() } else { holder };
            if from_empty_tab || to != holder {
                let line = RoutedLine {
                    message_id: id.clone(),
                    to: to.clone(),
                    new,
                    ..line
                };
                host.add_routed_line(holder, line)?;
            }
            Self::event(&host.db, "", Some(&to), "route_changed", message_id)
        })
    }
    /// What the router reads: the message, the thread it was sent from with its latest
    /// messages, the other threads in the tab strip and the Spaces that are on, by id, title,
    /// name and description only.
    fn router_input(&self, from: Option<&str>, body: &str) -> Result<RouterInput> {
        let at = now();
        let spaces: Vec<RouterSpace> = (self.spaces()?.into_iter())
            .filter(|record| record.local.enabled)
            .map(|record| RouterSpace {
                id: record.space.id,
                name: record.space.name,
                description: record.space.description,
            })
            .collect();
        ensure!(!spaces.is_empty(), "Add a Space in Settings › Spaces first");
        let on = |space: &SpaceId| spaces.iter().any(|s| &s.id == space);
        let current = match from {
            Some(id) => {
                let thread = self.thread(id)?;
                ensure!(
                    thread.project_id.is_none(),
                    "This thread became a project and is read-only"
                );
                Some(CurrentThread {
                    last_messages: self.router_lines(&thread)?,
                    id: thread.id,
                    title: thread.title,
                    space_id: thread.space_id,
                })
            }
            None => None,
        };
        let open_threads = (self.threads()?.into_iter())
            .filter(|t| t.in_strip(at) && t.project_id.is_none() && on(&t.space_id))
            .filter(|t| current.as_ref().is_none_or(|c| c.id != t.id))
            .map(|t| OpenThread {
                id: t.id,
                title: t.title,
                space_id: t.space_id,
            })
            .collect();
        Ok(RouterInput {
            message: body.into(),
            current_thread: current,
            open_threads,
            spaces,
        })
    }
    /// A thread's latest messages for the router, without the host's notices, or the messages
    /// moved out of it and their replies.
    fn router_lines(&self, thread: &Thread) -> Result<Vec<String>> {
        let mut rows: Vec<(String, Option<String>, String)> = self
            .db
            .prepare("SELECT id,sender,body FROM messages WHERE recipient=?1 ORDER BY sequence DESC LIMIT 50")?
            .query_map([&thread.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        let mut moving = false;
        let mut kept = vec![];
        for (id, sender, body) in &rows {
            if sender.is_none() {
                moving = thread.moved.iter().any(|m| m.message_id == *id);
            }
            if !moving && !is_notice(id) {
                kept.push((sender.is_none(), body.as_str()));
            }
        }
        Ok(last_messages(kept.into_iter().rev()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::assistant::router::{Choice, ThreadPick};

    /// A store with the Personal Space every store has and a Sage Space whose brain is a
    /// folder, so a test can see nothing was written to it.
    struct Store {
        _home: tempfile::TempDir,
        brain: tempfile::TempDir,
        host: Host,
        sage: SpaceId,
        personal: SpaceId,
    }
    fn store() -> Store {
        let home = tempfile::tempdir().unwrap();
        let brain = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let sage = (host
            .create_space("Sage", "Sage work", "", None)
            .unwrap()
            .space)
            .id;
        host.set_space_brain(&sage, &brain.path().to_string_lossy())
            .unwrap();
        let personal = (host.spaces().unwrap().into_iter())
            .find(|s| s.space.id != sage)
            .unwrap()
            .space
            .id;
        Store {
            _home: home,
            brain,
            host,
            sage,
            personal,
        }
    }
    fn answer(thread: ThreadPick, space: &SpaceId, confidence: f64) -> RouterAnswer {
        RouterAnswer {
            thread: Choice {
                pick: thread,
                confidence,
                alternatives: vec![],
            },
            space: Choice {
                pick: space.clone(),
                confidence,
                alternatives: vec![],
            },
            title: "Dentist Tuesday".into(),
        }
    }
    /// The messages any thread holds, which no held message is among.
    fn thread_messages(host: &Host) -> i64 {
        host.db
            .query_row(
                "SELECT count(*) FROM messages WHERE recipient IN (SELECT id FROM threads)",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn route(host: &mut Host, id: &str, from: Option<&str>) -> RouterRun {
        host.route_message(id, from, "remind me to book the dentist for Tuesday")
            .unwrap();
        let mut runs = host.take_router_runs();
        assert_eq!(runs.len(), 1);
        runs.remove(0)
    }

    #[test]
    fn a_sure_answer_opens_a_new_thread_titled_by_the_router() {
        let mut s = store();
        let run = route(&mut s.host, "m1", None);
        // The router sees the Spaces with their descriptions, and no thread yet.
        assert_eq!(run.input.spaces.len(), 2);
        assert!(run.input.current_thread.is_none() && run.input.open_threads.is_empty());
        assert_eq!(
            run.profile,
            s.host
                .resolve_job(&Job::Router, false, false)
                .unwrap()
                .profile
        );
        // Held, and nothing sent, while it decides.
        assert_eq!(s.host.held_messages().unwrap()[0].question, None);
        assert_eq!(thread_messages(&s.host), 0);

        let sure = answer(ThreadPick::New, &s.personal, 0.93);
        s.host.finish_router_run(run, Ok(sure)).unwrap();
        assert!(s.host.held_messages().unwrap().is_empty());
        let thread = s.host.threads().unwrap().remove(0);
        assert_eq!(
            (thread.title.as_str(), &thread.space_id),
            ("Dentist Tuesday", &s.personal)
        );
        // The thread takes the message's id, and from an empty tab the line with Change is in
        // the thread itself.
        assert_eq!(thread.id, "m1");
        assert_eq!(s.host.message("m1").unwrap().recipient, thread.id);
        assert_eq!(thread.routed[0].to, "m1");

        // Changed, the line moves with the message to its new thread.
        let to_sage = Destination::NewThread {
            space_id: s.sage.clone(),
        };
        s.host.change_route("m1", "m1", &to_sage).unwrap();
        let left = s.host.thread("m1").unwrap();
        assert!(left.routed.is_empty() && left.moved[0].message_id == "m1");
        let moved_to = s.host.thread(&left.moved[0].to).unwrap();
        assert_eq!(moved_to.routed[0].to, moved_to.id);
    }

    #[test]
    fn an_unsure_answer_holds_the_message_until_the_user_answers() {
        let mut s = store();
        let notes = s
            .host
            .start_thread("notes", &s.sage, "release notes", &[], None);
        let notes = notes.unwrap().id;
        let before = thread_messages(&s.host);
        let run = route(&mut s.host, "m1", Some(&notes));
        assert_eq!(run.input.current_thread.as_ref().unwrap().id, notes);
        let unsure = answer(ThreadPick::New, &s.personal, 0.62);
        s.host.finish_router_run(run, Ok(unsure)).unwrap();
        let held = s.host.held_messages().unwrap().remove(0);
        let question = held.question.clone().unwrap();
        assert!(
            question
                .why
                .starts_with("Not sure where this belongs (Personal 62%")
        );
        assert!(question.choices.contains(&Destination::Thread {
            thread_id: notes.clone()
        }));
        // Nothing reached a thread, or the Space's brain, before the click.
        assert_eq!(thread_messages(&s.host), before);
        assert_eq!(fs::read_dir(s.brain.path()).unwrap().count(), 0);

        let personal = Destination::NewThread {
            space_id: s.personal.clone(),
        };
        s.host.answer_held("m1", &personal).unwrap();
        assert!(s.host.held_messages().unwrap().is_empty());
        let sent = s.host.message("m1").unwrap();
        let line = s.host.thread(&notes).unwrap().routed.remove(0);
        assert_eq!(
            (line.to.as_str(), line.new),
            (sent.recipient.as_str(), true)
        );
        assert_eq!(line.choices, question.choices);
        // Answering again finds nothing to send.
        assert!(s.host.answer_held("m1", &personal).is_err());
    }

    #[test]
    fn a_spent_or_unusable_router_asks_without_running() {
        let mut s = store();
        let signed_out = ProviderStatus {
            installed: true,
            signed_in: false,
            plan: None,
        };
        for provider in PROVIDERS {
            s.host.record_detection(provider, &signed_out);
        }
        s.host.route_message("m1", None, "hello").unwrap();
        assert!(s.host.take_router_runs().is_empty());
        let question = s.host.held_messages().unwrap()[0].question.clone().unwrap();
        assert!(
            question.why.starts_with("The router did not run"),
            "{}",
            question.why
        );
        assert_eq!(question.choices.len(), 2);

        let signed_in = ProviderStatus {
            signed_in: true,
            ..signed_out
        };
        for provider in PROVIDERS {
            s.host.record_detection(provider, &signed_in);
            let window = QuotaWindow {
                key: "weekly".into(),
                label: "Weekly".into(),
                used_percent: Some(100.0),
                observed_at: now(),
                duration_mins: None,
                resets_at: Some(now() + 60 * 60 * 1000),
                status: None,
            };
            let reading = QuotaReading {
                provider,
                account: None,
                observed_at: now(),
                source: "test".into(),
                windows: vec![window],
                error: None,
            };
            s.host.record_quota(reading).unwrap();
        }
        s.host.route_message("m2", None, "hello").unwrap();
        assert!(s.host.take_router_runs().is_empty());
        let held = s.host.held_messages().unwrap();
        let question = held[1].question.as_ref().unwrap();
        assert_eq!(
            question.why,
            "The router did not run: every choice on its row is spent."
        );
    }

    #[test]
    fn a_held_message_survives_a_restart_and_is_asked_about() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_space("Sage", "Sage work", "", None).unwrap();
        host.route_message("m1", None, "hello").unwrap();
        assert_eq!(host.take_router_runs().len(), 1);
        drop(host);
        let host = Host::open_set_up(home.path()).unwrap();
        let held = host.held_messages().unwrap().remove(0);
        assert_eq!(held.body, "hello");
        let question = held.question.unwrap();
        assert_eq!(question.why, "The router stopped when Wiffletree quit.");
        assert_eq!(question.choices.len(), 2);
    }

    #[test]
    fn with_one_space_and_no_thread_no_router_runs() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.route_message("m1", None, "hello").unwrap();
        assert!(host.take_router_runs().is_empty() && host.held_messages().unwrap().is_empty());
        let thread = host.threads().unwrap().remove(0);
        assert_eq!(host.message("m1").unwrap().recipient, thread.id);
        // Sending the same message again changes nothing.
        host.route_message("m1", None, "hello").unwrap();
        assert_eq!(host.threads().unwrap().len(), 1);
    }

    #[test]
    fn change_moves_a_routed_message_and_marks_it_where_it_was() {
        let mut s = store();
        let notes = s
            .host
            .start_thread("notes", &s.sage, "release notes", &[], None);
        let notes = notes.unwrap().id;
        let run = route(&mut s.host, "m1", Some(&notes));
        let sure = answer(ThreadPick::New, &s.personal, 0.95);
        s.host.finish_router_run(run, Ok(sure)).unwrap();
        let dentist = s.host.message("m1").unwrap().recipient;
        assert_eq!(s.host.message("m1").unwrap().receipt, Receipt::Queued);

        let to_sage = Destination::NewThread {
            space_id: s.sage.clone(),
        };
        s.host.change_route(&notes, "m1", &to_sage).unwrap();
        // Not delivered yet, so it is cancelled where it was and marked moved.
        assert_eq!(s.host.message("m1").unwrap().receipt, Receipt::Cancelled);
        let left = s.host.thread(&dentist).unwrap();
        let moved = &left.moved[0];
        assert_eq!(moved.message_id, "m1");
        // The origin's line now names the new thread, which has the message as a new one.
        let line = s.host.thread(&notes).unwrap().routed.remove(0);
        assert_eq!(line.to, moved.to);
        assert!(line.new && line.message_id != "m1");
        assert_eq!(s.host.message(&line.message_id).unwrap().recipient, line.to);
        assert_eq!(s.host.thread(&line.to).unwrap().space_id, s.sage);
        // Changing it back to the thread it came from leaves no line.
        let back = Destination::Thread {
            thread_id: notes.clone(),
        };
        s.host
            .change_route(&notes, &line.message_id, &back)
            .unwrap();
        assert!(s.host.thread(&notes).unwrap().routed.is_empty());
        // The router no longer reads what was moved out of a thread.
        let run = route(&mut s.host, "m3", Some(&dentist));
        assert!(run.input.current_thread.unwrap().last_messages.is_empty());
    }

    #[test]
    fn the_threshold_is_a_shared_setting_from_half_to_one() {
        let mut s = store();
        assert_eq!(s.host.settings().router_threshold, 0.8);
        s.host.set_assistant_name("Dot").unwrap();
        s.host.set_router_threshold(0.7).unwrap();
        assert!(s.host.set_router_threshold(0.4).is_err());
        let settings = s.host.settings();
        assert_eq!(
            (settings.router_threshold, settings.assistant_name.as_str()),
            (0.7, "Dot")
        );
        let file = s.host.shared_folder().join(shared::ASSISTANT);
        let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        assert_eq!(saved, json!({"name": "Dot", "threshold": 0.7}));

        // The rules read it: 0.75 is enough at 0.7.
        let run = route(&mut s.host, "m1", None);
        let answer = answer(ThreadPick::New, &s.sage, 0.75);
        s.host.finish_router_run(run, Ok(answer)).unwrap();
        assert!(s.host.held_messages().unwrap().is_empty());
    }
}
