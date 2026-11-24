//! Where the selected host's disk space goes, as Settings › Disk, the Agents panel and the
//! Inbox's disk cards show it: the last `DiskUse` the host answered, read again in the
//! background every few minutes and after each reclaim, and the wording those views share.
use super::*;
use std::{collections::HashMap, time::Duration};
use workspace_core::{DiskSettings, DiskUse, GB, PendingDelete, TicketDiskState, TicketDiskUse};

/// How often the reading is refreshed while the app runs. The host measures at most every
/// five minutes, so reading more often finds nothing new.
const REFRESH: Duration = Duration::from_secs(3 * 60);
/// How soon the reading is refreshed while a reclaim is under way, so its row settles.
const FOLLOW_RECLAIM: Duration = Duration::from_secs(5);

/// The last disk use the selected host answered.
#[derive(Default)]
pub(crate) struct DiskReading {
    /// The paired host it came from, or `None` for This Mac's.
    host: Option<String>,
    disk: Option<DiskUse>,
    /// A read to follow a reclaim is on its way.
    following: bool,
}
impl Global for DiskReading {}

fn host_id(bridge: &Bridge) -> Option<String> {
    bridge.remote().map(|r| r.id.clone())
}

/// The selected host's disk use, once it has answered.
pub(crate) fn current(cx: &App) -> Option<&DiskUse> {
    let reading = cx.try_global::<DiskReading>()?;
    let selected = host_id(&cx.global::<Hosts>().bridge());
    (reading.host == selected).then_some(reading.disk.as_ref()?)
}

/// Reads the selected host's disk use now and then every few minutes while the app runs.
pub(crate) fn watch(cx: &mut App) {
    cx.set_global(DiskReading::default());
    cx.spawn(async move |cx| {
        loop {
            cx.update(|cx| ask(ask::disk_use(), None, cx));
            cx.background_executor().timer(REFRESH).await;
        }
    })
    .detach();
}

/// Sends `request` to the selected host and keeps the disk use it answers with, redrawing every
/// window. An error is shown in `window`, if given, or else logged.
pub(crate) fn ask(request: Ask<DiskUse>, window: Option<AnyWindowHandle>, cx: &mut App) {
    let bridge = cx.global::<Hosts>().bridge();
    let host = host_id(&bridge);
    let command = request.command().clone();
    let reply = match bridge.ask(request) {
        Ok(reply) => reply,
        Err(error) => return report(error, window, cx),
    };
    cx.spawn(async move |cx| {
        let answer = reply.await;
        cx.update(|cx| {
            shared::replied(&command, answer.is_ok(), cx);
            match answer {
                Ok(disk) => {
                    let reclaiming =
                        (disk.tickets.iter()).any(|t| t.state == TicketDiskState::Reclaiming);
                    let reading = cx.default_global::<DiskReading>();
                    let follow = reclaiming && !reading.following;
                    *reading = DiskReading {
                        host,
                        disk: Some(disk),
                        following: reading.following || follow,
                    };
                    if follow {
                        follow_reclaim(cx);
                    }
                    cx.refresh_windows();
                }
                Err(error) => report(error, window, cx),
            }
        });
    })
    .detach();
}

/// Reads again shortly, while a reclaim the last reading showed is under way.
fn follow_reclaim(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FOLLOW_RECLAIM).await;
        cx.update(|cx| {
            cx.default_global::<DiskReading>().following = false;
            ask(ask::disk_use(), None, cx);
        });
    })
    .detach();
}

fn report(error: String, window: Option<AnyWindowHandle>, cx: &mut App) {
    let Some(window) = window else {
        return eprintln!("Disk use: {error}");
    };
    let _ = window.update(cx, |_, window, cx| toast_error(error, window, cx));
}

/// A size as the mockup words it: "152 MB", "8.1 GB", "31 GB".
pub(crate) fn size(bytes: u64) -> String {
    const MB: u64 = 1_000_000;
    match bytes {
        b if b >= 10 * GB => format!("{} GB", (b as f64 / GB as f64).round()),
        b if b >= GB => format!("{:.1} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{} MB", (b as f64 / MB as f64).round()),
        b if b >= 1_000 => format!("{} KB", b / 1_000),
        b => format!("{b} B"),
    }
}

/// A Reclaim button shows for a ticket with at least 1 GB of build output and no build running
/// or reclaim under way there.
pub(crate) fn can_reclaim(ticket: &TicketDiskUse) -> bool {
    let busy = matches!(
        ticket.state,
        TicketDiskState::Building | TicketDiskState::Reclaiming
    );
    !busy && ticket.build_output >= GB
}

/// The Agents panel's quiet line: "8.1 GB build output · 152 MB source", or once reclaimed
/// "Build output reclaimed · 149 MB source".
pub(crate) fn panel_line(ticket: &TicketDiskUse) -> String {
    let output = match ticket.state {
        TicketDiskState::Reclaimed => "Build output reclaimed".to_owned(),
        _ => format!("{} build output", size(ticket.build_output)),
    };
    format!("{output} · {} source", size(ticket.source))
}

/// The line under the build output tile: how far over the budget it is and how many tickets
/// are being reclaimed, or how much of the budget is left. `true` when over.
pub(crate) fn budget_line(disk: &DiskUse, settings: &DiskSettings) -> (String, bool) {
    let budget = settings.budget();
    if disk.build_output <= budget {
        return (
            format!("{} left in the budget", size(budget - disk.build_output)),
            false,
        );
    }
    let over = format!("{} over", size(disk.build_output - budget));
    let reclaiming = (disk.tickets.iter())
        .filter(|t| t.state == TicketDiskState::Reclaiming)
        .count();
    let line = match reclaiming {
        0 => over,
        n => format!("{over} · reclaiming {}", wording::counted(n, "idle ticket")),
    };
    (line, true)
}

/// How a ticket's state reads in the table, and how it is colored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Busy,
    Quiet,
    Working,
    Done,
    Failing,
}
pub(crate) fn state_label(ticket: &TicketDiskUse) -> (String, Tone) {
    match ticket.state {
        TicketDiskState::Building => ("building".into(), Tone::Busy),
        TicketDiskState::Idle => ("idle".into(), Tone::Quiet),
        TicketDiskState::Reclaiming => ("reclaiming".into(), Tone::Working),
        TicketDiskState::Reclaimed => (format!("reclaimed {}", size(ticket.reclaimed)), Tone::Done),
        TicketDiskState::FailingToDelete => ("delete failing".into(), Tone::Failing),
    }
}

/// A row's small line, when the space is outside the worktree's own build folders or a folder
/// was left alone: "target/ 4.8 GB · scratch 7.2 GB", "build/ · Left alone: it holds tracked
/// files". `worktree` is the ticket's, so folders inside it read relative to it.
pub(crate) fn where_line(ticket: &TicketDiskUse, worktree: &str) -> Option<String> {
    fn inside<'a>(path: &'a str, worktree: &str) -> Option<&'a str> {
        path.strip_prefix(worktree)?.strip_prefix('/')
    }
    let name = |path: &str| -> String {
        if let Some(relative) = inside(path, worktree) {
            return format!("{relative}/");
        }
        let derived = path.split_once("/DerivedData/").map(|(_, name)| name);
        match derived {
            Some(name) => format!("DerivedData/{name}"),
            None => "scratch".into(),
        }
    };
    let outside = (ticket.parts.iter()).any(|part| inside(&part.path, worktree).is_none());
    if !outside && ticket.left_alone.is_empty() {
        return None;
    }
    let mut parts: Vec<_> = ticket.parts.iter().filter(|part| part.size > 0).collect();
    parts.sort_by_key(|part| std::cmp::Reverse(part.size));
    let found = parts
        .into_iter()
        .map(|part| format!("{} {}", name(&part.path), size(part.size)));
    let left =
        (ticket.left_alone.iter()).map(|part| format!("{} · {}", name(&part.path), part.note));
    Some(found.chain(left).collect::<Vec<_>>().join(" · "))
}

/// One project's tickets in the table, largest build output first.
pub(crate) struct Group<'a> {
    pub(crate) project_id: &'a str,
    pub(crate) tickets: Vec<&'a TicketDiskUse>,
}

/// The table's groups: one per project, the project with the most build output first, and in
/// each its tickets largest first.
pub(crate) fn groups(disk: &DiskUse) -> Vec<Group<'_>> {
    let mut by_project: HashMap<&str, Vec<&TicketDiskUse>> = HashMap::new();
    for ticket in &disk.tickets {
        by_project
            .entry(&ticket.project_id)
            .or_default()
            .push(ticket);
    }
    let largest_first = |t: &&TicketDiskUse| std::cmp::Reverse((t.build_output, t.source));
    let mut groups: Vec<Group> = (by_project.into_iter())
        .map(|(project_id, mut tickets)| {
            tickets.sort_by_key(largest_first);
            Group {
                project_id,
                tickets,
            }
        })
        .collect();
    let total = |g: &Group| g.tickets.iter().map(|t| t.build_output).sum::<u64>();
    groups.sort_by(|a, b| total(b).cmp(&total(a)).then(a.project_id.cmp(b.project_id)));
    groups
}

/// A waiting delete's small line: "Permission denied · 4 attempts · next in 2m".
pub(crate) fn delete_line(delete: &PendingDelete) -> String {
    let error = delete
        .last_error
        .as_deref()
        .unwrap_or("Waiting to be deleted");
    let mut line = error.to_owned();
    if delete.attempts > 0 {
        line += &format!(
            " · {}",
            wording::counted(delete.attempts as usize, "attempt")
        );
    }
    format!(
        "{line} · next {}",
        wording::from_now(delete.next_attempt_at)
    )
}

/// An Inbox card the disk work raises, from its operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiskCard {
    /// Free space is under the hard floor, so builds wait.
    BuildsHeld,
    /// A folder could not be deleted; the folder's name. A name the host shortened matches no
    /// waiting folder, so its card offers only Dismiss.
    DeleteFailed(String),
}
impl DiskCard {
    pub(crate) fn of(operation_id: &str) -> Option<Self> {
        if operation_id.starts_with(BUILDS_HELD_OPERATION) {
            return Some(Self::BuildsHeld);
        }
        let rest = operation_id.strip_prefix(DELETE_FAILED_OPERATION)?;
        let (_, name) = rest.split_once(':')?;
        Some(Self::DeleteFailed(name.to_owned()))
    }
}

/// The waiting delete a failing-delete card names, while it still waits.
fn failing_delete<'a>(disk: &'a DiskUse, name: &str) -> Option<&'a PendingDelete> {
    (disk.deletes.iter()).find(|delete| {
        let folder = std::path::Path::new(&delete.path).file_name();
        folder.is_some_and(|folder| folder.to_string_lossy() == name)
    })
}

/// A button on a disk card, in the order drawn; the last is the primary one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CardAction {
    OpenSettings,
    ReclaimAll,
    Dismiss,
    /// The folder waiting to be deleted.
    ShowInFinder(String),
    RetryNow(String),
}

/// What a disk card offers. The hard floor's card opens Settings › Disk or reclaims all but
/// running builds; the host answers it once space returns. A failing delete's card can be
/// dismissed, and while its folder still waits, shown in Finder on This Mac or retried now.
pub(crate) fn card_actions(
    card: &DiskCard,
    disk: Option<&DiskUse>,
    on_this_mac: bool,
) -> Vec<CardAction> {
    let name = match card {
        DiskCard::BuildsHeld => return vec![CardAction::OpenSettings, CardAction::ReclaimAll],
        DiskCard::DeleteFailed(name) => name,
    };
    let mut actions = vec![CardAction::Dismiss];
    if let Some(waiting) = disk.and_then(|disk| failing_delete(disk, name)) {
        if on_this_mac {
            actions.push(CardAction::ShowInFinder(waiting.path.clone()));
        }
        actions.push(CardAction::RetryNow(waiting.path.clone()));
    }
    actions
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        CardAction, DiskCard, budget_line, can_reclaim, card_actions, delete_line, groups,
        panel_line, size, where_line,
    };
    use workspace_core::{
        DiskPart, DiskSettings, DiskUse, GB, PendingDelete, TicketDiskState, TicketDiskUse,
    };

    fn ticket(id: &str, project: &str, build_output: u64, state: TicketDiskState) -> TicketDiskUse {
        TicketDiskUse {
            ticket_id: id.into(),
            project_id: project.into(),
            repository_id: "r".into(),
            title: id.into(),
            source: 150_000_000,
            build_output,
            parts: vec![],
            left_alone: vec![],
            last_turn_at: None,
            state,
            reclaimed_at: None,
            reclaimed: 0,
        }
    }
    fn part(path: &str, size: u64, note: &str) -> DiskPart {
        DiskPart {
            path: path.into(),
            size,
            note: note.into(),
        }
    }

    #[test]
    fn sizes_read_as_the_mockup_writes_them() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(152_000_000), "152 MB");
        assert_eq!(size(8_100_000_000), "8.1 GB");
        assert_eq!(size(31_200_000_000), "31 GB");
    }

    #[test]
    fn tickets_group_by_project_largest_first_in_each() {
        let disk = DiskUse {
            tickets: vec![
                ticket("small", "a", 4 * GB, TicketDiskState::Idle),
                ticket("only", "b", 31 * GB, TicketDiskState::Idle),
                ticket("large", "a", 12 * GB, TicketDiskState::Idle),
                ticket("middle", "a", 8 * GB, TicketDiskState::Building),
            ],
            ..DiskUse::default()
        };
        let groups = groups(&disk);
        let drawn: Vec<(&str, Vec<&str>)> = (groups.iter())
            .map(|g| {
                (
                    g.project_id,
                    g.tickets.iter().map(|t| t.title.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            drawn,
            [("b", vec!["only"]), ("a", vec!["large", "middle", "small"])]
        );
    }

    #[test]
    fn reclaim_shows_only_for_a_ticket_not_building_with_at_least_a_gigabyte() {
        use TicketDiskState::*;
        assert!(can_reclaim(&ticket("t", "p", 12 * GB, Idle)));
        assert!(can_reclaim(&ticket("t", "p", GB, Reclaimed)));
        assert!(!can_reclaim(&ticket("t", "p", 12 * GB, Building)));
        assert!(!can_reclaim(&ticket("t", "p", 12 * GB, Reclaiming)));
        assert!(!can_reclaim(&ticket("t", "p", GB - 1, Idle)));
    }

    #[test]
    fn the_budget_line_says_how_far_over_and_what_is_being_reclaimed() {
        let settings = DiskSettings::default();
        let mut disk = DiskUse {
            build_output: 131 * GB,
            tickets: vec![
                ticket("a", "p", 31 * GB, TicketDiskState::Reclaiming),
                ticket("b", "p", 100 * GB, TicketDiskState::Building),
            ],
            ..DiskUse::default()
        };
        assert_eq!(
            budget_line(&disk, &settings),
            ("31 GB over · reclaiming 1 idle ticket".into(), true)
        );
        disk.tickets[0].state = TicketDiskState::Idle;
        assert_eq!(budget_line(&disk, &settings), ("31 GB over".into(), true));
        disk.build_output = 60 * GB;
        assert_eq!(
            budget_line(&disk, &settings),
            ("40 GB left in the budget".into(), false)
        );
    }

    #[test]
    fn the_panel_line_says_reclaimed_once_it_is() {
        let mut t = ticket("t", "p", 8_100_000_000, TicketDiskState::Idle);
        assert_eq!(panel_line(&t), "8.1 GB build output · 150 MB source");
        t.state = TicketDiskState::Reclaimed;
        assert_eq!(panel_line(&t), "Build output reclaimed · 150 MB source");
    }

    #[test]
    fn the_small_line_names_space_outside_the_worktree_and_what_was_left_alone() {
        let mut t = ticket("t", "p", 12 * GB, TicketDiskState::Idle);
        t.parts = vec![part("/w/target", 4_800_000_000, "")];
        assert_eq!(where_line(&t, "/w"), None, "only target-style folders");
        t.parts.push(part("/tmp/scratch-t", 7_200_000_000, ""));
        assert_eq!(
            where_line(&t, "/w").as_deref(),
            Some("scratch 7.2 GB · target/ 4.8 GB")
        );
        t.parts = vec![part(
            "/Library/Developer/Xcode/DerivedData/app-a1b2",
            3_100_000_000,
            "",
        )];
        assert_eq!(
            where_line(&t, "/w").as_deref(),
            Some("DerivedData/app-a1b2 3.1 GB")
        );
        t.parts = vec![];
        t.left_alone = vec![part("/w/build", 0, "Left alone: it holds tracked files")];
        assert_eq!(
            where_line(&t, "/w").as_deref(),
            Some("build/ · Left alone: it holds tracked files")
        );
    }

    #[test]
    fn disk_cards_are_known_by_their_operation_and_offer_their_actions() {
        use CardAction::*;
        let held = DiskCard::of("builds-held:17").unwrap();
        assert_eq!(held, DiskCard::BuildsHeld);
        assert_eq!(card_actions(&held, None, true), [OpenSettings, ReclaimAll]);
        let failed = DiskCard::of("worktree-delete-failed:17:.removing-t").unwrap();
        assert_eq!(failed, DiskCard::DeleteFailed(".removing-t".into()));
        assert_eq!(DiskCard::of("permission:1"), None);
        let pending = |path: &str| PendingDelete {
            path: path.into(),
            ticket_id: None,
            attempts: 4,
            last_error: Some("Permission denied".into()),
            next_attempt_at: 0,
            failing: true,
        };
        // An older reclaim of the same ticket waits first, under a longer name.
        let folder = "/w/tasks/p/r/.removing-t".to_owned();
        let disk = DiskUse {
            deletes: vec![pending("/w/tasks/p/r/.removing-t-16-0"), pending(&folder)],
            ..DiskUse::default()
        };
        assert_eq!(
            card_actions(&failed, Some(&disk), true),
            [
                Dismiss,
                ShowInFinder(folder.clone()),
                RetryNow(folder.clone())
            ]
        );
        // A paired host's folder is not on this Mac to show.
        assert_eq!(
            card_actions(&failed, Some(&disk), false),
            [Dismiss, RetryNow(folder)]
        );
        // Once the folder is gone, or before the host answered, there is nothing to retry.
        let other = DiskCard::DeleteFailed(".removing-other".into());
        assert_eq!(card_actions(&other, Some(&disk), true), [Dismiss]);
        assert_eq!(card_actions(&failed, None, true), [Dismiss]);
        assert!(delete_line(&disk.deletes[0]).starts_with("Permission denied · 4 attempts · next"));
    }
}
