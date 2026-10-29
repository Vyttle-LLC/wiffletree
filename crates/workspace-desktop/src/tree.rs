//! The sidebar's shape: projects in the host's order, gathered into groups by first word, each
//! one row for its coordinator. Tickets and their agents are listed in the Agents panel.
use crate::agents_panel::Roster;
use crate::progress::ProgressBoard;
use std::collections::BTreeSet;
use workspace_core::*;

pub(crate) enum Row<'a> {
    Group(Group<'a>),
    Project {
        root: &'a Session,
        /// Its name; inside a group, without the first word the group already shows.
        label: String,
        grouped: bool,
    },
}

/// Projects whose names share a first word, drawn under one row while grouping is on.
#[derive(Debug)]
pub(crate) struct Group<'a> {
    /// The first word in lowercase: what the members share.
    pub key: String,
    /// The first word as the topmost member writes it.
    pub label: String,
    /// Their coordinators, in the host's order.
    pub members: Vec<&'a Session>,
}

/// A top-level row of the project list.
#[derive(Debug)]
pub(crate) enum Entry<'a> {
    Project(&'a Session),
    Group(Group<'a>),
}

/// A name's first word without trailing punctuation: "wiffletree" in "wiffletree: Sidebar".
fn first_word(name: &str) -> &str {
    let word = name.split_whitespace().next().unwrap_or_default();
    word.trim_end_matches(|c: char| !c.is_alphanumeric())
}

/// What projects in one group share: their first word, whatever its case.
pub(crate) fn group_key(name: &str) -> String {
    first_word(name).to_lowercase()
}

/// A grouped project's name without the first word and the separator after it, or the full
/// name when nothing else remains.
fn member_label(name: &str) -> &str {
    let rest = name
        .trim_start()
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim_start());
    // A separator standing on its own, as in "watch - Relay".
    let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let separator = |c: char| c.is_ascii_punctuation() || matches!(c, '–' | '—' | '·');
    let rest = if word.chars().all(separator) {
        after.trim_start()
    } else {
        rest
    };
    if rest.is_empty() { name } else { rest }
}

/// The shown projects in the host's order. With `grouped`, two or more that share a first word,
/// whatever its case, gather where the topmost of them is. Archived projects count only when
/// `show_archived`.
pub(crate) fn entries(
    index: &WorkspaceIndex,
    show_archived: bool,
    grouped: bool,
) -> Vec<Entry<'_>> {
    let mut groups: Vec<Group> = vec![];
    for project in &index.snapshot().projects {
        let Some(root) = (index.roots(&project.id)).find(|s| show_archived || !s.archived) else {
            continue;
        };
        let key = group_key(&root.name);
        match groups
            .iter_mut()
            .find(|group| grouped && !key.is_empty() && group.key == key)
        {
            Some(group) => group.members.push(root),
            None => groups.push(Group {
                key,
                label: first_word(&root.name).into(),
                members: vec![root],
            }),
        }
    }
    groups
        .into_iter()
        .map(|group| match group.members[..] {
            [root] => Entry::Project(root),
            _ => Entry::Group(group),
        })
        .collect()
}

/// The most urgent state inside a collapsed group.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Rollup {
    NeedsYou(usize),
    Stuck(usize),
    Blocked,
    Working,
}

/// A collapsed group's state, from the items in it that need the human, its stuck agents and
/// the status each member's row shows.
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

/// What a drag moves as one: a project by its id, or a group, by its key, with all its members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Movable {
    Project(String),
    Group(String),
}

/// Where a dragged row lands among the rows it can be reordered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Drop {
    Before(Movable),
    /// After the last member of the group with this key.
    EndOfGroup(String),
    /// After the last top-level row.
    End,
}

/// The host command's arguments for one drop.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ProjectMove {
    pub project_ids: Vec<String>,
    pub before: Option<String>,
}

impl Entry<'_> {
    pub(crate) fn movable(&self) -> Movable {
        match self {
            Entry::Project(root) => Movable::Project(root.project_id.clone()),
            Entry::Group(group) => Movable::Group(group.key.clone()),
        }
    }
    fn project_ids(&self) -> Vec<String> {
        match self {
            Entry::Project(root) => vec![root.project_id.clone()],
            Entry::Group(group) => group.project_ids(),
        }
    }
}
impl Group<'_> {
    fn project_ids(&self) -> Vec<String> {
        let ids = self.members.iter().map(|root| root.project_id.clone());
        ids.collect()
    }
}

/// The rows one row is reordered among: the top-level rows, or the members of one group.
struct Siblings<'a> {
    /// The group's key, when they are its members.
    group: Option<&'a str>,
    /// Each row with the projects it moves.
    rows: Vec<(Movable, Vec<String>)>,
}

impl<'a> Siblings<'a> {
    fn of(entries: &'a [Entry], row: &Movable) -> Option<Self> {
        if entries.iter().any(|entry| entry.movable() == *row) {
            let rows = entries
                .iter()
                .map(|entry| (entry.movable(), entry.project_ids()));
            return Some(Self {
                group: None,
                rows: rows.collect(),
            });
        }
        let Movable::Project(id) = row else {
            return None;
        };
        let group = entries.iter().find_map(|entry| match entry {
            Entry::Group(group) if group.members.iter().any(|m| m.project_id == *id) => Some(group),
            _ => None,
        })?;
        let rows = group
            .project_ids()
            .into_iter()
            .map(|id| (Movable::Project(id.clone()), vec![id]));
        Some(Self {
            group: Some(&group.key),
            rows: rows.collect(),
        })
    }
    fn position(&self, row: &Movable) -> Option<usize> {
        self.rows.iter().position(|(sibling, _)| sibling == row)
    }
}

/// Where a drop below `row` lands: before the row after it, or at the end of the rows it is
/// reordered among.
fn after(entries: &[Entry], row: &Movable) -> Option<Drop> {
    let siblings = Siblings::of(entries, row)?;
    let next = siblings.rows.get(siblings.position(row)? + 1);
    Some(match (next, siblings.group) {
        (Some((next, _)), _) => Drop::Before(next.clone()),
        (None, Some(key)) => Drop::EndOfGroup(key.into()),
        (None, None) => Drop::End,
    })
}

/// Where `dragged` lands when released over the upper and the lower half of the row `over`;
/// none when it may not land there. A grouped project stays among its group's members. A group
/// or an ungrouped project stays among the top-level rows, and over a group's member it lands
/// below that group.
pub(crate) fn drops(entries: &[Entry], dragged: &Movable, over: &Movable) -> Option<(Drop, Drop)> {
    let siblings = Siblings::of(entries, dragged)?;
    if siblings.position(over).is_some() {
        return Some((Drop::Before(over.clone()), after(entries, over)?));
    }
    let group = Movable::Group(Siblings::of(entries, over)?.group?.into());
    siblings.position(&group)?;
    let below = after(entries, &group)?;
    Some((below.clone(), below))
}

/// The move that puts `dragged` at `drop`; none when it is already there or may not land there.
pub(crate) fn project_move(
    entries: &[Entry],
    dragged: &Movable,
    drop: &Drop,
) -> Option<ProjectMove> {
    let siblings = Siblings::of(entries, dragged)?;
    let (group, rows) = (siblings.group, &siblings.rows);
    let from = siblings.position(dragged)?;
    let to = match drop {
        Drop::Before(target) => siblings.position(target)?,
        Drop::EndOfGroup(key) if group == Some(key.as_str()) => rows.len(),
        Drop::End if group.is_none() => rows.len(),
        _ => return None,
    };
    if to == from || to == from + 1 {
        return None;
    }
    let topmost = |at: usize| Some(rows.get(at)?.1[0].clone());
    if group.is_some() && to > from {
        // A group sits where its topmost member is, so that member moving down would take the
        // group with it. The members it passes move up instead, and the group stays put.
        let passed = rows[from + 1..to].iter().flat_map(|(_, ids)| ids.clone());
        return Some(ProjectMove {
            project_ids: passed.collect(),
            before: topmost(from),
        });
    }
    Some(ProjectMove {
        project_ids: rows[from].1.clone(),
        before: topmost(to),
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

/// Projects in the host's order, under their group's row while `grouped`; a collapsed group
/// hides its projects. Archived projects appear only when `show_archived`.
pub(crate) fn rows<'a>(
    index: &'a WorkspaceIndex,
    show_archived: bool,
    grouped: bool,
    collapsed_groups: &BTreeSet<String>,
) -> Vec<Row<'a>> {
    let mut rows = vec![];
    for entry in entries(index, show_archived, grouped) {
        let projects = match entry {
            Entry::Project(root) => vec![(root, root.name.clone(), false)],
            Entry::Group(group) => {
                let members = if collapsed_groups.contains(&group.key) {
                    vec![]
                } else {
                    let labelled = |root: &&'a Session| {
                        let label = member_label(&root.name);
                        (*root, label.to_owned(), true)
                    };
                    group.members.iter().map(labelled).collect()
                };
                rows.push(Row::Group(group));
                members
            }
        };
        for (root, label, grouped) in projects {
            rows.push(Row::Project {
                root,
                label,
                grouped,
            });
        }
    }
    rows
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
            assert_eq!(drawn(&snapshot, show_archived, true), ["Calls"]);
        }
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
        let none = BTreeSet::new();
        for show_archived in [false, true] {
            timed(
                &format!("tree rows, show archived {show_archived}"),
                &mut || {
                    std::hint::black_box(rows(&index, show_archived, true, &none));
                },
            );
        }
    }

    /// The project list as drawn: group rows with their count, their projects indented.
    fn drawn(index: &WorkspaceIndex, show_archived: bool, grouped: bool) -> Vec<String> {
        labels(&rows(index, show_archived, grouped, &BTreeSet::new()))
    }

    fn labels(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Group(group) => format!("{} ({})", group.label, group.members.len()),
                Row::Project {
                    label,
                    grouped: true,
                    ..
                } => format!("  {label}"),
                Row::Project { label, .. } => label.clone(),
            })
            .collect()
    }

    const MOCKUP: [&str; 9] = [
        "Trial reminder copy",
        "wiffletree: Need to reorder projects",
        "infra: Terraform state split",
        "watch: Call relay reconnect",
        "wiffletree: Repository owners",
        "ci-runners: Autoscaling",
        "Watch complications refresh",
        "infra: Neptune upgrade",
        "wiffletree: Compact context",
    ];

    #[test]
    fn projects_sharing_a_first_word_gather_where_the_topmost_of_them_is() {
        let (_directory, host) = listed(&MOCKUP);
        let snapshot = joined(&host);

        // Case and the colon do not matter, a lone first word stays an ordinary row, and
        // members keep the host's order under the word as their topmost member writes it.
        assert_eq!(
            drawn(&snapshot, false, true),
            [
                "Trial reminder copy",
                "wiffletree (3)",
                "  Need to reorder projects",
                "  Repository owners",
                "  Compact context",
                "infra (2)",
                "  Terraform state split",
                "  Neptune upgrade",
                "watch (2)",
                "  Call relay reconnect",
                "  complications refresh",
                "ci-runners: Autoscaling",
            ]
        );
        assert_eq!(drawn(&snapshot, false, false), MOCKUP, "grouping off");
    }

    #[test]
    fn a_first_word_ignores_case_and_trailing_punctuation() {
        let key = group_key;
        assert_eq!(key("Wiffletree: a"), key("wiffletree b"));
        assert_eq!(key("WATCH — relay"), key("watch, relay"));
        assert_eq!(first_word("ci-runners: Autoscaling"), "ci-runners");
        assert_ne!(key("wiffletree: a"), key("wiffle tree"));
        // Only whitespace ends the word, and a name with no word in front never groups.
        assert_eq!(first_word("infra:Neptune upgrade"), "infra:Neptune");
        assert_eq!(first_word("— untitled"), "");
        let (_directory, host) = listed(&["— one", "— two", "Wiffletree: a", "wiffletree b"]);
        assert_eq!(
            drawn(&joined(&host), false, true),
            ["— one", "— two", "Wiffletree (2)", "  a", "  b"]
        );
    }

    #[test]
    fn a_grouped_row_drops_the_word_and_its_separator_unless_nothing_remains() {
        assert_eq!(
            member_label("wiffletree: Repository owners"),
            "Repository owners"
        );
        assert_eq!(
            member_label("Watch complications refresh"),
            "complications refresh"
        );
        assert_eq!(member_label("watch - Relay (beta)"), "Relay (beta)");
        assert_eq!(member_label("watch — Relay"), "Relay");
        assert_eq!(member_label("infra #48 state"), "#48 state");
        // Only punctuation separates: what follows the word is kept, letters or not.
        assert_eq!(member_label("watch: 🚀"), "🚀");
        assert_eq!(member_label("watch 🚀 launch"), "🚀 launch");
        assert_eq!(member_label("infra: 2026"), "2026");
        for bare in ["wiffletree", "wiffletree:", "wiffletree -"] {
            assert_eq!(member_label(bare), bare);
        }
    }

    #[test]
    fn groups_form_over_the_projects_shown_so_archived_ones_join_only_when_shown() {
        let (_directory, mut host) = listed(&["infra: State split", "Billing", "infra: Neptune"]);
        let neptune = host.sessions().unwrap().remove(0);
        assert_eq!(neptune.name, "infra: Neptune");
        host.set_archived(&neptune.id, true).unwrap();
        let snapshot = joined(&host);

        assert_eq!(
            drawn(&snapshot, false, true),
            ["infra: State split", "Billing"]
        );
        assert_eq!(
            drawn(&snapshot, true, true),
            ["infra (2)", "  State split", "  Neptune", "Billing"]
        );
        assert_eq!(
            drawn(&snapshot, true, false),
            ["infra: State split", "Billing", "infra: Neptune"]
        );
    }

    #[test]
    fn a_collapsed_group_hides_its_projects_and_rolls_up_the_most_urgent_state() {
        let (_directory, host) = listed(&["infra: a", "Billing", "infra: b"]);
        let snapshot = joined(&host);
        let infra = BTreeSet::from(["infra".to_owned()]);

        let collapsed = rows(&snapshot, false, true, &infra);

        assert_eq!(labels(&collapsed), ["infra (2)", "Billing"]);
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

    /// The order the host would hold after `dragged` is dropped at `drop`.
    fn dropped(host: &mut Host, grouped: bool, dragged: &Movable, drop: &Drop) -> Option<String> {
        let snapshot = joined(host);
        let entries = entries(&snapshot, false, grouped);
        let moved = project_move(&entries, dragged, drop)?;
        let order = host
            .move_projects(&moved.project_ids, moved.before.as_deref())
            .unwrap();
        Some(order.iter().map(|p| p.name.as_str()).collect())
    }

    fn project(host: &Host, name: &str) -> Movable {
        let projects = host.projects().unwrap();
        Movable::Project(projects.into_iter().find(|p| p.name == name).unwrap().id)
    }

    #[test]
    fn with_grouping_off_any_project_drops_anywhere_in_the_flat_list() {
        let (_directory, mut host) = listed(&["a", "b", "c", "d"]);
        let [a, b, d] = ["a", "b", "d"].map(|name| project(&host, name));

        assert_eq!(
            dropped(&mut host, false, &d, &Drop::Before(a.clone())).as_deref(),
            Some("dabc")
        );
        assert_eq!(
            dropped(&mut host, false, &d, &Drop::End).as_deref(),
            Some("abcd")
        );
        // Onto itself or the place it already has: nothing to send.
        assert_eq!(dropped(&mut host, false, &d, &Drop::End), None);
        assert_eq!(
            dropped(&mut host, false, &a, &Drop::Before(a.clone())),
            None
        );
        assert_eq!(
            dropped(&mut host, false, &a, &Drop::Before(b.clone())),
            None
        );
        // The upper half of a row means before it; the lower half, before the next one, or
        // the end below the last.
        let snapshot = joined(&host);
        let flat = entries(&snapshot, false, false);
        let before = |row: &Movable| Drop::Before(row.clone());
        assert_eq!(drops(&flat, &d, &a), Some((before(&a), before(&b))));
        assert_eq!(drops(&flat, &a, &d), Some((before(&d), Drop::End)));
    }

    #[test]
    fn a_grouped_project_reorders_inside_its_group_and_the_group_stays_put() {
        let (_directory, mut host) = listed(&["w: 1", "x", "w: 2", "y", "w: 3"]);
        let [first, third, x] = ["w: 1", "w: 3", "x"].map(|name| project(&host, name));
        let end = Drop::EndOfGroup("w".into());

        // Up: it lands before the member it was dropped above.
        assert_eq!(
            dropped(&mut host, true, &third, &Drop::Before(first.clone())).as_deref(),
            Some("w: 3w: 1xw: 2y")
        );
        // Down from the top: the members it passes move up, so the group stays above "x".
        assert_eq!(
            dropped(&mut host, true, &third, &end).as_deref(),
            Some("w: 1w: 2w: 3xy")
        );
        assert_eq!(
            dropped(&mut host, true, &first, &Drop::Before(third.clone())).as_deref(),
            Some("w: 2w: 1w: 3xy")
        );
        assert_eq!(dropped(&mut host, true, &third, &end), None, "already last");

        // It cannot leave its group, and nothing from outside can land inside: over a member,
        // a top-level row lands below the whole group.
        let snapshot = joined(&host);
        let grouped = entries(&snapshot, false, true);
        let (group, before) = (Movable::Group("w".into()), |row: &Movable| {
            Drop::Before(row.clone())
        });
        assert_eq!(
            drops(&grouped, &third, &first),
            Some((before(&first), before(&third)))
        );
        assert_eq!(
            drops(&grouped, &first, &third),
            Some((before(&third), end.clone()))
        );
        assert_eq!(drops(&grouped, &third, &x), None);
        assert_eq!(drops(&grouped, &third, &group), None);
        assert_eq!(
            drops(&grouped, &x, &group),
            Some((before(&group), before(&x)))
        );
        assert_eq!(drops(&grouped, &x, &first), Some((before(&x), before(&x))));
        assert_eq!(project_move(&grouped, &third, &before(&x)), None);
        assert_eq!(project_move(&grouped, &third, &Drop::End), None);
        assert_eq!(project_move(&grouped, &x, &before(&first)), None);
        assert_eq!(project_move(&grouped, &x, &end), None);
    }

    #[test]
    fn drops_reckon_with_the_rows_shown_while_archived_projects_keep_their_place() {
        let (_directory, mut host) = listed(&["x", "w: 1", "old", "w: 2", "y", "gone"]);
        for session in host.sessions().unwrap() {
            if ["old", "gone"].contains(&session.name.as_str()) {
                host.set_archived(&session.id, true).unwrap();
            }
        }
        let [x, y] = ["x", "y"].map(|name| project(&host, name));
        let (group, hidden) = (Movable::Group("w".into()), project(&host, "old"));
        let snapshot = joined(&host);
        let shown = entries(&snapshot, false, true);
        // A hidden project is no row: nothing lands beside it, and it is not dragged.
        assert_eq!(drops(&shown, &x, &hidden), None);
        assert_eq!(project_move(&shown, &hidden, &Drop::End), None);
        assert_eq!(
            drops(&shown, &y, &x),
            Some((Drop::Before(x.clone()), Drop::Before(group.clone())))
        );

        // A top-level drop gathers the group above "x"; the archived project between its
        // members stays where it was.
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(x.clone())).as_deref(),
            Some("w: 1w: 2xoldygone")
        );
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::Before(group)).as_deref(),
            Some("yw: 1w: 2xoldgone")
        );
        // The very end is the end of the stored order, below the hidden project too.
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::End).as_deref(),
            Some("w: 1w: 2xoldgoney")
        );
        assert_eq!(
            dropped(&mut host, false, &x, &Drop::End).as_deref(),
            Some("w: 1w: 2oldgoneyx")
        );
    }

    #[test]
    fn a_dragged_group_takes_all_its_members_as_one_block() {
        let (_directory, mut host) = listed(&["x", "w: 1", "y", "w: 2", "z"]);
        let group = Movable::Group("w".into());

        let x = project(&host, "x");
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(x)).as_deref(),
            Some("w: 1w: 2xyz")
        );
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::End).as_deref(),
            Some("xyzw: 1w: 2")
        );
        // An ungrouped project lands above the group's topmost member.
        let y = project(&host, "y");
        assert_eq!(
            dropped(&mut host, true, &y, &Drop::Before(group.clone())).as_deref(),
            Some("xzyw: 1w: 2")
        );
        assert_eq!(
            dropped(&mut host, true, &group, &Drop::Before(y)).as_deref(),
            Some("xzw: 1w: 2y")
        );
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
