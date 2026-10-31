//! Issue threads' proposals (OpenSpec change `assistant-spaces-jobs-boards`, stage 6). A
//! `propose_*` tool stores a change with its thread and shows it as a card; nothing reaches the
//! tracker until the human applies it, which runs the tracker's own write commands as them.
//! Every issue a proposal names is in the thread's own Space's tracker.
use crate::*;
use workspace_core::assistant::Thread;
use workspace_core::assistant::proposal::{
    Attached, ChangedSince, IssueChanges, Outcome, Proposal, ProposalChange, ProposalView,
};

/// The longest description or comment a proposal may hold, in bytes.
const MAX_TEXT: usize = 16 * 1024;
/// The most new issues one split may file.
const MAX_SPLIT: usize = 10;
/// The fields a proposed new issue may set.
const NEW_FIELDS: [&str; 7] = [
    "title",
    "description",
    "state",
    "priority",
    "assignee",
    "labels",
    "repositories",
];
/// What a tool result says about every proposal.
const SHOWN: &str =
    "Shown to the human as a card. Nothing reaches the tracker until they apply it.";

impl Host {
    /// The thread's Space and its tracker's prefix: the only tracker its tools reach.
    pub(crate) fn thread_tracker(&self, thread: &Thread) -> Result<(Space, String)> {
        let space = self.space(&thread.space_id)?.space;
        let prefix = (self.tracker(&space))
            .with_context(|| format!("{} has no issues", space.name))?
            .prefix;
        Ok((space, prefix))
    }
    /// A `propose_*` tool: stores the proposal with the thread and shows its card. Any thread
    /// may propose a new issue in its own Space; only an issue thread proposes changes to one.
    pub(crate) fn propose_change(
        &mut self,
        thread: &mut Thread,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        ensure!(
            thread.job.is_none() && thread.project_id.is_none(),
            "{name} is not one of this thread's tools"
        );
        let (_, prefix) = self.thread_tracker(thread)?;
        let issue_id = |key: &str| -> Result<IssueId> {
            let id = IssueId::parse(
                args[key]
                    .as_str()
                    .with_context(|| format!("Missing {key}"))?,
            )?;
            ensure!(
                id.prefix() == prefix,
                "{id} is not in your Space's tracker, whose issues are {prefix}-…"
            );
            Ok(id)
        };
        let change = match name {
            "propose_issue" => {
                service::only_arguments(name, args, &NEW_FIELDS)?;
                ProposalChange::NewIssue {
                    new: new_issue(args)?,
                }
            }
            _ if thread.attached.is_none() => bail!("{name} is an issue thread's tool"),
            "propose_edit" => {
                let mut allowed = vec!["id"];
                allowed.extend(&NEW_FIELDS);
                service::only_arguments(name, args, &allowed)?;
                let id = issue_id("id")?;
                let changes = changes(args)?;
                ensure!(!changes.is_empty(), "Name at least one field to change");
                let base = Box::new(self.parsed_issue(&id)?);
                ProposalChange::Edit { id, base, changes }
            }
            "propose_comment" => {
                service::only_arguments(name, args, &["id", "body"])?;
                let id = issue_id("id")?;
                let body = text(args, "body")?.context("Missing body")?;
                ensure!(!body.trim().is_empty(), "A comment needs text");
                self.parsed_issue(&id)?;
                ProposalChange::Comment { id, body }
            }
            "propose_split" => {
                service::only_arguments(name, args, &["id", "new", "changes"])?;
                let id = issue_id("id")?;
                let new = (args["new"].as_array())
                    .context("new is a list of the issues to file")?
                    .iter()
                    .map(new_issue)
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    (1..=MAX_SPLIT).contains(&new.len()),
                    "A split files from 1 to {MAX_SPLIT} new issues"
                );
                let changes = match args.get("changes") {
                    Some(changes) => {
                        let allowed: Vec<&str> = NEW_FIELDS.to_vec();
                        service::only_arguments("changes", changes, &allowed)?;
                        self::changes(changes)?
                    }
                    None => IssueChanges::default(),
                };
                let base = Box::new(self.parsed_issue(&id)?);
                ProposalChange::Split {
                    id,
                    base,
                    changes,
                    new,
                }
            }
            "propose_duplicate" => {
                service::only_arguments(name, args, &["id", "of"])?;
                let (id, of) = (issue_id("id")?, issue_id("of")?);
                ensure!(id != of, "An issue is not a duplicate of itself");
                self.parsed_issue(&of)?;
                let base = Box::new(self.parsed_issue(&id)?);
                ProposalChange::Duplicate { id, of, base }
            }
            other => bail!("{other} is not one of a thread's tools"),
        };
        let proposal = Proposal {
            id: new_id(),
            created_at: now(),
            change,
            outcome: None,
            filed: vec![],
        };
        let result = json!({"proposal": proposal.id, "note": SHOWN});
        thread.proposals.push(proposal);
        self.save_thread(thread)?;
        Ok(result)
    }

    /// A temporary issue the tracker numbered, in every thread that names it: the issue it is
    /// attached to, its title and its proposals, so it stays the one thread for that issue and
    /// its proposals still apply.
    pub(crate) fn renumber_threads(&self, temporary: &IssueId, number: &IssueId) -> Result<()> {
        for mut thread in self.threads()? {
            let before = thread.clone();
            if let Some(Attached::Issue { id }) = &mut thread.attached
                && id == temporary
            {
                *id = number.clone();
            }
            if let Some(rest) = thread.title.strip_prefix(&temporary.to_string()) {
                thread.title = format!("{number}{rest}");
            }
            for proposal in &mut thread.proposals {
                proposal.change.renumber(temporary, number);
                let issues = match &mut proposal.outcome {
                    Some(Outcome::Applied { issues, .. }) => Some(issues),
                    _ => None,
                };
                for id in (proposal.filed.iter_mut()).chain(issues.into_iter().flatten()) {
                    if id == temporary {
                        *id = number.clone();
                    }
                }
            }
            if thread != before {
                self.save_thread(&thread)?;
            }
        }
        Ok(())
    }
    /// The thread's pending proposals as their cards show them now: each issue as it stands,
    /// and who changed it since the proposal read it.
    pub fn proposal_views(&self, thread_id: &str) -> Result<Vec<ProposalView>> {
        let thread = self.thread(thread_id)?;
        let pending = (thread.proposals.iter()).filter(|p| p.outcome.is_none());
        Ok(pending
            .map(|proposal| {
                let id = proposal.change.issue();
                let current = id.and_then(|id| self.read_issue(id).ok()?.issue);
                let changed_since = match (proposal.change.base(), &current, id) {
                    (Some(base), Some(now), Some(id)) if base != now => Some(self.last_change(id)),
                    _ => None,
                };
                ProposalView {
                    proposal_id: proposal.id.clone(),
                    current,
                    changed_since,
                }
            })
            .collect())
    }
    /// Who last committed the issue's `issue.md` and when, as its tracker's history says.
    fn last_change(&self, id: &IssueId) -> ChangedSince {
        let named = (|| -> Option<(String, String)> {
            let (space, _) = self.tracker_for(id.prefix()).ok()?;
            let folder = self.storage_folder(&space.storage.repository);
            let file = format!(":(literal){}", tracker::issue_file(id));
            let args = ["log", "-1", "--format=%an%x00%aI", "--", file.as_str()];
            let line = runtime::git_output(&folder, &args).ok()?;
            let (by, at) = line.trim_end().split_once('\0')?;
            Some((by.to_owned(), at.to_owned()))
        })();
        ChangedSince {
            by: named.as_ref().map(|(by, _)| by.clone()),
            at: named.map(|(_, at)| at),
        }
    }

    /// Applies a pending proposal as the human, or with `edited` their edit of it, which keeps
    /// its kind and issue: the tracker's own write commands run as them, a comment and a filed
    /// issue marked as their assistant's. A change to an issue is made on its current version.
    pub fn apply_proposal(
        &mut self,
        thread_id: &str,
        proposal_id: &str,
        edited: Option<ProposalChange>,
    ) -> Result<Thread> {
        let thread = self.thread(thread_id)?;
        let (space, prefix) = self.thread_tracker(&thread)?;
        let stored = pending(&thread, proposal_id)?.change.clone();
        let change = match edited {
            Some(edited) => {
                ensure!(
                    stored.same_kind(&edited),
                    "An edited proposal keeps its kind and its issue"
                );
                with_base(edited, &stored)
            }
            None => stored,
        };
        let named = change.issue().into_iter().chain(match &change {
            ProposalChange::Duplicate { of, .. } => Some(of),
            _ => None,
        });
        for id in named {
            ensure!(
                id.prefix() == prefix,
                "{id} is not in {}'s tracker",
                space.name
            );
        }
        let issues = match &change {
            ProposalChange::Edit { id, changes, .. } => {
                let current = self.parsed_issue(id)?;
                self.write_change(&current, changes.apply(&current))?;
                vec![id.clone()]
            }
            ProposalChange::Comment { id, body } => {
                self.post_comment(id, body, true)?;
                vec![id.clone()]
            }
            ProposalChange::Split {
                id, changes, new, ..
            } => {
                for issue in new {
                    tracker::validate_title(&issue.title)?;
                }
                // Each issue filed is kept as it is filed, so trying again after a failure
                // files none of them twice.
                let mut filed = pending(&thread, proposal_id)?.filed.clone();
                for issue in new.iter().skip(filed.len()) {
                    filed.push(self.file_issue(&space.id, issue.clone(), true)?.id);
                    let mut kept = self.thread(thread_id)?;
                    if let Some(proposal) = kept.proposals.iter_mut().find(|p| p.id == proposal_id)
                    {
                        proposal.filed = filed.clone();
                    }
                    self.save_thread(&kept)?;
                }
                let current = self.parsed_issue(id)?;
                let mut after = changes.apply(&current);
                after.related.extend(filed.iter().map(IssueId::to_string));
                self.write_change(&current, after)?;
                std::iter::once(id.clone()).chain(filed).collect()
            }
            ProposalChange::Duplicate { id, .. } => {
                let current = self.parsed_issue(id)?;
                let after = change
                    .applied_to(&current)
                    .context("A duplicate edits its issue")?;
                self.write_change(&current, after)?;
                vec![id.clone()]
            }
            ProposalChange::NewIssue { new } => {
                vec![self.file_issue(&space.id, new.clone(), true)?.id]
            }
        };
        let mut thread = self.thread(thread_id)?;
        let at = now();
        if let Some(proposal) = thread.proposals.iter_mut().find(|p| p.id == proposal_id) {
            proposal.change = change;
            proposal.outcome = Some(Outcome::Applied { at, issues });
        }
        self.save_thread(&thread)?;
        Ok(thread)
    }
    /// Discards a pending proposal; nothing is written to the tracker.
    pub fn discard_proposal(&mut self, thread_id: &str, proposal_id: &str) -> Result<Thread> {
        let mut thread = self.thread(thread_id)?;
        pending(&thread, proposal_id)?;
        if let Some(proposal) = thread.proposals.iter_mut().find(|p| p.id == proposal_id) {
            proposal.outcome = Some(Outcome::Discarded { at: now() });
        }
        self.save_thread(&thread)?;
        Ok(thread)
    }
    /// Writes `after` over `current` with the human's edit, then its move when the state
    /// changes, as the issue page would.
    fn write_change(&mut self, current: &Issue, after: Issue) -> Result<()> {
        let state = after.state;
        let edited = Issue {
            state: current.state,
            ..after
        };
        if edited != *current {
            self.edit_issue(edited)?;
        }
        if state != current.state {
            self.move_issue(&current.id, state)?;
        }
        Ok(())
    }
}

/// The proposal `id` of `thread`, while the human has neither applied nor discarded it.
fn pending<'a>(thread: &'a Thread, id: &str) -> Result<&'a Proposal> {
    let proposal = (thread.proposals.iter())
        .find(|p| p.id == id)
        .context("Proposal not found")?;
    ensure!(
        proposal.outcome.is_none(),
        "This proposal was already applied or discarded"
    );
    Ok(proposal)
}

/// The human's edit of `stored`, keeping the issue as the proposal first read it, which tells
/// whether it changed since.
fn with_base(edited: ProposalChange, stored: &ProposalChange) -> ProposalChange {
    let Some(base) = stored.base().cloned().map(Box::new) else {
        return edited;
    };
    match edited {
        ProposalChange::Edit { id, changes, .. } => ProposalChange::Edit { id, base, changes },
        ProposalChange::Split {
            id, changes, new, ..
        } => ProposalChange::Split {
            id,
            base,
            changes,
            new,
        },
        ProposalChange::Duplicate { id, of, .. } => ProposalChange::Duplicate { id, of, base },
        other => other,
    }
}

/// A bounded text argument, if given.
fn text(args: &Value, key: &str) -> Result<Option<String>> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(text) => {
            ensure!(text.len() <= MAX_TEXT, "{key} is at most 16 KB");
            Ok(Some(text.clone()))
        }
        _ => bail!("{key} is text"),
    }
}

/// The issue a proposal would file.
fn new_issue(args: &Value) -> Result<NewIssue> {
    let fields = (args.as_object())
        .context("An issue to file is an object of its fields")?
        .iter()
        .filter(|(key, _)| NEW_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    text(args, "description")?;
    let new: NewIssue = serde_json::from_value(Value::Object(fields))
        .context("An issue to file has a title and optionally description, state, priority, assignee, labels and repositories")?;
    tracker::validate_title(&new.title)?;
    Ok(new)
}

/// The fields a proposal changes.
fn changes(args: &Value) -> Result<IssueChanges> {
    let mut fields = serde_json::Map::new();
    for key in NEW_FIELDS {
        if let Some(value) = args.get(key).filter(|value| !value.is_null()) {
            fields.insert(key.into(), value.clone());
        }
    }
    text(args, "description")?;
    let changes: IssueChanges = serde_json::from_value(Value::Object(fields))
        .context("state is one of triage, backlog, todo, in_progress, in_review, done or canceled; priority one of urgent, high, medium, low or none; labels and repositories are lists")?;
    if let Some(title) = &changes.title {
        tracker::validate_title(title)?;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue_work::tests::clone_here;
    use crate::storage::tests::{sage, shared_storage, space_named};
    use crate::sync::tests::NAME;
    use crate::test_support::git;
    use workspace_core::assistant::proposal::{Attached, FILED_FOR, filed_for};

    /// This Mac with a Sage Space on a local tracker, SAGE, signed in to gh as `jlin`, and
    /// SAGE-1 filed in it by a teammate's hand.
    fn sage_here() -> (tempfile::TempDir, Host, Space, Issue) {
        let (home, mut host) = crate::storage::tests::host();
        let sage = (host.create_space("Sage", "", "", Some("SAGE")).unwrap()).space;
        host.record_gh_login(Some("jlin".into()));
        let new = NewIssue {
            title: "Delivery webhook retries forever".into(),
            description: "Since Monday it retries.".into(),
            labels: vec!["bug".into()],
            ..NewIssue::default()
        };
        let issue = host.create_issue(&sage.id, new).unwrap();
        (home, host, sage, issue)
    }
    fn issue_thread(host: &mut Host, issue: &Issue) -> (Thread, Session) {
        let attached = Attached::Issue {
            id: issue.id.clone(),
        };
        let thread = host.open_issue_thread(&attached, None).unwrap();
        let session = host.session(&thread.id).unwrap();
        (thread, session)
    }
    fn call(host: &mut Host, session: &Session, tool: &str, args: Value) -> Result<Value> {
        host.thread_tool(session, tool, &args)
    }
    fn pending_id(host: &Host, thread: &str) -> String {
        let thread = host.thread(thread).unwrap();
        let pending = thread.proposals.iter().rev().find(|p| p.outcome.is_none());
        pending.unwrap().id.clone()
    }

    #[test]
    fn an_issue_thread_opens_once_per_space_and_a_shared_tracker_has_one_in_each() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let watch = (sage("Watch app"))
            .replace("sage-id", "watch-id")
            .replace("name: Sage", "name: Sage Watch")
            .replace("folder: sage", "folder: watch");
        github.commit("spaces/watch/space.yaml", watch);
        let (_home, mut host) = crate::storage::tests::host();
        let staged = storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let ticked = ["sage", "watch"].map(|folder| JoinSpace {
            folder: folder.into(),
            brain_path: None,
            clones: vec![],
        });
        host.join_spaces(staged, &ticked).unwrap();
        let (sage, watch) = (space_named(&host, "Sage"), space_named(&host, "Sage Watch"));
        host.record_gh_login(Some("jlin".into()));
        let new = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        let issue = host.create_issue(&sage.id, new).unwrap();
        let attached = Attached::Issue {
            id: issue.id.clone(),
        };
        let in_sage = host.open_issue_thread(&attached, Some(&sage.id)).unwrap();
        assert_eq!(in_sage.space_id, sage.id);
        assert!(in_sage.title.starts_with(&issue.id.to_string()));
        let sent = host.messages(&in_sage.id, None, 10).unwrap();
        assert!(sent.is_empty(), "it waits for the human");
        host.set_thread_open(&in_sage.id, false).unwrap();
        let again = host.open_issue_thread(&attached, Some(&sage.id)).unwrap();
        assert_eq!(again.id, in_sage.id, "resumed in its own Space");
        assert!(!again.closed);
        let in_watch = host.open_issue_thread(&attached, Some(&watch.id)).unwrap();
        assert_eq!(in_watch.space_id, watch.id);
        assert_ne!(in_watch.id, in_sage.id, "a separate thread in Sage Watch");
        let list = host
            .open_issue_thread(&Attached::Issues, Some(&watch.id))
            .unwrap();
        assert_eq!(
            (list.space_id.clone(), list.title.as_str()),
            (watch.id.clone(), "Issues · Sage Watch")
        );
        let list_again = host
            .open_issue_thread(&Attached::Issues, Some(&watch.id))
            .unwrap();
        assert_eq!(list_again.id, list.id);
    }

    #[test]
    fn an_issue_thread_reads_only_its_spaces_tracker_and_has_no_web_source_or_coordination() {
        let (_home, mut host, sage, issue) = sage_here();
        let wood = (host
            .create_space("Woodworking", "", "", Some("WOOD"))
            .unwrap())
        .space;
        let theirs = host
            .create_issue(
                &wood.id,
                NewIssue {
                    title: "Router table".into(),
                    ..NewIssue::default()
                },
            )
            .unwrap();
        let (thread, session) = issue_thread(&mut host, &issue);
        assert_eq!(thread.space_id, sage.id);
        let listed = call(&mut host, &session, "list_issues", json!({})).unwrap();
        let ids: Vec<&str> = (listed["issues"].as_array().unwrap().iter())
            .map(|issue| issue["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, [issue.id.to_string()]);
        let read = call(&mut host, &session, "read_issue", json!({"id": issue.id})).unwrap();
        assert_eq!(read["description"]["written_by"], "jlin");
        for (tool, args) in [
            ("read_issue", json!({"id": theirs.id})),
            ("propose_comment", json!({"id": theirs.id, "body": "x"})),
        ] {
            let refused = call(&mut host, &session, tool, args);
            assert!(
                format!("{:#}", refused.unwrap_err()).contains("tracker"),
                "{tool}"
            );
        }
        for tool in ["link_issue", "create_ticket", "assign_ticket", "WebFetch"] {
            let refused = call(&mut host, &session, tool, json!({"id": issue.id}));
            let error = format!("{:#}", refused.unwrap_err());
            assert!(
                error.contains("not one of a thread's tools"),
                "{tool}: {error}"
            );
        }
        assert!(host.read_issue(&issue.id).unwrap().links.is_empty());
        let launch = host.thread_launch(&session).unwrap();
        assert!(launch.issue_thread && launch.sources.is_empty());
        // Its turns name the attached issue, by ID only.
        let told = host.thread_instructions(&session).unwrap();
        assert!(
            told.contains(&format!("opened this thread on {}", issue.id)),
            "{told}"
        );
        assert!(!told.contains(&issue.title));
        // A thread with nothing attached may propose an issue, and nothing else of these.
        let plain = host
            .start_thread("plain", &sage.id, "Hi", &[], None)
            .unwrap();
        let plain = host.session(&plain.id).unwrap();
        for tool in ["list_issues", "propose_edit", "search_repositories"] {
            let args = json!({"id": issue.id, "title": "x", "query": "x"});
            assert!(call(&mut host, &plain, tool, args).is_err(), "{tool}");
        }
        call(
            &mut host,
            &plain,
            "propose_issue",
            json!({"title": "Track the retry"}),
        )
        .unwrap();
        assert!(!host.thread_launch(&plain).unwrap().issue_thread);
    }

    #[test]
    fn proposals_change_nothing_until_the_human_applies_them_as_themselves() {
        let (_home, mut host, sage, issue) = sage_here();
        let (thread, session) = issue_thread(&mut host, &issue);
        let before = host.read_issue(&issue.id).unwrap();
        let other = host
            .create_issue(
                &sage.id,
                NewIssue {
                    title: "Retries on the watch".into(),
                    ..NewIssue::default()
                },
            )
            .unwrap();
        for (tool, args) in [
            (
                "propose_comment",
                json!({"id": issue.id, "body": "Reject it with 400."}),
            ),
            (
                "propose_edit",
                json!({"id": issue.id, "labels": ["bug", "regression"], "description": "Since Monday it retries.\n## Acceptance criteria"}),
            ),
            ("propose_duplicate", json!({"id": other.id, "of": issue.id})),
            (
                "propose_split",
                json!({"id": issue.id, "new": [{"title": "Reject a bad payload"}, {"title": "Log by event ID"}]}),
            ),
            (
                "propose_issue",
                json!({"title": "Crash log export fails", "labels": ["watch"]}),
            ),
        ] {
            let shown = call(&mut host, &session, tool, args).unwrap();
            assert!(
                shown["note"]
                    .as_str()
                    .unwrap()
                    .contains("Nothing reaches the tracker")
            );
        }
        let unchanged = host.read_issue(&issue.id).unwrap();
        assert_eq!(unchanged.issue, before.issue);
        assert!(unchanged.comments.is_empty());
        assert_eq!(host.list_issues(Some(&sage.id), None).unwrap().len(), 2);
        let proposals = host.thread(&thread.id).unwrap().proposals;
        assert_eq!(proposals.len(), 5);
        let id = |at: usize| proposals[at].id.clone();

        host.apply_proposal(&thread.id, &id(0), None).unwrap();
        let comment = host.read_issue(&issue.id).unwrap().comments.remove(0);
        assert_eq!(
            (comment.author.as_str(), comment.assistant_for.as_deref()),
            ("jlin", Some("jlin"))
        );
        assert!(
            host.apply_proposal(&thread.id, &id(0), None).is_err(),
            "applied once"
        );

        host.discard_proposal(&thread.id, &id(1)).unwrap();
        assert_eq!(
            host.read_issue(&issue.id).unwrap().issue.unwrap().labels,
            ["bug"]
        );
        assert!(host.apply_proposal(&thread.id, &id(1), None).is_err());

        host.apply_proposal(&thread.id, &id(2), None).unwrap();
        let duplicate = host.read_issue(&other.id).unwrap().issue.unwrap();
        assert_eq!(duplicate.duplicate_of, Some(issue.id.to_string()));
        assert_eq!(duplicate.state, other.state, "a duplicate only links");

        let split = host.apply_proposal(&thread.id, &id(3), None).unwrap();
        let Some(Outcome::Applied { issues, .. }) = &split.proposals[3].outcome else {
            panic!("applied");
        };
        assert_eq!(issues.len(), 3);
        let original = host.read_issue(&issue.id).unwrap().issue.unwrap();
        assert_eq!(
            original.related,
            [issues[1].to_string(), issues[2].to_string()]
        );
        let filed = host.read_issue(&issues[1]).unwrap().issue.unwrap();
        assert_eq!(filed_for(&filed), Some("jlin"));

        host.apply_proposal(&thread.id, &id(4), None).unwrap();
        let listed = host.list_issues(Some(&sage.id), Some("Crash log")).unwrap();
        let filed = host.read_issue(&listed[0].id).unwrap().issue.unwrap();
        assert_eq!(
            (filed.author.as_str(), filed_for(&filed)),
            ("jlin", Some("jlin"))
        );
        assert_eq!(filed.extra[FILED_FOR], "jlin");
    }

    #[test]
    fn a_numbered_issue_keeps_its_one_thread_and_its_pending_proposals() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (_home, mut host) = crate::storage::tests::host();
        let staged = storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
        let sage = JoinSpace {
            folder: "sage".into(),
            brain_path: None,
            clones: vec![],
        };
        host.join_spaces(staged, &[sage]).unwrap();
        let sage = space_named(&host, "Sage");
        host.record_gh_login(Some("jlin".into()));
        let new = NewIssue {
            title: "Watch app drops replies".into(),
            ..NewIssue::default()
        };
        let issue = host.create_issue(&sage.id, new).unwrap();
        assert!(issue.id.is_temporary());
        let (thread, session) = issue_thread(&mut host, &issue);
        let args = json!({"id": issue.id, "body": "Reject it."});
        call(&mut host, &session, "propose_comment", args).unwrap();
        host.sync_issues(None, &github.tools()).unwrap();
        let numbered = IssueId::parse("SAGE-1").unwrap();
        let renamed = host.thread(&thread.id).unwrap();
        let attached = Attached::Issue {
            id: numbered.clone(),
        };
        assert_eq!(renamed.attached.as_ref(), Some(&attached));
        assert!(renamed.title.starts_with("SAGE-1 · "), "{}", renamed.title);
        let again = host.open_issue_thread(&attached, None).unwrap();
        assert_eq!(again.id, thread.id, "still the one thread for the issue");
        let proposal = pending_id(&host, &thread.id);
        host.apply_proposal(&thread.id, &proposal, None).unwrap();
        assert_eq!(host.read_issue(&numbered).unwrap().comments.len(), 1);
    }

    #[test]
    fn a_split_files_nothing_on_a_bad_title_and_never_twice_after_a_failed_link() {
        let (_home, mut host, sage, issue) = sage_here();
        let (thread, session) = issue_thread(&mut host, &issue);
        let args = json!({"id": issue.id, "new": [{"title": "Reject a bad payload"}, {"title": "Log by event ID"}]});
        call(&mut host, &session, "propose_split", args).unwrap();
        let proposal = pending_id(&host, &thread.id);
        let count = |host: &Host| host.list_issues(Some(&sage.id), None).unwrap().len();
        let ProposalChange::Split {
            id,
            base,
            changes,
            mut new,
        } = host.thread(&thread.id).unwrap().proposals[0].change.clone()
        else {
            panic!("a split");
        };
        new[1].title = "x".repeat(600);
        let edited = ProposalChange::Split {
            id,
            base,
            changes,
            new,
        };
        assert!(
            host.apply_proposal(&thread.id, &proposal, Some(edited))
                .is_err()
        );
        assert_eq!(count(&host), 1, "no new issue was filed");
        // The original can't be read meanwhile, so its link fails after the filings.
        let file = host
            .storage_folder(&sage.storage.repository)
            .join(tracker::issue_file(&issue.id));
        let written = fs::read_to_string(&file).unwrap();
        fs::write(&file, "Edited by hand, with no front matter.\n").unwrap();
        assert!(host.apply_proposal(&thread.id, &proposal, None).is_err());
        assert_eq!(count(&host), 3);
        assert_eq!(host.thread(&thread.id).unwrap().proposals[0].filed.len(), 2);
        fs::write(&file, written).unwrap();
        host.apply_proposal(&thread.id, &proposal, None).unwrap();
        assert_eq!(count(&host), 3, "trying again files neither twice");
        let original = host.read_issue(&issue.id).unwrap().issue.unwrap();
        assert_eq!(original.related.len(), 2);
    }

    #[test]
    fn a_proposal_whose_issue_changed_shows_it_and_applies_on_the_new_version() {
        let (_home, mut host, _sage, issue) = sage_here();
        let (thread, session) = issue_thread(&mut host, &issue);
        let args = json!({"id": issue.id, "description": "Since Monday it retries.\n## Acceptance criteria"});
        call(&mut host, &session, "propose_edit", args).unwrap();
        let proposal = pending_id(&host, &thread.id);
        let views = host.proposal_views(&thread.id).unwrap();
        assert_eq!(views[0].changed_since, None);
        // A teammate raises the priority meanwhile.
        let mut theirs = host.read_issue(&issue.id).unwrap().issue.unwrap();
        theirs.priority = Priority::Urgent;
        host.edit_issue(theirs).unwrap();
        let views = host.proposal_views(&thread.id).unwrap();
        assert!(views[0].changed_since.as_ref().unwrap().by.is_some());
        assert_eq!(
            views[0].current.as_ref().unwrap().priority,
            Priority::Urgent
        );
        // The human's edit keeps the kind and the issue.
        let refused = ProposalChange::Comment {
            id: issue.id.clone(),
            body: "x".into(),
        };
        assert!(
            host.apply_proposal(&thread.id, &proposal, Some(refused))
                .is_err()
        );
        let ProposalChange::Edit {
            id,
            base,
            mut changes,
        } = host.thread(&thread.id).unwrap().proposals[0].change.clone()
        else {
            panic!("an edit");
        };
        changes.description = Some("Since Monday it retries.\n## Done when".into());
        let edited = ProposalChange::Edit { id, base, changes };
        host.apply_proposal(&thread.id, &proposal, Some(edited))
            .unwrap();
        let after = host.read_issue(&issue.id).unwrap().issue.unwrap();
        assert_eq!(
            after.priority,
            Priority::Urgent,
            "the teammate's change stays"
        );
        assert!(after.description.ends_with("## Done when"));
    }

    #[test]
    fn repository_reads_see_only_the_base_branch_of_the_spaces_repositories() {
        let (home, mut host, sage, issue) = sage_here();
        let folder = home.path().join("watch-app");
        let repository = clone_here(&mut host, &folder, "Sage-Social/watch-app");
        host.set_repository_spaces("Sage-Social/watch-app", std::slice::from_ref(&sage.id))
            .unwrap();
        let elsewhere = home.path().join("other");
        clone_here(&mut host, &elsewhere, "Sage-Social/other");
        fs::write(elsewhere.join("other.rs"), "fn delivery() {}\n").unwrap();
        git(&elsewhere, &["add", "."]);
        git(&elsewhere, &["commit", "-q", "-m", "Other"]);
        let lines: String = (0..300).map(|n| format!("delivery {n}\n")).collect();
        fs::write(folder.join("delivery.rs"), &lines).unwrap();
        git(&folder, &["add", "."]);
        git(&folder, &["commit", "-q", "-m", "Delivery"]);
        // The base as an import records it: origin's branch, as last fetched.
        git(&folder, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        host.set_repository_base(&repository.id, "origin/main")
            .unwrap();
        // Neither another branch nor the worktree is read.
        git(&folder, &["checkout", "-q", "-b", "feature"]);
        fs::write(folder.join("feature.rs"), "delivery on a branch\n").unwrap();
        git(&folder, &["add", "."]);
        git(&folder, &["commit", "-q", "-m", "Feature"]);
        fs::write(folder.join("delivery.rs"), "delivery in the worktree\n").unwrap();
        let (_, session) = issue_thread(&mut host, &issue);
        let search = |host: &Host, args: Value| {
            host.repository_read(&session, "search_repositories", &args)?
                .run()
        };
        let found = search(&host, json!({"query": "DELIVERY"})).unwrap();
        let matches = found["matches"].as_array().unwrap();
        assert!(found["cut_short"].as_bool().unwrap());
        assert_eq!(matches.len(), 20, "at most 20 lines of one file");
        assert!(
            matches
                .iter()
                .all(|m| m["repository"] == "Sage-Social/watch-app" && m["path"] == "delivery.rs")
        );
        let read = host
            .repository_read(
                &session,
                "read_repository_file",
                &json!({"repository": "watch-app", "path": "delivery.rs"}),
            )
            .unwrap()
            .run()
            .unwrap();
        assert!(read["text"].as_str().unwrap().starts_with("delivery 0\n"));
        for path in ["feature.rs", "../x", "/etc/hosts"] {
            let args = json!({"repository": "watch-app", "path": path});
            let refused = host.repository_read(&session, "read_repository_file", &args);
            assert!(refused.and_then(|read| read.run()).is_err(), "{path}");
        }
        let other = json!({"repository": "Sage-Social/other", "path": "other.rs"});
        assert!(
            host.repository_read(&session, "read_repository_file", &other)
                .is_err()
        );
        assert_eq!(
            git(&folder, &["branch", "--show-current"]).trim(),
            "feature"
        );
    }
}
