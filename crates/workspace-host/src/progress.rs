//! The progress board. It is derived from the store and each session's latest plan whenever
//! something changes, and never stored, so a restart rebuilds the same figures.
use crate::*;
use std::collections::HashMap;

/// The last derived board, each entry with the revision at which it last changed.
#[derive(Default)]
pub(crate) struct Progress {
    revision: u64,
    /// The revision at which an entry was last removed. A cursor from before it gets the
    /// whole board, since a page of changes cannot say what is gone.
    removed_at: u64,
    agents: Vec<(u64, AgentProgress)>,
    tickets: Vec<(u64, TicketProgress)>,
    projects: Vec<(u64, ProjectProgress)>,
}
impl Progress {
    /// Takes a newly derived board. Entries that are new or differ get the next revision and
    /// the rest keep theirs. Returns whether anything changed, an entry's removal included.
    fn replace(&mut self, board: Board) -> bool {
        let next = self.revision + 1;
        let merged = [
            merge(&mut self.agents, board.agents, next, |a| &a.session_id),
            merge(&mut self.tickets, board.tickets, next, |t| &t.ticket_id),
            merge(&mut self.projects, board.projects, next, |p| &p.project_id),
        ];
        let removed = merged.iter().any(|merge| merge.removed);
        let changed = removed || merged.iter().any(|merge| merge.changed);
        if changed {
            self.revision = next;
        }
        if removed {
            self.removed_at = next;
        }
        changed
    }
    fn page(&self, after: u64) -> ProgressPage {
        fn newer<T: Clone>(entries: &[(u64, T)], after: u64) -> Vec<T> {
            let changed = entries.iter().filter(|(revision, _)| *revision > after);
            changed.map(|(_, entry)| entry.clone()).collect()
        }
        let complete = after == 0 || after < self.removed_at;
        let after = if complete { 0 } else { after };
        ProgressPage {
            revision: self.revision,
            complete,
            agents: newer(&self.agents, after),
            tickets: newer(&self.tickets, after),
            projects: newer(&self.projects, after),
        }
    }
}

struct Merge {
    changed: bool,
    removed: bool,
}

fn merge<T: PartialEq>(
    held: &mut Vec<(u64, T)>,
    derived: Vec<T>,
    next: u64,
    id: impl Fn(&T) -> &str,
) -> Merge {
    let removed = held
        .iter()
        .any(|(_, old)| derived.iter().all(|entry| id(entry) != id(old)));
    let mut changed = false;
    let merged = derived
        .into_iter()
        .map(|entry| {
            let same = held.iter().find(|(_, old)| id(old) == id(&entry));
            match same.filter(|(_, old)| *old == entry) {
                Some((revision, _)) => (*revision, entry),
                None => {
                    changed = true;
                    (next, entry)
                }
            }
        })
        .collect();
    *held = merged;
    Merge { changed, removed }
}

impl Host {
    /// Each session's checklist, read from stored steps on first use after a start.
    fn plans(&mut self) -> Result<&mut HashMap<String, PlanCounts>> {
        if self.plans.is_none() {
            self.plans = Some(self.latest_plans()?);
        }
        Ok(self.plans.as_mut().expect("loaded above"))
    }
    /// A recorded plan step replaces the session's checklist; one without counts clears it.
    pub(crate) fn plan_recorded(
        &mut self,
        session_id: &str,
        plan: Option<&PlanCounts>,
    ) -> Result<()> {
        let plans = self.plans()?;
        match plan {
            Some(counts) => plans.insert(session_id.into(), counts.clone()),
            None => plans.remove(session_id),
        };
        Ok(())
    }
    /// A new run starts with no checklist, so its agent is indeterminate until it writes one.
    pub(crate) fn run_started(&mut self, session_id: &str) -> Result<()> {
        self.plans()?.remove(session_id);
        Ok(())
    }
    /// Derives the board again; true when any entry changed. Writes nothing and wakes no one.
    pub(crate) fn refresh_progress(&mut self) -> Result<bool> {
        let (sessions, runtimes, tickets) = (self.sessions()?, self.runtimes()?, self.tickets()?);
        let board = derive(&sessions, &runtimes, &tickets, self.plans()?);
        Ok(self.progress.replace(board))
    }
    /// The board entries changed after `after`, as of now.
    pub fn progress(&mut self, after: u64) -> Result<ProgressPage> {
        self.refresh_progress()?;
        Ok(self.progress.page(after))
    }
}
