//! The sidebar's shape in the Outcome lens: a section for each Space, listing its loose projects
//! and parents in the host's order with each parent's children under it, each project one row
//! for its coordinator. Tickets and their agents are listed in the Agents panel.
use crate::agents_panel::Roster;
use crate::progress::ProgressBoard;
use std::collections::{BTreeMap, BTreeSet};
use workspace_core::*;

pub(crate) enum Row<'a> {
    Space {
        space: SpaceId,
        name: String,
        /// Its shown projects' coordinators, parents' children included.
        projects: Vec<&'a Session>,
    },
    /// A loose project, or a child under its parent's row.
    Project { root: &'a Session, child: bool },
    /// A parent; its children's rows follow unless it is collapsed.
    Parent {
        root: &'a Session,
        /// Its shown children's coordinators, in the host's order.
        children: Vec<&'a Session>,
    },
}

/// A top-level row of a Space's section: a loose project, or a parent with its children.
#[derive(Debug)]
pub(crate) struct Entry<'a> {
    pub root: &'a Session,
    /// Its shown children's coordinators, in the host's order.
    pub children: Vec<&'a Session>,
    /// It may be filed under another project: it has no parent and no children, shown or not.
    fileable: bool,
    /// Projects may be filed under it: it has no parent and is not archived.
    adopts: bool,
}

/// One Space's shown projects, under its section row.
#[derive(Debug)]
pub(crate) struct Section<'a> {
    /// Its Space's id; empty for the projects in no listed Space.
    pub space: SpaceId,
    pub name: String,
    pub entries: Vec<Entry<'a>>,
}

/// The section a project is listed in: its Space's, or the one for projects in no listed Space,
/// such as those from a host older than Spaces.
pub(crate) fn section_of(index: &WorkspaceIndex, project: &Project) -> SpaceId {
    let spaces = &index.snapshot().spaces;
    if spaces.iter().any(|space| space.id == project.space) {
        project.space.clone()
    } else {
        SpaceId::default()
    }
}

/// A section for each Space with a shown project, in the Spaces' order, then No Space with the
/// projects in none. Archived projects count only when `show_archived`.
pub(crate) fn sections(index: &WorkspaceIndex, show_archived: bool) -> Vec<Section<'_>> {
    let snapshot = index.snapshot();
    let parents: BTreeSet<&str> = (snapshot.projects.iter())
        .filter_map(|project| project.parent_id.as_deref())
        .collect();
    let listed = (snapshot.spaces.iter()).map(|space| (space.id.clone(), space.name.clone()));
    let none = (SpaceId::default(), "No Space".to_owned());
    listed
        .chain([none])
        .map(|(space, name)| Section {
            entries: entries(index, &space, show_archived, &parents),
            space,
            name,
        })
        .filter(|section| !section.entries.is_empty())
        .collect()
}

/// The section for `space`: its loose projects and parents in the host's order, and each
/// parent's children in the host's order under it, wherever they are stored. A child whose
/// parent is not shown is listed where it is stored.
fn entries<'a>(
    index: &'a WorkspaceIndex,
    space: &SpaceId,
    show_archived: bool,
    parents: &BTreeSet<&str>,
) -> Vec<Entry<'a>> {
    let shown: Vec<(&Project, &Session)> = (index.snapshot().projects.iter())
        // The Assistant's work is listed in its rail, never as a project row.
        .filter(|project| project.assistant_work.is_none() && section_of(index, project) == *space)
        .filter_map(|project| {
            let root = index
                .roots(&project.id)
                .find(|s| show_archived || !s.archived)?;
            Some((project, root))
        })
        .collect();
    let top_level: BTreeSet<&str> = (shown.iter())
        .filter(|(project, _)| project.parent_id.is_none())
        .map(|(project, _)| project.id.as_str())
        .collect();
    let mut entries = vec![];
    let mut children = vec![];
    for (project, root) in shown {
        match project.parent_id.as_deref() {
            Some(parent) if top_level.contains(parent) => children.push((parent, root)),
            _ => entries.push(Entry {
                root,
                children: vec![],
                fileable: project.parent_id.is_none() && !parents.contains(project.id.as_str()),
                adopts: project.parent_id.is_none() && !root.archived,
            }),
        }
    }
    let at: BTreeMap<String, usize> = (entries.iter().enumerate())
        .map(|(at, entry)| (entry.root.project_id.clone(), at))
        .collect();
    for (parent, root) in children {
        entries[at[parent]].children.push(root);
    }
    entries
}

impl Entry<'_> {
    fn id(&self) -> String {
        self.root.project_id.clone()
    }
}

impl<'a> Section<'a> {
    /// Its projects' coordinators, parents' children included.
    fn projects(&self) -> Vec<&'a Session> {
        let entries = self.entries.iter();
        let all = entries.flat_map(|entry| [entry.root].into_iter().chain(entry.children.clone()));
        all.collect()
    }
}

/// Projects under their Space's row, and children under their parent's row; a collapsed Space
/// or parent hides what is under it. Archived projects appear only when `show_archived`.
pub(crate) fn rows<'a>(
    index: &'a WorkspaceIndex,
    show_archived: bool,
    collapsed_spaces: &BTreeSet<SpaceId>,
    collapsed_parents: &BTreeSet<String>,
) -> Vec<Row<'a>> {
    let mut rows = vec![];
    for section in sections(index, show_archived) {
        let collapsed = collapsed_spaces.contains(&section.space);
        rows.push(Row::Space {
            projects: section.projects(),
            space: section.space,
            name: section.name,
        });
        if collapsed {
            continue;
        }
        for Entry { root, children, .. } in section.entries {
            if children.is_empty() {
                rows.push(Row::Project { root, child: false });
                continue;
            }
            let under = if collapsed_parents.contains(&root.project_id) {
                vec![]
            } else {
                children.clone()
            };
            rows.push(Row::Parent { root, children });
            rows.extend(
                under
                    .into_iter()
                    .map(|root| Row::Project { root, child: true }),
            );
        }
    }
    rows
}

/// The most urgent state inside a collapsed parent or Space.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rollup {
    NeedsYou(usize),
    Stuck(usize),
    Blocked,
    Working,
}

/// A collapsed parent's or Space's state, from the items in it that need the human, its stuck
/// agents and the status each project's row shows. Questions with a parent are not the human's,
/// so they never count here.
pub(crate) fn rollup(needs_you: usize, stuck: usize, shown: &[Status]) -> Option<Rollup> {
    if needs_you > 0 {
        Some(Rollup::NeedsYou(needs_you))
    } else if stuck > 0 {
        Some(Rollup::Stuck(stuck))
    } else if shown.contains(&Status::Blocked) {
        Some(Rollup::Blocked)
    } else if shown.contains(&Status::Working) {
        Some(Rollup::Working)
    } else {
        None
    }
}

/// What a project row says about the agents the Agents panel lists for it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentCounts {
    pub working: usize,
    pub stuck: usize,
}

pub(crate) fn agent_counts(
    board: &ProgressBoard,
    index: &WorkspaceIndex,
    coordinator: &Session,
) -> AgentCounts {
    let roster = Roster::new(board, index, &coordinator.project_id, &coordinator.id);
    AgentCounts {
        working: roster.working(),
        stuck: roster.stuck(index),
    }
}

/// What a drag moves: a project by its id, with its shown children when it is a parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Movable(pub String);

/// Where a dragged row lands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Drop {
    /// Above this project's row, among the rows listed with it.
    Before(String),
    /// Below the last shown child of this parent.
    EndOfChildren(String),
    /// Below the last row of the section for this Space.
    End(SpaceId),
    /// Onto this loose project or parent, filing the dragged one under it.
    Under(String),
}

/// Where a dragged row lands when released over the top, the middle and the bottom of a row;
/// none where it may not land.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Aims {
    pub above: Option<Drop>,
    pub onto: Option<Drop>,
    pub below: Option<Drop>,
}

impl Aims {
    /// Both halves of a row land at `drop`.
    fn only(drop: Drop) -> Self {
        Self {
            above: Some(drop.clone()),
            onto: None,
            below: Some(drop),
        }
    }
    /// Where a release lands `fraction` of the way down the row: its top and bottom quarters, or
    /// its middle, where nothing lands when the dragged row may not drop onto it.
    pub(crate) fn at(&self, fraction: f32) -> Option<Drop> {
        if fraction < 0.25 {
            self.above.clone()
        } else if fraction > 0.75 {
            self.below.clone()
        } else {
            self.onto.clone()
        }
    }
}

/// Where a project's row is in its section: the top-level row `entry`, or that row's child
/// `child`.
struct Spot<'s, 'a> {
    section: &'s Section<'a>,
    entry: usize,
    child: Option<usize>,
}

impl<'s, 'a> Spot<'s, 'a> {
    fn find(sections: &'s [Section<'a>], id: &str) -> Option<Self> {
        sections.iter().find_map(|section| {
            (section.entries.iter().enumerate()).find_map(|(entry, listed)| {
                let child = if listed.root.project_id == id {
                    None
                } else {
                    Some(listed.children.iter().position(|c| c.project_id == id)?)
                };
                Some(Spot {
                    section,
                    entry,
                    child,
                })
            })
        })
    }
    fn entry(&self) -> &'s Entry<'a> {
        &self.section.entries[self.entry]
    }
    /// Below this top-level row and its children: above the next one, or at the section's end.
    fn after_block(&self) -> Drop {
        match self.section.entries.get(self.entry + 1) {
            Some(next) => Drop::Before(next.id()),
            None => Drop::End(self.section.space.clone()),
        }
    }
    /// Below the child `at` of this row: above the next child, or below the last.
    fn after_child(&self, at: usize) -> Drop {
        match self.entry().children.get(at + 1) {
            Some(next) => Drop::Before(next.project_id.clone()),
            None => Drop::EndOfChildren(self.entry().id()),
        }
    }
    /// The top-level row this one is, or is a child of, may be filed under `parent`.
    fn may_file_under(&self, parent: &Entry) -> bool {
        self.child.is_none() && self.entry().fileable && parent.adopts
    }
}

/// The Space whose section lists `row`.
pub(crate) fn space_of<'a>(sections: &'a [Section], row: &Movable) -> Option<&'a SpaceId> {
    Some(&Spot::find(sections, &row.0)?.section.space)
}

/// Where `dragged` lands when released over the row of `over`, none when nowhere on it. Nothing
/// leaves its Space's section. Loose projects and parents land among the top-level rows, a
/// parent's children with it; a child lands among its siblings or, made loose, among the
/// top-level rows. A loose project with no children may also land on a loose project or parent,
/// or among a parent's children, to be filed under it. Over rows where it may not land, it lands
/// below their parent's block.
pub(crate) fn aims(
    sections: &[Section],
    dragged: &Movable,
    over: &Movable,
    collapsed_parents: &BTreeSet<String>,
) -> Option<Aims> {
    let from = Spot::find(sections, &dragged.0)?;
    let to = Spot::find(sections, &over.0)?;
    if from.section.space != to.section.space {
        return None;
    }
    let target = to.entry();
    let own_parent = from.child.is_some() && from.entry == to.entry;
    let joins = own_parent || from.may_file_under(target);
    let aims = match to.child {
        None => {
            let expanded =
                !target.children.is_empty() && !collapsed_parents.contains(&target.root.project_id);
            let below = if expanded && joins {
                Drop::Before(target.children[0].project_id.clone())
            } else {
                to.after_block()
            };
            let onto = (from.may_file_under(target) && from.entry != to.entry)
                .then(|| Drop::Under(target.id()));
            Aims {
                above: Some(Drop::Before(target.id())),
                onto,
                below: Some(below),
            }
        }
        Some(child) if joins => Aims {
            above: Some(Drop::Before(over.0.clone())),
            onto: None,
            below: Some(to.after_child(child)),
        },
        Some(_) => Aims::only(to.after_block()),
    };
    Some(aims)
}

/// The host command's arguments for one reorder.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ProjectMove {
    pub project_ids: Vec<String>,
    pub before: Option<String>,
}

/// What a drop asks of the host.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Landing {
    Move(ProjectMove),
    /// Files `project` under `parent`, or makes it loose with none, once the human confirms;
    /// then `then` places it where it was dropped.
    Refile {
        project: String,
        parent: Option<String>,
        then: Option<ProjectMove>,
    },
}

/// What dropping `dragged` at `drop` does; none when it is already there or may not land there.
/// The host keeps one order for every Space, and only a project's place among its own section's
/// rows, or among its siblings, matters to the tree.
pub(crate) fn landing(
    index: &WorkspaceIndex,
    sections: &[Section],
    dragged: &Movable,
    drop: &Drop,
) -> Option<Landing> {
    let from = Spot::find(sections, &dragged.0)?;
    let (section, own) = (from.section, from.entry());
    let target = match drop {
        Drop::Before(id) | Drop::EndOfChildren(id) | Drop::Under(id) => {
            Some(Spot::find(sections, id)?).filter(|spot| spot.section.space == section.space)
        }
        Drop::End(space) if *space == section.space => None,
        Drop::End(_) => return None,
    };
    let refile = |parent: Option<&Entry>, then| {
        Some(Landing::Refile {
            project: dragged.0.clone(),
            parent: parent.map(Entry::id),
            then,
        })
    };
    let placed_before = |id: &String| {
        Some(ProjectMove {
            project_ids: vec![dragged.0.clone()],
            before: Some(id.clone()),
        })
    };
    match (from.child, drop, target) {
        // A loose project or a parent among the top-level rows.
        (
            None,
            Drop::Before(_),
            Some(Spot {
                child: None, entry, ..
            }),
        ) => reorder_top(section, from.entry, entry).map(Landing::Move),
        (None, Drop::End(_), None) => {
            reorder_top(section, from.entry, section.entries.len()).map(Landing::Move)
        }
        // A loose project filed by a drop on a project or among a parent's children.
        (None, Drop::Under(_), Some(spot)) if spot.child.is_none() && spot.entry != from.entry => {
            let parent = spot.entry();
            if !from.may_file_under(parent) {
                return None;
            }
            refile(Some(parent), after_children(index, parent, &dragged.0))
        }
        (None, Drop::EndOfChildren(_), Some(spot)) if spot.child.is_none() => {
            let parent = spot.entry();
            if !from.may_file_under(parent) || parent.children.is_empty() {
                return None;
            }
            refile(Some(parent), after_children(index, parent, &dragged.0))
        }
        (None, Drop::Before(id), Some(spot))
            if spot.child.is_some() && from.may_file_under(spot.entry()) =>
        {
            refile(Some(spot.entry()), placed_before(id))
        }
        // A child among its siblings.
        (
            Some(at),
            Drop::Before(_),
            Some(Spot {
                child: Some(to),
                entry,
                ..
            }),
        ) if entry == from.entry => reorder_children(own, at, to).map(Landing::Move),
        (
            Some(at),
            Drop::EndOfChildren(_),
            Some(Spot {
                child: None, entry, ..
            }),
        ) if entry == from.entry => {
            reorder_children(own, at, own.children.len()).map(Landing::Move)
        }
        // A child made loose among the top-level rows.
        (Some(_), Drop::Before(id), Some(Spot { child: None, .. })) => {
            refile(None, placed_before(id))
        }
        (Some(_), Drop::End(_), None) => refile(
            None,
            Some(ProjectMove {
                project_ids: vec![dragged.0.clone()],
                before: None,
            }),
        ),
        _ => None,
    }
}

/// Moves the top-level row `from`, with its children, to just above the row `to`, or to the end
/// of the order past the last one, which is still below every other project of its Space.
fn reorder_top(section: &Section, from: usize, to: usize) -> Option<ProjectMove> {
    if to == from || to == from + 1 {
        return None;
    }
    let block = &section.entries[from];
    let ids = [block.root]
        .into_iter()
        .chain(block.children.iter().copied());
    Some(ProjectMove {
        project_ids: ids.map(|root| root.project_id.clone()).collect(),
        before: section.entries.get(to).map(Entry::id),
    })
}

/// Moves child `from` of `parent` to just above child `to`, or below the last past it.
fn reorder_children(parent: &Entry, from: usize, to: usize) -> Option<ProjectMove> {
    if to == from || to == from + 1 {
        return None;
    }
    let ids: Vec<String> = (parent.children.iter())
        .map(|child| child.project_id.clone())
        .collect();
    if to > from {
        // The siblings it passes move up above it, so nothing lands outside the siblings' span.
        return Some(ProjectMove {
            project_ids: ids[from + 1..to].to_vec(),
            before: Some(ids[from].clone()),
        });
    }
    Some(ProjectMove {
        project_ids: vec![ids[from].clone()],
        before: Some(ids[to].clone()),
    })
}

/// The move that puts `project` just after `parent`'s last shown child in the host's order;
/// none when it has no children or `project` already follows the last.
fn after_children(index: &WorkspaceIndex, parent: &Entry, project: &str) -> Option<ProjectMove> {
    let last = &parent.children.last()?.project_id;
    let order = &index.snapshot().projects;
    let at = order.iter().position(|p| &p.id == last)?;
    let next = order.get(at + 1).map(|p| p.id.clone());
    (next.as_deref() != Some(project)).then(|| ProjectMove {
        project_ids: vec![project.to_owned()],
        before: next,
    })
}

/// How near an edge of the list a dragged row starts scrolling it, and the most one tick scrolls.
const SCROLL_ZONE: f32 = 32.;
const SCROLL_STEP: f32 = 12.;

/// How far one tick scrolls the list while a row is dragged with the pointer at `pointer`
/// between the list's `top` and `bottom` edges: up when negative, down when positive, and
/// further the nearer the edge. Zero away from both edges and outside the list.
pub(crate) fn edge_scroll(pointer: f32, top: f32, bottom: f32) -> f32 {
    if pointer < top || pointer > bottom {
        return 0.;
    }
    let step = |from_edge: f32| ((SCROLL_ZONE - from_edge) / SCROLL_ZONE * SCROLL_STEP).ceil();
    if pointer - top < SCROLL_ZONE {
        -step(pointer - top)
    } else if bottom - pointer < SCROLL_ZONE {
        step(bottom - pointer)
    } else {
        0.
    }
}

/// What releasing a dragged row over itself does.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OwnRelease {
    /// The drag left its row and came back: it is cancelled.
    Nothing,
    Click,
    /// The second click of a double-click.
    Rename,
}

/// A drag that never left its row is a click that moved, so it does what that click, the
/// `click_count`th of its press, would have done still.
pub(crate) fn own_release(left_row: bool, click_count: usize) -> OwnRelease {
    if left_row {
        OwnRelease::Nothing
    } else if click_count == 2 {
        OwnRelease::Rename
    } else {
        OwnRelease::Click
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ticket_store as store;
    use std::rc::Rc;
    use workspace_host::Host;

    /// The host's snapshot joined with its archive, as the app shows it.
    fn joined(host: &Host) -> WorkspaceIndex {
        let archive = Rc::new(host.archive().unwrap());
        WorkspaceIndex::new(host.snapshot().unwrap(), archive)
    }

    #[test]
    fn a_project_is_one_row_whatever_tickets_and_agents_it_has() {
        let (_directory, mut host, root, ticket) = store();
        host.assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let finished = host
            .assign_ticket(&ticket.id, Role::Reviewer, Provider::Codex, "Review", None)
            .unwrap();
        host.set_archived(&finished.id, true).unwrap();
        let repository = host.repositories().unwrap().remove(0);
        let closed = host
            .create_ticket(&root.id, &repository.id, "Old calls", "Done")
            .unwrap();
        host.close_ticket(&closed.id).unwrap();
        host.create_session(
            &root.project_id,
            &root.id,
            None,
            "Older worker",
            Role::Implementer,
            Provider::Codex,
        )
        .unwrap();
        let snapshot = joined(&host);

        // Open, archived and legacy agents alike are listed in the Agents panel instead.
        for show_archived in [false, true] {
            assert_eq!(drawn(&snapshot, show_archived), ["PERSONAL (1)", "Calls"]);
        }
    }

    #[test]
    fn the_assistants_work_is_never_a_project_row() {
        let (_directory, host, _, _) = store();
        let mut snapshot = host.snapshot().unwrap();
        snapshot.projects[0].assistant_work = Some(AssistantWork {
            small: true,
            state: WorkState::Running,
            ended_at: None,
        });
        let index = WorkspaceIndex::new(snapshot, Default::default());
        assert!(drawn(&index, true).is_empty());
    }

    /// The store's index with `agent` in `status`, and its last turn failed when `failed`.
    fn with_agent(host: &Host, agent: &str, status: Status, failed: bool) -> WorkspaceIndex {
        let mut snapshot = host.snapshot().unwrap();
        let session = snapshot
            .sessions
            .iter_mut()
            .find(|s| s.id == agent)
            .unwrap();
        session.status = status;
        let runtime = snapshot.runtimes.iter_mut().find(|r| r.session_id == agent);
        runtime.unwrap().last_error = failed.then(|| "The provider exited".into());
        WorkspaceIndex::new(snapshot, Default::default())
    }

    #[test]
    fn a_project_row_counts_its_working_and_stuck_agents() {
        let (_directory, mut host, root, ticket) = store();
        let implementer = host
            .assign_ticket(&ticket.id, Role::Implementer, Provider::Codex, "Fix", None)
            .unwrap();
        let tester = host
            .assign_ticket(&ticket.id, Role::Tester, Provider::Codex, "Test", None)
            .unwrap();
        let mut board = ProgressBoard::default();
        board.merge(host.progress(0).unwrap());
        let working = AgentProgress {
            working: true,
            ..board.agent(&implementer.id).unwrap().clone()
        };
        board.merge(ProgressPage {
            revision: 2,
            complete: false,
            agents: vec![working],
            tickets: vec![],
            projects: vec![],
        });
        let counts = |index: &WorkspaceIndex| agent_counts(&board, index, &root);

        let quiet = with_agent(&host, &tester.id, Status::Ready, false);
        assert_eq!(
            counts(&quiet),
            AgentCounts {
                working: 1,
                stuck: 0
            }
        );
        for (status, failed) in [(Status::Disconnected, false), (Status::Ready, true)] {
            let stuck = with_agent(&host, &tester.id, status, failed);
            assert_eq!(counts(&stuck).stuck, 1, "{status:?}");
        }
        // The coordinator answers a blocked agent, so nothing is raised for it.
        let blocked = with_agent(&host, &tester.id, Status::Blocked, true);
        assert_eq!(counts(&blocked).stuck, 0);
    }

    /// A store whose projects read, top to bottom, as `names`.
    fn listed(names: &[&str]) -> (tempfile::TempDir, Host) {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(directory.path()).unwrap();
        for name in names.iter().rev() {
            host.create_project(name).unwrap();
        }
        (directory, host)
    }

    fn id(host: &Host, name: &str) -> String {
        let projects = host.projects().unwrap();
        projects.into_iter().find(|p| p.name == name).unwrap().id
    }

    /// Files each `(child, parent)` pair, by name.
    fn file(host: &mut Host, pairs: &[(&str, &str)]) {
        for (child, parent) in pairs {
            let (child, parent) = (id(host, child), id(host, parent));
            host.set_project_parent(&child, Some(&parent)).unwrap();
        }
    }

    /// Times what the desktop does with a store's workspace: decoding the snapshot and the
    /// archive, joining them, and the sidebar's rows. Run it on a copy of a store, such as the
    /// benchmark's kept fixture: `WIFFLETREE_BENCH_DIR=<folder> cargo test --release -p
    /// workspace-desktop tree_rows_benchmark -- --ignored --nocapture`. Never on a live store.
    #[test]
    #[ignore]
    fn tree_rows_benchmark() {
        let folder = std::env::var("WIFFLETREE_BENCH_DIR").expect("WIFFLETREE_BENCH_DIR");
        let host = Host::open(folder).unwrap();
        let timed = |name: &str, work: &mut dyn FnMut()| {
            let mut times: Vec<f64> = (0..200)
                .map(|_| {
                    let start = std::time::Instant::now();
                    work();
                    start.elapsed().as_secs_f64() * 1000.
                })
                .collect();
            times.sort_by(f64::total_cmp);
            println!(
                "{name}: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms",
                times[100], times[190], times[199]
            );
        };
        let snapshot_bytes = serde_json::to_vec(&host.snapshot().unwrap()).unwrap().len();
        let archive_bytes = serde_json::to_vec(&host.archive().unwrap()).unwrap().len();
        println!("snapshot {snapshot_bytes} bytes, archive {archive_bytes} bytes");
        timed("snapshot with JSON round trip", &mut || {
            let value = serde_json::to_value(host.snapshot().unwrap()).unwrap();
            std::hint::black_box(serde_json::from_value::<Snapshot>(value).unwrap());
        });
        timed("archive with JSON round trip", &mut || {
            let value = serde_json::to_value(host.archive().unwrap()).unwrap();
            std::hint::black_box(serde_json::from_value::<Archive>(value).unwrap());
        });
        let (snapshot, archive) = (host.snapshot().unwrap(), Rc::new(host.archive().unwrap()));
        timed("index build", &mut || {
            let index = WorkspaceIndex::new(snapshot.clone(), archive.clone());
            std::hint::black_box(index);
        });
        let index = WorkspaceIndex::new(snapshot, archive);
        let (spaces, parents) = (BTreeSet::new(), BTreeSet::new());
        for show_archived in [false, true] {
            timed(
                &format!("tree rows, show archived {show_archived}"),
                &mut || {
                    std::hint::black_box(rows(&index, show_archived, &spaces, &parents));
                },
            );
        }
    }

    /// The project list as drawn: Space rows in capitals with their count, parents with their
    /// shown children's count, and children indented.
    fn drawn(index: &WorkspaceIndex, show_archived: bool) -> Vec<String> {
        labels(&rows(
            index,
            show_archived,
            &BTreeSet::new(),
            &BTreeSet::new(),
        ))
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Space { name, projects, .. } => {
                    format!("{} ({})", name.to_uppercase(), projects.len())
                }
                Row::Parent { root, children } => format!("{} +{}", root.name, children.len()),
                Row::Project { root, child: true } => format!("  {}", root.name),
                Row::Project { root, .. } => root.name.clone(),
            })
            .collect()
    }

    const CELADON: [&str; 4] = [
        "Accessible contrast",
        "Contrast warnings",
        "Light themes",
        "Recipes",
    ];

    #[test]
    fn children_are_drawn_under_their_parent_wherever_they_are_stored() {
        let (_directory, mut host) = listed(&CELADON);
        file(&mut host, &[("Contrast warnings", "Accessible contrast")]);
        let expected = [
            "PERSONAL (4)",
            "Accessible contrast +1",
            "  Contrast warnings",
            "Light themes",
            "Recipes",
        ];
        assert_eq!(drawn(&joined(&host), false), expected);

        // Stored above its parent, the child is still drawn under it, where the parent sits.
        let child = id(&host, "Contrast warnings");
        let light = id(&host, "Light themes");
        host.move_projects(&[id(&host, "Accessible contrast")], Some(&light))
            .unwrap();
        assert_eq!(host.projects().unwrap()[0].id, child);
        assert_eq!(drawn(&joined(&host), false), expected);
    }

    #[test]
    fn names_sharing_a_first_word_are_ordinary_rows() {
        let (_directory, host) = listed(&[
            "Pickers: restore highlights",
            "Pickers: generated palette check",
        ]);
        assert_eq!(
            drawn(&joined(&host), false),
            [
                "PERSONAL (2)",
                "Pickers: restore highlights",
                "Pickers: generated palette check"
            ]
        );
    }

    #[test]
    fn archived_children_show_only_with_the_archived() {
        let (_directory, mut host) = listed(&["Light themes", "Daylight", "Palette check"]);
        file(
            &mut host,
            &[
                ("Daylight", "Light themes"),
                ("Palette check", "Light themes"),
            ],
        );
        let daylight = host.sessions().unwrap();
        let daylight = daylight.iter().find(|s| s.name == "Daylight").unwrap();
        host.set_archived(&daylight.id, true).unwrap();
        let snapshot = joined(&host);

        assert_eq!(
            drawn(&snapshot, false),
            ["PERSONAL (2)", "Light themes +1", "  Palette check"]
        );
        assert_eq!(
            drawn(&snapshot, true),
            [
                "PERSONAL (3)",
                "Light themes +2",
                "  Daylight",
                "  Palette check"
            ]
        );
    }

    /// A store with the Spaces Personal, Vyttle, Sage and Work, whose projects read, top to
    /// bottom, as `projects`, each in the Space it is paired with.
    fn in_spaces(projects: &[(&str, &str)]) -> (tempfile::TempDir, Host) {
        let (directory, mut host) = listed(&[]);
        let mut spaces = vec![("Personal".to_owned(), home(&host))];
        for name in ["Vyttle", "Sage", "Work"] {
            let space = host.create_space(name, "", "", None).unwrap().space.id;
            spaces.push((name.to_owned(), space));
        }
        for (space, name) in projects.iter().rev() {
            let (_, space) = spaces.iter().find(|(named, _)| named == space).unwrap();
            host.create_project_in(name, space).unwrap();
        }
        (directory, host)
    }

    const SPACED: [(&str, &str); 6] = [
        ("Sage", "Trial reminder copy"),
        ("Vyttle", "Organize the list"),
        ("Personal", "Garden planner"),
        ("Vyttle", "Repository owners"),
        ("Personal", "Reading list"),
        ("Vyttle", "Sky contrast"),
    ];

    #[test]
    fn each_space_with_a_shown_project_is_a_section_in_the_spaces_order() {
        let (_directory, mut host) = in_spaces(&SPACED);
        file(&mut host, &[("Sky contrast", "Organize the list")]);

        // Work has no project, so no section.
        assert_eq!(
            drawn(&joined(&host), false),
            [
                "PERSONAL (2)",
                "Garden planner",
                "Reading list",
                "VYTTLE (3)",
                "Organize the list +1",
                "  Sky contrast",
                "Repository owners",
                "SAGE (1)",
                "Trial reminder copy",
            ]
        );
    }

    #[test]
    fn a_collapsed_space_or_parent_hides_what_is_under_it_but_keeps_it_for_the_rollup() {
        let (_directory, mut host) = in_spaces(&SPACED);
        file(&mut host, &[("Sky contrast", "Organize the list")]);
        let snapshot = joined(&host);
        let personal = host.spaces().unwrap()[0].space.id.clone();
        let organize = BTreeSet::from([id(&host, "Organize the list")]);

        let collapsed = rows(&snapshot, false, &BTreeSet::from([personal]), &organize);

        assert_eq!(
            labels(&collapsed),
            [
                "PERSONAL (2)",
                "VYTTLE (3)",
                "Organize the list +1",
                "Repository owners",
                "SAGE (1)",
                "Trial reminder copy",
            ]
        );
        // A collapsed parent keeps its children, and its Space's row every project, for their
        // rollups.
        let Row::Parent { children, .. } = &collapsed[2] else {
            panic!("the parent's row");
        };
        assert_eq!(children[0].name, "Sky contrast");
        let Row::Space { projects, .. } = &collapsed[0] else {
            panic!("Personal's row");
        };
        assert_eq!(projects.len(), 2);
    }

    #[test]
    fn a_project_from_no_listed_space_is_listed_under_no_space_last() {
        let (_directory, host) = in_spaces(&SPACED[..2]);
        let mut snapshot = host.snapshot().unwrap();
        snapshot.projects[0].space = SpaceId::default();

        assert_eq!(
            drawn(&WorkspaceIndex::new(snapshot, Default::default()), false),
            [
                "VYTTLE (1)",
                "Organize the list",
                "NO SPACE (1)",
                "Trial reminder copy"
            ]
        );
    }

    #[test]
    fn a_rollup_shows_the_most_urgent_state() {
        use Status::*;
        assert_eq!(
            rollup(2, 1, &[Blocked, Working]),
            Some(Rollup::NeedsYou(2)),
            "needs you first"
        );
        assert_eq!(
            rollup(0, 1, &[Blocked, Working]),
            Some(Rollup::Stuck(1)),
            "then stuck agents"
        );
        assert_eq!(
            rollup(0, 0, &[Working, Blocked, Ready]),
            Some(Rollup::Blocked)
        );
        assert_eq!(
            rollup(0, 0, &[Done, Working, Paused]),
            Some(Rollup::Working)
        );
        assert_eq!(rollup(0, 0, &[Ready, Done, Paused, Disconnected]), None);
    }

    /// The Space `listed` puts every project in.
    fn home(host: &Host) -> SpaceId {
        host.default_space().unwrap()
    }

    fn project(host: &Host, name: &str) -> Movable {
        Movable(id(host, name))
    }

    fn before(host: &Host, name: &str) -> Drop {
        Drop::Before(id(host, name))
    }

    /// What dropping `dragged` at `drop` asks of the host, from the rows shown.
    fn landed(host: &Host, dragged: &str, drop: &Drop) -> Option<Landing> {
        let snapshot = joined(host);
        let sections = sections(&snapshot, false);
        landing(&snapshot, &sections, &project(host, dragged), drop)
    }

    /// The tree after `dragged` is dropped at `drop` and any confirmation is accepted; none when
    /// the drop does nothing.
    fn dropped(host: &mut Host, dragged: &str, drop: &Drop) -> Option<Vec<String>> {
        let landing = landed(host, dragged, drop)?;
        let moved = match landing {
            Landing::Move(moved) => Some(moved),
            Landing::Refile {
                project,
                parent,
                then,
            } => {
                host.set_project_parent(&project, parent.as_deref())
                    .unwrap();
                then
            }
        };
        if let Some(moved) = moved {
            host.move_projects(&moved.project_ids, moved.before.as_deref())
                .unwrap();
        }
        Some(drawn(&joined(host), false)[1..].to_vec())
    }

    #[test]
    fn loose_projects_reorder_among_themselves() {
        let (_directory, mut host) = listed(&["a", "b", "c", "d"]);
        let end = Drop::End(home(&host));

        let above_a = before(&host, "a");
        assert_eq!(
            dropped(&mut host, "d", &above_a).unwrap(),
            ["d", "a", "b", "c"]
        );
        assert_eq!(dropped(&mut host, "d", &end).unwrap(), ["a", "b", "c", "d"]);
        // Onto the place it already has: nothing to send.
        assert_eq!(landed(&host, "d", &end), None);
        assert_eq!(landed(&host, "a", &before(&host, "a")), None);
        assert_eq!(landed(&host, "a", &before(&host, "b")), None);
    }

    #[test]
    fn a_child_moves_among_its_siblings_and_its_parent_stays() {
        let names = [
            "Accessible contrast",
            "Dimmed inactive panes",
            "Colour-blind palettes",
            "Contrast warnings",
            "Recipes",
        ];
        let (_directory, mut host) = listed(&names);
        let pairs: Vec<_> = (names[1..4].iter())
            .map(|child| (*child, "Accessible contrast"))
            .collect();
        file(&mut host, &pairs);
        let parent = id(&host, "Accessible contrast");

        let above_dimmed = before(&host, "Dimmed inactive panes");
        assert_eq!(
            dropped(&mut host, "Contrast warnings", &above_dimmed).unwrap(),
            [
                "Accessible contrast +3",
                "  Contrast warnings",
                "  Dimmed inactive panes",
                "  Colour-blind palettes",
                "Recipes"
            ]
        );
        // Down to the end of its siblings, the ones it passes move up instead.
        assert_eq!(
            dropped(
                &mut host,
                "Contrast warnings",
                &Drop::EndOfChildren(parent.clone())
            )
            .unwrap(),
            [
                "Accessible contrast +3",
                "  Dimmed inactive panes",
                "  Colour-blind palettes",
                "  Contrast warnings",
                "Recipes"
            ]
        );
        assert_eq!(
            landed(&host, "Contrast warnings", &Drop::EndOfChildren(parent)),
            None,
            "already last"
        );
    }

    #[test]
    fn a_parent_moves_as_a_block_with_its_children() {
        let names = [
            "Accessible contrast",
            "Recipes",
            "Light themes",
            "Daylight",
            "Palette",
        ];
        let (_directory, mut host) = listed(&names);
        file(
            &mut host,
            &[("Daylight", "Light themes"), ("Palette", "Light themes")],
        );

        let above_accessible = before(&host, "Accessible contrast");
        let moved = dropped(&mut host, "Light themes", &above_accessible);
        assert_eq!(
            moved.unwrap(),
            [
                "Light themes +2",
                "  Daylight",
                "  Palette",
                "Accessible contrast",
                "Recipes"
            ]
        );
        let order: Vec<String> = host
            .projects()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(
            order[..3],
            ["Light themes", "Daylight", "Palette"],
            "the host's order keeps the block together"
        );
    }

    #[test]
    fn a_loose_project_dropped_on_a_project_is_filed_under_it_as_its_last_child() {
        let names = [
            "Sponsorships",
            "Sponsor tiers",
            "Recipes",
            "Redo sponsor page",
        ];
        let (_directory, mut host) = listed(&names);
        file(&mut host, &[("Sponsor tiers", "Sponsorships")]);
        let sponsorships = id(&host, "Sponsorships");

        assert_eq!(
            landed(
                &host,
                "Redo sponsor page",
                &Drop::Under(sponsorships.clone())
            ),
            Some(Landing::Refile {
                project: id(&host, "Redo sponsor page"),
                parent: Some(sponsorships.clone()),
                then: Some(ProjectMove {
                    project_ids: vec![id(&host, "Redo sponsor page")],
                    before: Some(id(&host, "Recipes")),
                }),
            })
        );
        assert_eq!(
            dropped(&mut host, "Redo sponsor page", &Drop::Under(sponsorships)).unwrap(),
            [
                "Sponsorships +2",
                "  Sponsor tiers",
                "  Redo sponsor page",
                "Recipes"
            ]
        );
        // A loose project with no children becomes a parent the same way.
        let onto_sponsorships = Drop::Under(id(&host, "Sponsorships"));
        assert_eq!(
            dropped(&mut host, "Recipes", &onto_sponsorships).unwrap(),
            [
                "Sponsorships +3",
                "  Sponsor tiers",
                "  Redo sponsor page",
                "  Recipes"
            ]
        );
    }

    #[test]
    fn a_loose_project_dropped_among_a_parents_children_is_filed_there() {
        let names = ["Sponsorships", "Tiers", "Thanks page", "Redo sponsor page"];
        let (_directory, mut host) = listed(&names);
        file(
            &mut host,
            &[("Tiers", "Sponsorships"), ("Thanks page", "Sponsorships")],
        );

        let above_thanks = before(&host, "Thanks page");
        assert_eq!(
            dropped(&mut host, "Redo sponsor page", &above_thanks).unwrap(),
            [
                "Sponsorships +3",
                "  Tiers",
                "  Redo sponsor page",
                "  Thanks page"
            ]
        );
    }

    #[test]
    fn a_child_dropped_among_loose_projects_becomes_loose_there() {
        let names = [
            "Light themes",
            "Generated palette check",
            "Recipes",
            "Billing",
        ];
        let (_directory, mut host) = listed(&names);
        file(&mut host, &[("Generated palette check", "Light themes")]);

        assert_eq!(
            landed(&host, "Generated palette check", &before(&host, "Billing")),
            Some(Landing::Refile {
                project: id(&host, "Generated palette check"),
                parent: None,
                then: Some(ProjectMove {
                    project_ids: vec![id(&host, "Generated palette check")],
                    before: Some(id(&host, "Billing")),
                }),
            })
        );
        let above_billing = before(&host, "Billing");
        assert_eq!(
            dropped(&mut host, "Generated palette check", &above_billing).unwrap(),
            [
                "Light themes",
                "Recipes",
                "Generated palette check",
                "Billing"
            ]
        );
    }

    #[test]
    fn no_target_shows_where_the_host_would_refuse() {
        let names = [
            "Accessible contrast",
            "Contrast warnings",
            "Sponsorships",
            "Recipes",
        ];
        let (_directory, mut host) = listed(&names);
        file(&mut host, &[("Contrast warnings", "Accessible contrast")]);
        let snapshot = joined(&host);
        let sections = sections(&snapshot, false);
        let none = BTreeSet::new();
        let aims = |dragged: &str, over: &str| {
            aims(
                &sections,
                &project(&host, dragged),
                &project(&host, over),
                &none,
            )
        };

        // A parent never lands on another project, only beside it.
        let parent_over_loose = aims("Accessible contrast", "Sponsorships").unwrap();
        assert_eq!(parent_over_loose.onto, None);
        let sponsorships = id(&host, "Sponsorships");
        assert_eq!(
            landed(
                &host,
                "Accessible contrast",
                &Drop::Under(sponsorships.clone())
            ),
            None
        );
        // Over a child, a parent lands only below the whole block.
        assert_eq!(
            aims("Sponsorships", "Contrast warnings").map(|a| a.onto),
            Some(None),
            "nothing lands on a child"
        );
        let below_block = Aims::only(before(&host, "Sponsorships"));
        let accessible = id(&host, "Accessible contrast");
        assert_eq!(
            aims("Accessible contrast", "Contrast warnings"),
            Some(below_block)
        );
        // A child lands on no project, and a loose project never on itself.
        assert_eq!(
            aims("Contrast warnings", "Sponsorships").unwrap().onto,
            None
        );
        assert_eq!(
            landed(
                &host,
                "Contrast warnings",
                &Drop::Under(sponsorships.clone())
            ),
            None
        );
        assert_eq!(aims("Recipes", "Recipes").unwrap().onto, None);
        assert_eq!(
            aims("Recipes", "Accessible contrast").unwrap().onto,
            Some(Drop::Under(accessible))
        );
    }

    #[test]
    fn a_release_lands_by_where_on_the_row_it_is() {
        let drop = |name: &str| Some(Drop::Before(name.into()));
        let refused = Aims {
            above: drop("a"),
            onto: None,
            below: drop("b"),
        };
        assert_eq!(refused.at(0.2), drop("a"));
        assert_eq!(refused.at(0.8), drop("b"));
        // A middle it may not drop onto lands nowhere, rather than beside the row.
        assert_eq!(refused.at(0.4), None);
        assert_eq!(refused.at(0.6), None);
        let filing = Aims {
            onto: Some(Drop::Under("a".into())),
            ..refused
        };
        assert_eq!(filing.at(0.5), Some(Drop::Under("a".into())));
    }

    #[test]
    fn nothing_dragged_lands_in_another_space() {
        let (_directory, host) = in_spaces(&SPACED);
        let snapshot = joined(&host);
        let sections = sections(&snapshot, false);
        let [sky, trial, garden] =
            ["Sky contrast", "Trial reminder copy", "Garden planner"].map(|n| project(&host, n));
        let sage = host.spaces().unwrap()[2].space.id.clone();
        let none = BTreeSet::new();

        assert_eq!(aims(&sections, &sky, &trial, &none), None);
        assert_eq!(aims(&sections, &sky, &garden, &none), None);
        let land = |drop: Drop| landing(&snapshot, &sections, &sky, &drop);
        assert_eq!(land(Drop::Before(trial.0.clone())), None);
        assert_eq!(land(Drop::Under(trial.0.clone())), None);
        assert_eq!(land(Drop::End(sage)), None);
    }

    #[test]
    fn a_move_within_a_space_leaves_the_other_spaces_order_alone() {
        let (_directory, mut host) = in_spaces(&SPACED);
        let organize = before(&host, "Organize the list");

        dropped(&mut host, "Sky contrast", &organize).unwrap();

        let order: Vec<String> = host
            .projects()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(
            order,
            [
                "Trial reminder copy",
                "Sky contrast",
                "Organize the list",
                "Garden planner",
                "Repository owners",
                "Reading list"
            ]
        );
    }

    #[test]
    fn drops_reckon_with_the_rows_shown_while_archived_projects_keep_their_place() {
        let (_directory, mut host) = listed(&["x", "old", "y", "gone"]);
        for session in host.sessions().unwrap() {
            if ["old", "gone"].contains(&session.name.as_str()) {
                host.set_archived(&session.id, true).unwrap();
            }
        }
        let snapshot = joined(&host);
        let shown = sections(&snapshot, false);
        let (x, hidden) = (project(&host, "x"), project(&host, "old"));
        // A hidden project is no row: nothing lands beside it, and it is not dragged.
        assert_eq!(aims(&shown, &x, &hidden, &BTreeSet::new()), None);
        assert_eq!(
            landing(&snapshot, &shown, &hidden, &Drop::End(home(&host))),
            None
        );
        // The very end is the end of the stored order, below the hidden projects too.
        let end = Drop::End(home(&host));
        dropped(&mut host, "x", &end).unwrap();
        let order: Vec<String> = host
            .projects()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(order, ["old", "y", "gone", "x"]);
    }

    #[test]
    fn a_dragged_row_scrolls_the_list_only_near_its_edges_and_faster_nearer_them() {
        let (top, bottom) = (100., 500.);
        assert_eq!(edge_scroll(300., top, bottom), 0.);
        assert_eq!(edge_scroll(100. + SCROLL_ZONE, top, bottom), 0.);
        assert_eq!(edge_scroll(500. - SCROLL_ZONE, top, bottom), 0.);
        // Just inside a zone still moves, so a resting pointer reaches the end.
        assert_eq!(edge_scroll(131., top, bottom), -1.);
        assert_eq!(edge_scroll(469., top, bottom), 1.);
        assert_eq!(edge_scroll(116., top, bottom), -SCROLL_STEP / 2.);
        assert_eq!(edge_scroll(100., top, bottom), -SCROLL_STEP);
        assert_eq!(edge_scroll(500., top, bottom), SCROLL_STEP);
        // Past either edge the pointer has left the list.
        assert_eq!(edge_scroll(99., top, bottom), 0.);
        assert_eq!(edge_scroll(501., top, bottom), 0.);
    }

    #[test]
    fn a_release_on_the_drags_own_row_is_the_click_it_would_have_been_or_a_cancel() {
        assert_eq!(own_release(false, 1), OwnRelease::Click);
        assert_eq!(own_release(false, 2), OwnRelease::Rename);
        // A third click is not another rename.
        assert_eq!(own_release(false, 3), OwnRelease::Click);
        for click_count in [1, 2] {
            assert_eq!(own_release(true, click_count), OwnRelease::Nothing);
        }
    }
}
