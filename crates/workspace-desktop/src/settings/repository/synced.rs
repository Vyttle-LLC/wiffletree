//! The chosen repository: sync's status, the paused settings, Sync now and Stop syncing, and
//! the sync mark the sidebar footer shows.
use super::*;
use crate::workspace_notes::{self, NoteAction, OnNote};
use chrono::{DateTime, Local};
use ui::banner;

const AN_HOUR_MS: i64 = 60 * 60_000;

/// What the Settings window shows of the latest snapshot, which the main window receives.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct SyncShown {
    pub(super) status: Option<SyncStatus>,
    pub(super) notes: Vec<PausedSetting>,
    pub(super) still_needs: Vec<StillNeed>,
    pub(super) owners_allowed: bool,
}
impl Global for SyncShown {}

/// Hands a snapshot's sync status to the Settings window, which redraws only on a change.
pub(crate) fn share_sync(snapshot: &Snapshot, cx: &mut App) {
    let shown = SyncShown {
        status: snapshot.settings_sync.clone(),
        notes: snapshot.paused_settings().cloned().collect(),
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
    /// Chosen, and no pass has run since: joining waits for the next pull.
    NotYet,
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
            _ if status.synced_at.is_none() => Self::NotYet,
            _ => Self::Synced,
        }
    }
    pub(crate) fn icon(&self) -> &'static str {
        match self {
            Self::Stopped => "alert",
            Self::Failing | Self::Waiting => "cloud-off",
            Self::Paused(_) => "pause",
            Self::Syncing | Self::NotYet => "refresh",
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
            Self::Waiting if !status.waiting.is_empty() => {
                format!("{} waiting", count(status.waiting.len(), "change"))
            }
            Self::Waiting => "Waiting to reach GitHub".into(),
            Self::NotYet => "Not synced yet".into(),
            Self::Synced => "Synced".into(),
        }
    }
    pub(crate) fn tooltip(&self, status: &SyncStatus, notes: &[PausedSetting]) -> String {
        let name = &status.repository.name;
        match (self, notes) {
            (Self::Stopped, _) => status.error.clone().unwrap_or_default(),
            (Self::Failing | Self::Waiting, _) => format!(
                "Couldn’t reach GitHub at {}. Next try {}.",
                clock(status.failed_at),
                clock(status.next_pull_at)
            ),
            (Self::Paused(_), [note]) => format!("{} See your Inbox.", paused_reason(note)),
            (Self::Paused(n), _) => format!("{n} settings wait for you to choose. See your Inbox."),
            (Self::Syncing, _) => format!("Syncing settings with {name}"),
            (Self::NotYet, _) => format!(
                "Settings sync with {name} from the next pull, at {}",
                clock(status.next_pull_at)
            ),
            (Self::Synced, _) => {
                format!("Settings synced at {} with {name}", clock(status.synced_at))
            }
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
            Mark::Syncing | Mark::NotYet => p.subtle,
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
            .children(shown.notes.iter().map(|note| self.paused_row(note, p, cx)))
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
    let waiting = (!status.waiting.is_empty()).then(|| row("Waiting", waiting(status), p));
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

/// "1 change: Check-ins. This Mac already uses it."
fn waiting(status: &SyncStatus) -> String {
    let settings = &status.waiting;
    let changes = count(settings.len(), "change");
    let uses = if settings.len() == 1 { "it" } else { "them" };
    format!(
        "{changes}: {}. This Mac already uses {uses}.",
        settings.join(", ")
    )
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

impl Settings {
    /// A paused setting and why; Compare opens both versions and the note's choices.
    fn paused_row(&self, note: &PausedSetting, p: Palette, cx: &mut Context<Self>) -> Div {
        let view = cx.entity().downgrade();
        let on: OnNote = Rc::new(move |path, action, _, cx| {
            let _ = view.update(cx, |view, cx| view.note_action(path, action, cx));
        });
        let (icon_name, why, compare) = match &note.paused {
            Paused::TwoMachines { machine } => (
                "pause",
                format!("Changed on this Mac and on {machine}. This Mac uses its own."),
                "Compare",
            ),
            Paused::OutsideWiffletree => (
                "pencil",
                "Changed outside Wiffletree on this Mac.".into(),
                "Compare",
            ),
            Paused::CannotBeRead { machine, reason } => (
                "alert",
                format!(
                    "The version from {machine} can’t be used here: {reason}. This Mac keeps its own."
                ),
                "Details",
            ),
        };
        let compared = &self.compared;
        let comparison = workspace_notes::comparison(note, compared, p);
        let open = comparison.is_some();
        div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(p.yellow.opacity(0.4))
            .bg(p.yellow.opacity(0.07))
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        icon(icon_name)
                            .size(px(14.))
                            .mt(px(2.))
                            .text_color(p.yellow),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(note.setting.clone()),
                            )
                            .child(hint(why, p).text_size(px(11.5))),
                    )
                    .child(workspace_notes::compare_link(
                        note, compare, "Hide", compared, &on,
                    )),
            )
            .children(comparison)
            .when(open, |d| {
                d.child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .children(workspace_notes::settle_buttons(note, &on)),
                )
            })
            .children(
                (workspace_notes::kept_note(note, compared))
                    .map(|kept| hint(kept, p).text_size(px(11.))),
            )
    }

    /// Redraws for a new sync status, asking again for an open comparison it may have changed.
    pub(in crate::settings) fn sync_shown(&mut self, cx: &mut Context<Self>) {
        let notes = (cx.try_global::<SyncShown>()).map_or(vec![], |shown| shown.notes.clone());
        if let Some(command) = workspace_notes::recompare(&mut self.compared, &notes) {
            self.ask_note(command, cx);
        }
        cx.notify();
    }

    /// Opens or hides a paused setting's comparison, or settles it as its note would.
    fn note_action(&mut self, path: String, action: NoteAction, cx: &mut Context<Self>) {
        let command = match action {
            NoteAction::Compare => workspace_notes::toggle_compare(&mut self.compared, path),
            NoteAction::Settle(settle) => Some(Command::SettlePausedFile { path, settle }),
        };
        cx.notify();
        if let Some(command) = command {
            self.ask_note(command, cx);
        }
    }

    /// Asks the host for a comparison's lines or to settle a note, and shows its answer.
    fn ask_note(&mut self, command: Command, cx: &mut Context<Self>) {
        self.repository_error = None;
        let asked = command.clone();
        let receiver = match self.bridge.request(command.clone()) {
            Ok(receiver) => receiver,
            Err(error) => return self.repository_error = Some(error),
        };
        cx.spawn(async move |this, cx| {
            let answer =
                (receiver.recv().await).unwrap_or_else(|_| Err("Host disconnected".into()));
            let _ = this.update(cx, |view, cx| {
                let shown = answer.and_then(|value| match command {
                    Command::ComparePausedFile { path } => {
                        let lines = serde_json::from_value(value).map_err(|e| e.to_string())?;
                        workspace_notes::compared_arrived(&mut view.compared, &path, lines);
                        Ok(())
                    }
                    _ => {
                        view.compared = None;
                        view.refresh_sync(cx);
                        Ok(())
                    }
                });
                if let Err(error) = shown {
                    workspace_notes::compare_failed(&mut view.compared, &asked);
                    view.repository_error = Some(error);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn paused_reason(note: &PausedSetting) -> String {
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
            waiting: vec![],
            machine: "Mike’s MacBook Pro".into(),
        }
    }

    #[test]
    fn the_mark_is_quiet_while_sync_works_yellow_when_paused_or_stopped_and_red_after_an_hour() {
        let now = 10 * AN_HOUR_MS;
        let p = palette(false);
        let synced = status();
        assert_eq!(Mark::of(&synced, 0, now), Mark::Synced);
        let joined = SyncStatus {
            synced_at: None,
            ..status()
        };
        assert_eq!(
            Mark::of(&joined, 0, now).label(&joined, now),
            "Not synced yet"
        );
        assert_eq!(Mark::of(&synced, 0, now).tint(p), None);
        let paused = Mark::of(&synced, 1, now);
        assert_eq!(
            (paused.label(&synced, now), paused.tint(p)),
            ("1 setting paused".into(), Some(p.yellow))
        );
        let waiting = SyncStatus {
            error: Some("Could not resolve host: github.com".into()),
            failing_since: Some(now - 20 * 60_000),
            waiting: vec!["Check-ins".into()],
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
        let note = PausedSetting {
            path: "skills/implementing/".into(),
            setting: "Implementing · your rules".into(),
            paused: Paused::TwoMachines {
                machine: "Mike’s MacBook Air".into(),
            },
            text: String::new(),
        };
        let tooltip = Mark::Paused(1).tooltip(&status(), &[note]);
        assert_eq!(
            tooltip,
            "Implementing · your rules changed on both Macs. See your Inbox."
        );
    }
}
