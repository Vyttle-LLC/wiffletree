//! Which files agents touched during a ticket, tallied from their stored read and edit steps,
//! and what Git says the ticket worktree changed. Git is only read, and never on the host
//! thread: the store part is gathered first and `with_git` finishes the reply elsewhere.
use crate::*;

/// Untracked files larger than this are listed as changed without a line count.
const MAX_COUNTED_FILE: u64 = 1024 * 1024;

/// Where Git reads a ticket's changes: its worktree, and the base it branched from.
pub(crate) struct Checkout {
    pub worktree: PathBuf,
    pub base: String,
}

impl Host {
    /// Every file the session's read and edit steps touched, across all of its stored runs,
    /// by path. Finished steps are counted and running ones marked; failed and interrupted
    /// steps count nothing.
    pub(crate) fn file_activity(&self, session_id: &str) -> Result<Vec<FileActivity>> {
        Ok(self
            .db
            .prepare(
                "WITH touched AS (
                    SELECT p.value AS path, json_extract(s.data,'$.kind')='file_edit' AS edit, json_extract(s.data,'$.state') AS state
                    FROM steps s, json_each(s.data,'$.paths') p
                    WHERE s.session_id=?1 AND json_extract(s.data,'$.kind') IN ('read','file_edit')
                 )
                 SELECT path,
                    COUNT(*) FILTER (WHERE NOT edit AND state='succeeded'),
                    COUNT(*) FILTER (WHERE edit AND state='succeeded'),
                    COUNT(*) FILTER (WHERE NOT edit AND state='running'),
                    COUNT(*) FILTER (WHERE edit AND state='running')
                 FROM touched WHERE state IN ('succeeded','running') GROUP BY path ORDER BY path",
            )?
            .query_map([session_id], |r| {
                Ok(FileActivity {
                    path: r.get(0)?,
                    reads: r.get(1)?,
                    edits: r.get(2)?,
                    reading: r.get::<_, u32>(3)? > 0,
                    writing: r.get::<_, u32>(4)? > 0,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// A ticket agent's own ticket and files, or every non-archived agent's on each open
    /// ticket a project coordinator owns. Read from the store only; Git fills in the rest.
    pub(crate) fn ticket_files(&self, session_id: &str) -> Result<Vec<(TicketFiles, Checkout)>> {
        let session = self.session(session_id)?;
        let runtimes = self.runtimes()?;
        let focus_of = |id: &str| {
            let runtime = runtimes.iter().find(|r| r.session_id == id);
            runtime.and_then(|r| r.focus.clone())
        };
        let (tickets, agents): (Vec<Ticket>, Vec<Session>) =
            if session.role == Role::ProjectOrchestrator {
                let tickets = self.tickets()?;
                let open = |t: &Ticket| t.is_open() && t.coordinator_id == session.id;
                let agents = self.sessions()?;
                (
                    tickets.into_iter().filter(open).collect(),
                    agents.into_iter().filter(|s| !s.archived).collect(),
                )
            } else {
                let ticket = self.session_runtime(session_id)?.ticket_id;
                (
                    ticket
                        .map(|id| self.ticket(&id))
                        .transpose()?
                        .into_iter()
                        .collect(),
                    vec![session],
                )
            };
        let mut report = vec![];
        for ticket in tickets {
            // A ticket stored before tickets recorded their repository has no base to compare.
            let Ok(repository) = self.repository(&ticket.repository_id) else {
                continue;
            };
            let on_ticket = |s: &&Session| {
                runtimes
                    .iter()
                    .any(|r| r.session_id == s.id && r.ticket_id.as_ref() == Some(&ticket.id))
            };
            let agents = agents
                .iter()
                .filter(on_ticket)
                .map(|agent| {
                    Ok(AgentFiles {
                        session_id: agent.id.clone(),
                        name: agent.role.agent_label(focus_of(&agent.id).as_deref()),
                        role: agent.role,
                        working: agent.status == Status::Working,
                        files: self.file_activity(&agent.id)?,
                    })
                })
                .collect::<Result<_>>()?;
            let checkout = Checkout {
                worktree: PathBuf::from(&ticket.worktree),
                base: repository.base,
            };
            report.push((
                TicketFiles {
                    ticket_id: ticket.id,
                    title: ticket.title,
                    repository_id: repository.id,
                    repository: repository.name,
                    agents,
                    changes: vec![],
                    tree: None,
                },
                checkout,
            ));
        }
        Ok(report)
    }
}

/// Adds each ticket's line counts, and its file listing when asked. A worktree that is
/// missing or that Git cannot compare keeps its ticket, without them.
pub(crate) fn with_git(report: Vec<(TicketFiles, Checkout)>, whole_tree: bool) -> Vec<TicketFiles> {
    report
        .into_iter()
        .map(|(mut ticket, checkout)| {
            if checkout.worktree.is_dir() {
                ticket.changes = line_counts(&checkout).unwrap_or_default();
                ticket.tree = whole_tree
                    .then(|| worktree_files(&checkout.worktree).ok())
                    .flatten();
            }
            ticket
        })
        .collect()
}

/// `+N −M` per file against the merge-base with the ticket base: committed and uncommitted
/// changes to tracked files, and new files Git does not track yet.
fn line_counts(checkout: &Checkout) -> Result<Vec<LineCount>> {
    let worktree = &checkout.worktree;
    let diff = runtime::git_output(
        worktree,
        &[
            "diff",
            "--numstat",
            "--no-renames",
            "-z",
            "--merge-base",
            &checkout.base,
        ],
    )?;
    // Each record is "added<TAB>removed<TAB>path"; a binary file has "-" for both counts.
    let mut counts: Vec<LineCount> = diff
        .split_terminator('\0')
        .filter_map(|record| {
            let mut fields = record.splitn(3, '\t');
            let mut count = || fields.next().map(|n| n.parse().unwrap_or(0));
            Some(LineCount {
                added: count()?,
                removed: count()?,
                path: fields.next()?.to_owned(),
            })
        })
        .collect();
    let untracked = runtime::git_output(
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    counts.extend(untracked.split_terminator('\0').map(|path| LineCount {
        path: path.to_owned(),
        added: new_file_lines(&worktree.join(path)),
        removed: 0,
    }));
    counts.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(counts)
}

/// A new text file's line count; 0 for one too large to read or not text.
fn new_file_lines(path: &Path) -> u32 {
    let small = fs::metadata(path).is_ok_and(|m| m.len() <= MAX_COUNTED_FILE);
    let text = small.then(|| fs::read_to_string(path).ok()).flatten();
    text.map_or(0, |text| text.lines().count() as u32)
}

/// The worktree's tracked and untracked-but-not-ignored files.
fn worktree_files(worktree: &Path) -> Result<Vec<String>> {
    let listing = runtime::git_output(
        worktree,
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    Ok(listing.split_terminator('\0').map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, args: &[&str]) {
        let identity = ["-c", "user.name=F", "-c", "user.email=f@example.invalid"];
        let status = std::process::Command::new("git")
            .current_dir(path)
            .args(identity)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A project whose repository tracks `notes.txt` on `main`, and its coordinator.
    fn project() -> (tempfile::TempDir, Host, Session, Repository) {
        let home = tempfile::tempdir().unwrap();
        let repository = home.path().join("web");
        fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-q", "-b", "main"]);
        fs::write(repository.join("notes.txt"), "one\ntwo\n").unwrap();
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "-q", "-m", "F"]);
        let mut host = Host::open(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Files").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let attached = host
            .attach_repository(&project.id, repository.to_str().unwrap(), "main")
            .unwrap();
        (home, host, coordinator, attached)
    }

    fn agent(host: &mut Host, ticket: &Ticket, role: Role) -> Session {
        host.assign_ticket(&ticket.id, role, Provider::Claude, "Go", None)
            .unwrap()
    }

    /// Stores a step of `run`, starting the run on first use.
    fn step(
        host: &Host,
        session: &Session,
        run: &str,
        kind: StepKind,
        state: StepState,
        path: &str,
    ) {
        host.db
            .execute(
                "INSERT OR IGNORE INTO provider_runs(id,session_id,messages,started_at,detail) VALUES (?1,?2,'[]',1,'{}')",
                params![run, session.id],
            )
            .unwrap();
        let revision = host.step_revision(&session.id).unwrap() + 1;
        let step = Step {
            run_id: run.into(),
            id: format!("{path}:{kind:?}"),
            parent_id: None,
            kind,
            state,
            title: path.into(),
            note: None,
            detail: None,
            paths: vec![path.into()],
            omitted: 0,
            seq: revision as u32,
            revision,
            started_at: 0,
            finished_at: None,
        };
        host.save_step(&session.id, &step).unwrap();
    }

    fn summary(files: &[FileActivity]) -> Vec<(&str, u32, u32, bool, bool)> {
        files
            .iter()
            .map(|f| (f.path.as_str(), f.reads, f.edits, f.reading, f.writing))
            .collect()
    }

    #[test]
    fn activity_spans_runs_and_follows_a_running_step_until_it_finishes() {
        use {StepKind::*, StepState::*};
        let (_home, mut host, coordinator, repository) = project();
        let ticket = host
            .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Do")
            .unwrap();
        let implementer = agent(&mut host, &ticket, Role::Implementer);
        step(&host, &implementer, "first", Read, Succeeded, "src/a.rs");
        step(&host, &implementer, "second", Read, Succeeded, "src/a.rs");
        step(
            &host,
            &implementer,
            "second",
            FileEdit,
            Succeeded,
            "src/a.rs",
        );
        step(&host, &implementer, "second", FileEdit, Running, "src/b.rs");
        step(&host, &implementer, "second", Read, Failed, "missing.rs");
        // A step stored before steps carried paths, and a command that names a file.
        host.db
            .execute(
                "INSERT INTO steps(session_id,run_id,id,revision,data) VALUES (?1,'first','old',9,?2)",
                params![
                    implementer.id,
                    r#"{"run_id":"first","id":"old","kind":"read","state":"succeeded","title":"src/old.rs","seq":9,"revision":9,"started_at":0,"finished_at":1}"#
                ],
            )
            .unwrap();
        step(
            &host,
            &implementer,
            "second",
            Command,
            Succeeded,
            "cat src/c.rs",
        );

        assert!(
            host.steps(&implementer.id, Some("first"), 0).unwrap().steps[1]
                .paths
                .is_empty()
        );
        assert_eq!(
            summary(&host.file_activity(&implementer.id).unwrap()),
            [
                ("src/a.rs", 2, 1, false, false),
                ("src/b.rs", 0, 0, false, true)
            ]
        );
        step(
            &host,
            &implementer,
            "second",
            FileEdit,
            Succeeded,
            "src/b.rs",
        );
        assert_eq!(
            summary(&host.file_activity(&implementer.id).unwrap())[1],
            ("src/b.rs", 0, 1, false, false)
        );
    }

    #[test]
    fn a_coordinator_sees_each_open_ticket_with_its_agents_and_an_agent_only_itself() {
        use {StepKind::*, StepState::*};
        let (_home, mut host, coordinator, repository) = project();
        let mut ticket = |title| {
            host.create_ticket(&coordinator.id, &repository.id, title, "Do")
                .unwrap()
        };
        let (panel, paths, done) = (ticket("Panel"), ticket("Paths"), ticket("Done"));
        let implementer = agent(&mut host, &panel, Role::Implementer);
        let reviewer = agent(&mut host, &panel, Role::Reviewer);
        let retired = agent(&mut host, &panel, Role::Tester);
        let other = agent(&mut host, &paths, Role::Implementer);
        let finished = agent(&mut host, &done, Role::Implementer);
        for agent in [&implementer, &reviewer, &retired, &other, &finished] {
            step(&host, agent, &agent.id, Read, Succeeded, "src/steps.rs");
        }
        step(
            &host,
            &implementer,
            &implementer.id,
            FileEdit,
            Succeeded,
            "src/ui.rs",
        );
        host.set_archived(&retired.id, true).unwrap();
        host.close_ticket(&done.id).unwrap();

        let rollup = with_git(host.ticket_files(&coordinator.id).unwrap(), false);
        let shape: Vec<_> = rollup
            .iter()
            .map(|t| {
                let agents = t.agents.iter().map(|a| {
                    let paths = a.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>();
                    (a.name.as_str(), paths)
                });
                (t.title.as_str(), agents.collect::<Vec<_>>())
            })
            .collect();
        assert_eq!(
            shape,
            [
                (
                    "Panel",
                    vec![
                        ("Implementer", vec!["src/steps.rs", "src/ui.rs"]),
                        ("Reviewer", vec!["src/steps.rs"]),
                    ]
                ),
                ("Paths", vec![("Implementer", vec!["src/steps.rs"])]),
            ]
        );
        assert_eq!(rollup[0].repository_id, rollup[1].repository_id);

        let own = with_git(host.ticket_files(&reviewer.id).unwrap(), false);
        assert_eq!((own.len(), own[0].agents.len()), (1, 1));
        assert_eq!(own[0].agents[0].session_id, reviewer.id);
    }

    #[test]
    fn line_counts_cover_commits_uncommitted_edits_and_new_files_and_survive_a_lost_worktree() {
        let (_home, mut host, coordinator, repository) = project();
        let mut ticket = |title| {
            host.create_ticket(&coordinator.id, &repository.id, title, "Do")
                .unwrap()
        };
        let (kept, lost) = (ticket("Kept"), ticket("Lost"));
        let worktree = Path::new(&kept.worktree);
        fs::write(worktree.join("notes.txt"), "one\nthree\n").unwrap();
        git(worktree, &["commit", "-q", "-am", "Edit"]);
        fs::write(worktree.join("notes.txt"), "one\nthree\nfour\n").unwrap();
        fs::write(worktree.join("new.txt"), "a\nb\nc\n").unwrap();
        fs::remove_dir_all(&lost.worktree).unwrap();

        let files = with_git(host.ticket_files(&coordinator.id).unwrap(), true);
        let counts = |t: &TicketFiles| {
            let count = |c: &LineCount| (c.path.clone(), c.added, c.removed);
            t.changes.iter().map(count).collect::<Vec<_>>()
        };
        assert_eq!(
            counts(&files[0]),
            [("new.txt".into(), 3, 0), ("notes.txt".into(), 2, 1)]
        );
        assert_eq!(files[0].tree.as_deref().unwrap(), ["new.txt", "notes.txt"]);
        assert_eq!(
            (files[1].title.as_str(), counts(&files[1]).len()),
            ("Lost", 0)
        );
        assert_eq!(files[1].tree, None);
    }
}
