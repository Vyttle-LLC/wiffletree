//! What the desktop's Me lens needs from every ticket of a project, derived on each snapshot read
//! so the desktop never reads the archive for it.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

/// One ticket, as far as the Me lens goes.
struct TicketFacts {
    id: String,
    state: TicketState,
    repository_id: String,
    pull_request: Option<(PrState, u64, String)>,
    passed: bool,
}

impl Host {
    /// By project id, the Me facts of each unarchived project in `projects` that has any.
    pub(crate) fn me_facts(&self, projects: &[Project]) -> Result<BTreeMap<String, MeFacts>> {
        let tickets = self.tickets_by_project()?;
        let archived = self.archived_projects()?;
        let last_turns = self.projects_last_turn()?;
        let repositories = self.repositories()?;
        let protects = |repository_id: &str, base: &str| {
            (repositories.iter())
                .find(|repository| repository.id == repository_id)
                .is_some_and(|repository| ProtectedBranches::of(repository).protects(base))
        };
        let open_child = |parent: &str| {
            (projects.iter()).any(|child| {
                child.parent_id.as_deref() == Some(parent) && !archived.contains(&child.id)
            })
        };
        let mut facts = BTreeMap::new();
        for project in projects.iter().filter(|p| !archived.contains(&p.id)) {
            let own = tickets.get(&project.id).map_or(&[][..], Vec::as_slice);
            let settled = |ticket: &TicketFacts| {
                matches!(ticket.state, TicketState::Accepted | TicketState::Closed)
                    && (ticket.pull_request.as_ref())
                        .is_none_or(|(state, ..)| *state == PrState::Merged)
            };
            let ready_to_close = !project.standing
                && !own.is_empty()
                && own.iter().all(settled)
                && !open_child(&project.id);
            let merges = (own.iter())
                .filter_map(|ticket| {
                    let (state, number, base) = ticket.pull_request.as_ref()?;
                    let human = *state == PrState::Open
                        && ticket.passed
                        && protects(&ticket.repository_id, base);
                    human.then(|| HumanMerge {
                        ticket_id: ticket.id.clone(),
                        number: *number,
                        base: base.clone(),
                    })
                })
                .collect();
            let project_facts = MeFacts {
                ready_to_close,
                merges,
                last_turn_started: last_turns.get(&project.id).copied(),
            };
            if project_facts != MeFacts::default() {
                facts.insert(project.id.clone(), project_facts);
            }
        }
        Ok(facts)
    }

    /// Every ticket's Me facts, by its coordinator's project, in the order they were created.
    fn tickets_by_project(&self) -> Result<BTreeMap<String, Vec<TicketFacts>>> {
        let mut rows = self.db.prepare(
            "SELECT s.project_id, t.id, json_extract(t.data,'$.state'),
                coalesce(json_extract(t.data,'$.repository_id'),''),
                json_extract(t.data,'$.pull_request.state'),
                json_extract(t.data,'$.pull_request.number'),
                json_extract(t.data,'$.pull_request.base'),
                json_extract(t.data,'$.verification.outcome')
             FROM tickets t JOIN sessions s ON s.id=t.coordinator_id
             WHERE s.project_id IS NOT NULL ORDER BY t.rowid",
        )?;
        let read = rows.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<u64>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut tickets: BTreeMap<String, Vec<TicketFacts>> = BTreeMap::new();
        for row in read {
            let (project, id, state, repository_id, pr_state, number, base, outcome) = row?;
            let pull_request = match (pr_state, number, base) {
                (Some(state), Some(number), Some(base)) => Some((named(&state)?, number, base)),
                _ => None,
            };
            let passed =
                outcome.as_deref().map(named).transpose()? == Some(VerificationOutcome::Passed);
            tickets.entry(project).or_default().push(TicketFacts {
                id,
                state: named(&state)?,
                repository_id,
                pull_request,
                passed,
            });
        }
        Ok(tickets)
    }

    /// By project, when one of its sessions, archived ones included, last started a turn. The
    /// Assistant's session has no project.
    fn projects_last_turn(&self) -> Result<BTreeMap<String, i64>> {
        let mut rows = self.db.prepare(
            "SELECT s.project_id, max(json_extract(r.data,'$.last_started_at'))
             FROM runtimes r JOIN sessions s ON s.id=r.session_id
             WHERE s.project_id IS NOT NULL AND json_extract(r.data,'$.last_started_at') IS NOT NULL
             GROUP BY s.project_id",
        )?;
        let turns = rows.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(turns.collect::<rusqlite::Result<_>>()?)
    }

    /// The projects whose coordinator is archived.
    fn archived_projects(&self) -> Result<BTreeSet<String>> {
        let query = format!(
            "SELECT project_id FROM sessions
             WHERE project_id IS NOT NULL AND parent_id IS NULL AND role=?1 AND {ARCHIVED_FLAG}=1"
        );
        let mut rows = self.db.prepare(&query)?;
        let projects = rows.query_map([tag(&Role::ProjectOrchestrator)?], |r| r.get(0))?;
        Ok(projects.collect::<rusqlite::Result<_>>()?)
    }
}

/// A stored enum from its snake_case name.
fn named<T: serde::de::DeserializeOwned>(name: &str) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(
        name.into(),
    ))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::git;

    struct Fixture {
        _home: tempfile::TempDir,
        host: Host,
        repository: Repository,
    }

    /// A store with one repository whose `main` is protected.
    fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("site");
        std::fs::create_dir(&path).unwrap();
        git(&path, &["init", "-q"]);
        git(&path, &["commit", "-q", "--allow-empty", "-m", "Base"]);
        let mut host = Host::open_set_up(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let repository = host.add_repository(path.to_str().unwrap(), "HEAD").unwrap();
        let repository = host
            .set_protected_branch(&repository.id, "main", true)
            .unwrap();
        Fixture {
            _home: home,
            host,
            repository,
        }
    }

    impl Fixture {
        fn project(&mut self, name: &str) -> Project {
            let project = self.host.create_project(name).unwrap();
            let path = self.repository.path.clone();
            self.host
                .attach_repository(&project.id, &path, "HEAD")
                .unwrap();
            project
        }
        fn coordinator(&self, project: &Project) -> Session {
            (self.host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id && s.role == Role::ProjectOrchestrator)
                .unwrap()
        }
        /// A ticket of `project` in `state`, with a pull request into `base` in `pr`, its review
        /// passed when `passed`.
        fn ticket(
            &mut self,
            project: &Project,
            state: TicketState,
            pr: Option<(u64, PrState, &str)>,
            passed: bool,
        ) -> Ticket {
            let coordinator = self.coordinator(project);
            // A ticket's title names its branch, so each is its own.
            let title = format!("Work {}", self.host.tickets().unwrap().len());
            let created = self
                .host
                .create_ticket(&coordinator.id, &self.repository.id, &title, "Do it")
                .unwrap();
            let mut ticket = self.host.ticket(&created.id).unwrap();
            ticket.state = state;
            ticket.pull_request = pr.map(|(number, state, base)| {
                serde_json::from_value(serde_json::json!({
                    "number": number, "url": "u", "state": state, "draft": false,
                    "base": base, "head": "h", "merge_state": "clean", "checks": "none",
                    "unresolved_threads": 0, "comments": {}
                }))
                .unwrap()
            });
            ticket.verification = passed.then(|| {
                serde_json::from_value(serde_json::json!({
                    "cycle": 1, "max_rounds": 3, "outcome": "passed", "rounds": []
                }))
                .unwrap()
            });
            self.host.save_ticket(&ticket).unwrap();
            ticket
        }
        fn facts(&self, project: &Project) -> MeFacts {
            let snapshot = self.host.snapshot().unwrap();
            snapshot
                .me_facts
                .get(&project.id)
                .cloned()
                .unwrap_or_default()
        }
    }

    #[test]
    fn a_project_is_ready_to_close_once_every_ticket_settles_with_its_pr_merged() {
        let mut f = fixture();
        let runners = f.project("CI runners");
        f.ticket(
            &runners,
            TicketState::Accepted,
            Some((1, PrState::Merged, "main")),
            false,
        );
        // A ticket with no pull request does not hold it back.
        f.ticket(&runners, TicketState::Closed, None, false);
        assert!(f.facts(&runners).ready_to_close);

        let recipes = f.project("Recipes");
        f.ticket(
            &recipes,
            TicketState::Closed,
            Some((2, PrState::Closed, "main")),
            false,
        );
        assert!(!f.facts(&recipes).ready_to_close, "a PR closed unmerged");
        let open = f.project("Open work");
        f.ticket(&open, TicketState::Accepted, None, false);
        f.ticket(&open, TicketState::Assigned, None, false);
        assert!(!f.facts(&open).ready_to_close, "an open ticket");
        let empty = f.project("Not started");
        assert!(!f.facts(&empty).ready_to_close, "no tickets");
        let platform = f.project("Platform");
        f.ticket(&platform, TicketState::Accepted, None, false);
        f.host.set_project_standing(&platform.id, true).unwrap();
        assert!(!f.facts(&platform).ready_to_close, "standing");
    }

    #[test]
    fn a_parent_is_ready_to_close_only_once_its_children_are_archived() {
        let mut f = fixture();
        let parent = f.project("Light themes");
        let child = f.project("Generated palette check");
        f.host
            .set_project_parent(&child.id, Some(&parent.id))
            .unwrap();
        f.ticket(&parent, TicketState::Accepted, None, false);
        assert!(!f.facts(&parent).ready_to_close);

        let child_coordinator = f.coordinator(&child);
        f.host.set_archived(&child_coordinator.id, true).unwrap();
        assert!(f.facts(&parent).ready_to_close);
        // An archived project has no facts at all.
        f.host
            .set_archived(&f.coordinator(&parent).id, true)
            .unwrap();
        assert!(!f.host.snapshot().unwrap().me_facts.contains_key(&parent.id));
    }

    #[test]
    fn only_a_reviewed_open_pr_into_a_protected_branch_is_the_humans_to_merge() {
        let mut f = fixture();
        let sponsor = f.project("Redo sponsor page");
        let human = f.ticket(
            &sponsor,
            TicketState::Accepted,
            Some((41, PrState::Open, "main")),
            true,
        );
        f.ticket(
            &sponsor,
            TicketState::Passed,
            Some((42, PrState::Open, "main")),
            false,
        );
        f.ticket(
            &sponsor,
            TicketState::Accepted,
            Some((43, PrState::Open, "feature")),
            true,
        );
        f.ticket(
            &sponsor,
            TicketState::Accepted,
            Some((44, PrState::Merged, "main")),
            true,
        );
        assert_eq!(
            f.facts(&sponsor).merges,
            [HumanMerge {
                ticket_id: human.id,
                number: 41,
                base: "main".into()
            }]
        );
    }

    #[test]
    fn the_last_turn_counts_an_archived_worker_the_snapshot_leaves_out() {
        let mut f = fixture();
        let platform = f.project("Platform");
        f.host.set_project_standing(&platform.id, true).unwrap();
        let ticket = f.ticket(&platform, TicketState::Planned, None, false);
        let worker = (f.host)
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let started = |host: &Host, session: &str, at: i64| {
            let mut runtime = host.session_runtime(session).unwrap();
            runtime.last_started_at = Some(at);
            host.save_runtime(&runtime).unwrap();
        };
        let day = 24 * 60 * 60 * 1000;
        started(&f.host, &f.coordinator(&platform).id, 100 * day);
        started(&f.host, &worker.id, 105 * day);
        let mut accepted = f.host.ticket(&ticket.id).unwrap();
        accepted.state = TicketState::Accepted;
        f.host.save_ticket(&accepted).unwrap();
        f.host.set_archived(&worker.id, true).unwrap();

        let snapshot = f.host.snapshot().unwrap();
        assert!(!snapshot.runtimes.iter().any(|r| r.session_id == worker.id));
        assert_eq!(f.facts(&platform).last_turn_started, Some(105 * day));
    }

    #[test]
    fn the_assistants_session_which_has_no_project_is_left_out() {
        let mut f = fixture();
        let platform = f.project("Platform");
        let assistant = f.host.assistant().unwrap();
        let mut runtime = f.host.session_runtime(&assistant.id).unwrap();
        runtime.last_started_at = Some(7);
        f.host.save_runtime(&runtime).unwrap();

        let snapshot = f.host.snapshot().unwrap();
        assert!(
            snapshot
                .me_facts
                .keys()
                .all(|project| *project == platform.id)
        );
    }
}
