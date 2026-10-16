//! The chosen repository: sync's status, the paused settings, Sync now and Stop syncing, and
//! the sync mark the sidebar footer shows.
use super::*;
use chrono::{DateTime, Local};
use ui::banner;

const AN_HOUR_MS: i64 = 60 * 60_000;

/// What the Settings window shows of the latest snapshot, which the main window receives.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct SyncShown {
    pub(super) status: Option<SyncStatus>,
    pub(super) notes: Vec<WorkspaceNote>,
    pub(super) still_needs: Vec<String>,
    pub(super) owners_allowed: bool,
}
impl Global for SyncShown {}

/// Hands a snapshot's sync status to the Settings window, which redraws only on a change.
pub(crate) fn share_sync(snapshot: &Snapshot, cx: &mut App) {
    let shown = SyncShown {
        status: snapshot.settings_sync.clone(),
        notes: snapshot.workspace_notes.clone(),
        still_needs: snapshot.still_needs.clone(),
        owners_allowed: !snapshot.allowed_owners.is_empty(),
    };
    if cx.try_global::<SyncShown>() != Some(&shown) {
        cx.set_global(shown);
    }
}

/// The sync mark's state, the most pressing first.
#[derive(Debug, PartialEq)]
pub(crate) enum Mark {
    Stopped,
    /// GitHub has been out of reach for more than an hour.
    Failing,
    Paused(usize),
    Syncing,
    Waiting,
    Synced,
}
impl Mark {
    pub(crate) fn of(status: &SyncStatus, paused: usize, now: i64) -> Self {
        let failing = |since: i64| now - since > AN_HOUR_MS;
        match () {
            _ if status.stopped => Self::Stopped,
            _ if status.failing_since.is_some_and(failing) => Self::Failing,
            _ if paused > 0 => Self::Paused(paused),
            _ if status.syncing => Self::Syncing,
            _ if status.error.is_some() => Self::Waiting,
            _ => Self::Synced,
        }
    }
    pub(crate) fn icon(&self) -> &'static str {
        match self {
            Self::Stopped => "alert",
            Self::Failing | Self::Waiting => "cloud-off",
            Self::Paused(_) => "pause",
            Self::Syncing => "refresh",
            Self::Synced => "check",
        }
    }
    /// Yellow while a setting is paused or sync has stopped, red once GitHub has been out of
    /// reach for an hour; otherwise the mark stays quiet.
    pub(crate) fn tint(&self, p: Palette) -> Option<Hsla> {
        match self {
            Self::Stopped | Self::Paused(_) => Some(p.yellow),
            Self::Failing => Some(p.red),
            _ => None,
        }
    }
    pub(crate) fn label(&self, status: &SyncStatus, now: i64) -> String {
        match self {
            Self::Stopped => "Sync stopped".into(),
            Self::Failing => {
                let since = status.synced_at.or(status.failing_since).unwrap_or(now);
                format!("Not synced for {}", hours(now - since))
            }
            Self::Paused(n) => format!("{} paused", count(*n, "setting")),
            Self::Syncing => "Syncing".into(),
            Self::Waiting if status.waiting > 0 => {
                format!("{} waiting", count(status.waiting, "change"))
            }
            Self::Waiting => "Waiting to reach GitHub".into(),
            Self::Synced => "Synced".into(),
        }
    }
    pub(crate) fn tooltip(&self, status: &SyncStatus, notes: &[WorkspaceNote]) -> String {
        let name = &status.repository.name;
        match (self, notes) {
            (Self::Stopped, _) => status.error.clone().unwrap_or_default(),
            (Self::Failing | Self::Waiting, _) => format!(
                "Couldn’t reach GitHub at {}. Next try {}.",
                clock(status.failed_at),
                clock(status.next_pull_at)
            ),
            (Self::Paused(_), [note]) => paused_reason(note),
            (Self::Paused(n), _) => format!("{n} settings wait for you to choose."),
            (Self::Syncing, _) => format!("Syncing settings with {name}"),
            (Self::Synced, _) => match status.synced_at {
                Some(at) => format!("Settings synced at {} with {name}", clock(Some(at))),
                None => format!("Settings sync with {name}"),
            },
        }
    }
    /// The section's heading names the state rather than what waits.
    fn heading(&self, status: &SyncStatus, now: i64) -> String {
        match self {
            Self::Waiting => "Waiting to reach GitHub".into(),
            _ => self.label(status, now),
        }
    }
}

impl Settings {
    /// Reads the sync status again, for when the main window is not there to.
    pub(in crate::settings) fn refresh_sync(&self, cx: &mut Context<Self>) {
        let Ok(receiver) = self.bridge.request(Command::Snapshot) else {
            return;
        };
        cx.spawn(async move |_, cx| {
            let snapshot = receiver.recv().await.ok().and_then(Result::ok);
            let snapshot = snapshot.and_then(|value| serde_json::from_value(value).ok());
            if let Some(snapshot) = snapshot {
                cx.update(|cx| share_sync(&snapshot, cx));
            }
        })
        .detach();
    }

    /// What this Mac still needs after joining, sync's status, the paused settings, Sync now and
    /// Stop syncing.
    pub(super) fn synced_view(
        &self,
        section: Div,
        status: &SyncStatus,
        shown: &SyncShown,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let now = ui::now();
        let mark = Mark::of(status, shown.notes.len(), now);
        let color = match mark {
            Mark::Synced => p.green,
            Mark::Syncing => p.subtle,
            Mark::Failing => p.red,
            _ => p.yellow,
        };
        let section = section
            .when(self.joined.is_some(), |d| d.child(join::steps(3, p)))
            .child(heading(mark.heading(status, now), color, p))
            .children(
                (self.joined.as_ref())
                    .map(|how| self.joined(how, shown, p))
                    .into_iter()
                    .flatten(),
            )
            .children(
                status
                    .error
                    .as_ref()
                    .map(|error| error_banner(error, status, p)),
            )
            .child(rows(status, p))
            .children(shown.notes.iter().map(|note| paused_row(note, p)))
            .when(!shown.notes.is_empty(), |d| {
                d.child(hint("Every other setting keeps syncing.", p))
            });
        section.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    button("stop-syncing")
                        .link()
                        .label("Stop syncing…")
                        .disabled(self.asking)
                        .on_click(cx.listener(|v, _, _, c| v.confirm(Some(Confirm::Stop), c))),
                )
                .child(
                    button("sync-now")
                        .ghost()
                        .icon(icon("refresh").size(px(12.)))
                        .label("Sync now")
                        .disabled(self.asking || status.syncing)
                        .on_click(cx.listener(|v, _, _, c| v.ask_repository(Command::SyncNow, c))),
                ),
        )
    }
}

/// Why the latest pass failed: Git's words when GitHub could not be reached.
fn error_banner(error: &str, status: &SyncStatus, p: Palette) -> Div {
    if status.stopped {
        return banner(p.yellow).child(error.to_owned());
    }
    let reached = format!(
        "Couldn’t reach GitHub at {}. Git said:",
        clock(status.failed_at)
    );
    banner(p.red).child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(reached)
            .child(
                div()
                    .font_family(".SystemMonoFont")
                    .text_size(px(11.))
                    .child(error.to_owned()),
            ),
    )
}

/// The repository, who can read it, the last and next sync, what waits, and this Mac.
fn rows(status: &SyncStatus, p: Palette) -> Div {
    let repository = &status.repository;
    let name = format!("{} · {}", repository.name, repository.branch);
    let last = match (status.synced_at, &status.error) {
        (None, _) => "Not yet".into(),
        (at, Some(_)) => clock(at),
        (at, None) => format!("{} · {}", clock(at), moved(status.pulled, status.pushed)),
    };
    let next = clock(status.next_pull_at);
    let next = match (&status.error, status.stopped) {
        (None, _) => ("Next pull", next),
        (Some(_), true) => ("Next try", next),
        (Some(_), false) => (
            "Next try",
            format!("{next} · tries less often while GitHub is out of reach"),
        ),
    };
    let waiting = (status.waiting > 0).then(|| {
        let uses = if status.waiting == 1 { "it" } else { "them" };
        let changes = count(status.waiting, "change");
        row(
            "Waiting",
            format!("{changes} to push. This Mac already uses {uses}."),
            p,
        )
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(row("Repository", name, p))
        .child(row("Who can read it", readers(repository), p))
        .child(row("Last sync", last, p))
        .children(waiting)
        .child(row(next.0, next.1, p))
        .child(row("This Mac", status.machine.clone(), p))
}

fn row(label: &'static str, value: String, p: Palette) -> Div {
    div()
        .flex()
        .gap_3()
        .text_size(px(12.))
        .child(
            div()
                .w(px(110.))
                .flex_none()
                .text_color(p.subtle)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// A paused setting and why, in the note's words.
fn paused_row(note: &WorkspaceNote, p: Palette) -> Div {
    div()
        .p_2()
        .flex()
        .items_start()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(p.yellow.opacity(0.4))
        .bg(p.yellow.opacity(0.07))
        .text_size(px(12.))
        .child(icon("pause").size(px(14.)).mt(px(2.)).text_color(p.yellow))
        // The note's text names the setting, so the row says it once.
        .child(div().flex_1().min_w_0().child(note.text.clone()))
}

fn paused_reason(note: &WorkspaceNote) -> String {
    let setting = &note.setting;
    match &note.paused {
        Paused::TwoMachines { .. } => format!("{setting} changed on both Macs."),
        Paused::OutsideWiffletree => format!("{setting} was changed outside Wiffletree."),
        Paused::CannotBeRead { machine, .. } => {
            format!("{setting} from {machine} can’t be used here.")
        }
    }
}

/// Who can read the repository, as the status's short line.
fn readers(repository: &SettingsRepository) -> String {
    let owner = owner(repository);
    match repository.readers {
        Readers::Invited => "Only you and people you invite".into(),
        Readers::EveryMember => format!("Every member of {owner}"),
        Readers::EnterpriseMembers => format!("Every member of {owner}’s enterprise"),
        Readers::MaybeMembers => format!("Maybe members of {owner}"),
    }
}

/// "1 change", "3 changes".
fn count(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        n => format!("{n} {noun}s"),
    }
}

/// "3 hours", or days past two of them.
fn hours(ms: i64) -> String {
    let hours = (ms / AN_HOUR_MS).max(1) as usize;
    match hours {
        ..48 => count(hours, "hour"),
        _ => count(hours / 24, "day"),
    }
}

/// "14:32" on this Mac's clock.
fn clock(at: Option<i64>) -> String {
    let at = at.and_then(DateTime::from_timestamp_millis);
    at.map_or("—".into(), |at| {
        at.with_timezone(&Local).format("%H:%M").to_string()
    })
}

/// "pulled 2 changes, pushed 1".
fn moved(pulled: usize, pushed: usize) -> String {
    match (pulled, pushed) {
        (0, 0) => "nothing to move".into(),
        (n, 0) => format!("pulled {}", count(n, "change")),
        (0, m) => format!("pushed {}", count(m, "change")),
        (n, m) => format!("pulled {}, pushed {m}", count(n, "change")),
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{AN_HOUR_MS, Mark};
    use crate::palette::palette;
    use workspace_core::*;

    fn status() -> SyncStatus {
        SyncStatus {
            repository: SettingsRepository {
                name: "MHohlios/wiffletree-storage".into(),
                push_url: "https://github.com/MHohlios/wiffletree-storage.git".into(),
                branch: "main".into(),
                readers: Readers::Invited,
            },
            synced_at: Some(0),
            pulled: 2,
            pushed: 1,
            next_pull_at: Some(0),
            error: None,
            failed_at: None,
            failing_since: None,
            stopped: false,
            syncing: false,
            waiting: 0,
            machine: "Mike’s MacBook Pro".into(),
        }
    }

    #[test]
    fn the_mark_is_quiet_while_sync_works_yellow_when_paused_or_stopped_and_red_after_an_hour() {
        let now = 10 * AN_HOUR_MS;
        let p = palette(false);
        let synced = status();
        assert_eq!(Mark::of(&synced, 0, now), Mark::Synced);
        assert_eq!(Mark::of(&synced, 0, now).tint(p), None);
        let paused = Mark::of(&synced, 1, now);
        assert_eq!(
            (paused.label(&synced, now), paused.tint(p)),
            ("1 setting paused".into(), Some(p.yellow))
        );
        let waiting = SyncStatus {
            error: Some("Could not resolve host: github.com".into()),
            failing_since: Some(now - 20 * 60_000),
            waiting: 1,
            ..status()
        };
        let mark = Mark::of(&waiting, 0, now);
        assert_eq!(
            (mark.label(&waiting, now), mark.tint(p)),
            ("1 change waiting".into(), None)
        );
        let failing = SyncStatus {
            synced_at: Some(now - 3 * AN_HOUR_MS),
            failing_since: Some(now - 2 * AN_HOUR_MS),
            ..waiting
        };
        let mark = Mark::of(&failing, 1, now);
        assert_eq!(
            (mark.label(&failing, now), mark.tint(p)),
            ("Not synced for 3 hours".into(), Some(p.red))
        );
        let stopped = SyncStatus {
            stopped: true,
            ..failing
        };
        assert_eq!(Mark::of(&stopped, 0, now).tint(p), Some(p.yellow));
    }

    #[test]
    fn a_paused_file_is_named_in_the_marks_tooltip() {
        let note = WorkspaceNote {
            path: "skills/implementing/".into(),
            setting: "Implementing · your rules".into(),
            paused: Paused::TwoMachines {
                machine: "Mike’s MacBook Air".into(),
            },
            text: String::new(),
        };
        let tooltip = Mark::Paused(1).tooltip(&status(), &[note]);
        assert_eq!(tooltip, "Implementing · your rules changed on both Macs.");
    }
}
