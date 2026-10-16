//! The Settings repository section: naming a repository and setting it up or joining it, sync's
//! status and stopping it, and the sync mark the sidebar footer shows.
mod choose;
mod join;
mod synced;

use super::*;
use gpui_component::button::ButtonVariants;
pub(crate) use synced::{Mark, SyncShown, share_sync};
use ui::pill;

/// The section's place among the scrolled body's children: three sections and their rules.
pub(super) const SECTION: usize = 6;

/// What asks first; Cancel sends nothing.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Confirm {
    SetUp,
    Replace,
    Stop,
}

fn owner(repository: &SettingsRepository) -> &str {
    repository.name.split('/').next().unwrap_or_default()
}

/// Who can read the repository, and whether that reaches past the human and their invitees.
fn who_can_read(repository: &SettingsRepository) -> (String, bool) {
    let owner = owner(repository);
    let changes =
        "Its owners can also change it, and so change your agents’ instructions on every Mac.";
    match repository.readers {
        Readers::Invited => (
            "Only you and people you invite can read or change it.".into(),
            false,
        ),
        Readers::EveryMember => (
            format!(
                "Every member of {owner} can read it: the organization’s base permission is at least Read. {changes}"
            ),
            true,
        ),
        Readers::EnterpriseMembers => (
            format!(
                "Every member of {owner}’s enterprise can read it: the repository is internal. {changes}"
            ),
            true,
        ),
        Readers::MaybeMembers => (
            format!(
                "Members of {owner} may be able to read it, depending on the organization’s settings."
            ),
            false,
        ),
    }
}

/// The question, then what it means. Choosing a repository is what lets Wiffletree push to it,
/// so both ways of choosing one name it, its branch and who can read it.
fn confirmation(confirm: Confirm, repository: &SettingsRepository) -> Vec<String> {
    let (name, branch) = (&repository.name, &repository.branch);
    let then =
        "then pushes there by itself a few seconds after each change, and pulls every 5 minutes.";
    let only = format!(
        "It pushes only to this repository’s {branch} branch, only its own files, and never by force. Owners you allowed for pull requests aren’t affected, and don’t cover this."
    );
    let readers = who_can_read(repository).0;
    match confirm {
        Confirm::SetUp => vec![
            format!("Sync your settings with {name}?"),
            format!("Wiffletree adds this Mac’s shared settings to {branch} in one commit, {then}"),
            only,
            readers,
        ],
        Confirm::Replace => vec![
            format!("Replace the settings in {name} with this Mac’s?"),
            format!(
                "Wiffletree pushes this Mac’s shared settings over the repository’s to {branch} in one commit, and the repository’s stay in its history. It {then}"
            ),
            only,
            readers,
        ],
        Confirm::Stop => vec![
            format!("Stop syncing with {name}?"),
            "This Mac keeps every setting as it is and stops pulling and pushing. Nothing changes on GitHub.".into(),
            "The notes about paused settings close. To sync again, name the repository and join it.".into(),
        ],
    }
}

impl Settings {
    pub(super) fn repository_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let shown = cx.try_global::<SyncShown>().cloned().unwrap_or_default();
        let chosen = (self.host.as_ref()).and_then(|h| h.settings_repository.as_ref());
        let section = div().flex().flex_col().gap_2();
        let section = match (chosen, &shown.status, &self.checked) {
            (Some(_), Some(status), _) => self.synced_view(section, status, &shown, p, cx),
            (Some(_), None, _) => section.child(heading("…", p.subtle, p)),
            (None, _, Some((_, check))) if check.joining.is_some() => {
                self.join_view(section, check, p, cx)
            }
            (None, ..) => self.choose_view(section, p, cx),
        };
        section
            .children((self.repository_error.clone()).map(|error| hint(error, p).text_color(p.red)))
    }

    /// Asks the host about the repository, or acts on it, and shows its answer or error.
    fn ask_repository(&mut self, command: Command, cx: &mut Context<Self>) {
        let joined_from = match (&command, &self.checked) {
            (Command::JoinSettingsRepository { .. }, Some((_, check))) => {
                (check.joining.as_ref()).map(|joining| joining.last_machine.clone())
            }
            _ => None,
        };
        let checking = matches!(command, Command::CheckSettingsRepository { .. });
        let answers_settings = !checking && !matches!(command, Command::SyncNow);
        self.repository_error = None;
        let receiver = match self.bridge.request(command) {
            Ok(receiver) => receiver,
            Err(error) => return self.repository_error = Some(error),
        };
        self.asking = true;
        let input = self.repository_input.read(cx).value().to_string();
        cx.spawn(async move |this, cx| {
            let answer =
                (receiver.recv().await).unwrap_or_else(|_| Err("Host disconnected".into()));
            let _ = this.update(cx, |view, cx| {
                view.asking = false;
                let shown = answer.and_then(|value| {
                    if checking {
                        let check = serde_json::from_value(value).map_err(|e| e.to_string())?;
                        view.checked = Some((input, check));
                    } else if answers_settings {
                        let settings = serde_json::from_value(value).map_err(|e| e.to_string())?;
                        view.host = Some(settings);
                        view.checked = None;
                        view.joined_from = joined_from;
                    }
                    Ok(())
                });
                match shown {
                    Ok(()) if !checking => view.refresh_sync(cx),
                    Ok(()) => {}
                    Err(error) => view.repository_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm(&mut self, confirm: Option<Confirm>, cx: &mut Context<Self>) {
        self.confirming = confirm;
        cx.notify();
    }

    /// The open confirmation over the window; Cancel only closes it.
    pub(super) fn confirm_dialog(
        &self,
        confirm: Confirm,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let act = button("confirm-repository");
        let (repository, command, act) = match confirm {
            Confirm::SetUp | Confirm::Replace => {
                let (input, check) = self.checked.as_ref()?;
                let repository = input.clone();
                match confirm {
                    Confirm::SetUp => (
                        &check.repository,
                        Command::SetUpSettingsRepository { repository },
                        act.primary().label("Set up and sync"),
                    ),
                    _ => (
                        &check.repository,
                        Command::JoinSettingsRepository {
                            repository,
                            with: JoinWith::ThisMachine,
                        },
                        act.danger().label("Replace and sync"),
                    ),
                }
            }
            Confirm::Stop => (
                self.host.as_ref()?.settings_repository.as_ref()?,
                Command::StopSettingsSync,
                act.danger().label("Stop syncing"),
            ),
        };
        let mut text = confirmation(confirm, repository).into_iter();
        let ask = text.next().unwrap_or_default();
        let act = act.on_click(cx.listener(move |v, _, _, c| {
            v.confirm(None, c);
            v.ask_repository(command.clone(), c);
        }));
        Some(
            div()
                .w(px(400.))
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_xl()
                .border_1()
                .border_color(p.edge)
                .bg(p.surface)
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(ask),
                )
                .children(text.map(|line| hint(line, p)))
                .child(
                    div()
                        .pt_2()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("cancel-repository")
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(|v, _, _, c| v.confirm(None, c))),
                        )
                        .child(act),
                ),
        )
    }
}

fn heading(state: impl Into<SharedString>, color: Hsla, p: Palette) -> Div {
    section("SETTINGS REPOSITORY", p).child(pill(state, color))
}

/// The repository's name, its kind or state, and who can read it.
fn repository_card(
    repository: &SettingsRepository,
    organization: bool,
    kind: Div,
    more: Option<String>,
    p: Palette,
) -> Div {
    let (who, warn) = who_can_read(repository);
    let who = match more {
        Some(more) => format!("{who} {more}"),
        None => who,
    };
    let (who_icon, who_color, icon_color) = match warn {
        true => ("alert", p.yellow, p.yellow),
        false => ("lock", p.text, p.subtle),
    };
    div()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(p.edge.opacity(0.7))
        .bg(p.surface)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    icon(if organization { "people" } else { "lock" })
                        .size(px(14.))
                        .text_color(p.subtle),
                )
                .child(
                    mono(repository.name.clone(), p)
                        .flex_1()
                        .text_size(px(12.))
                        .text_color(p.text),
                )
                .child(kind),
        )
        .child(
            div()
                .flex()
                .items_start()
                .gap_2()
                .text_size(px(12.))
                .text_color(who_color)
                .child(
                    icon(who_icon)
                        .size(px(14.))
                        .mt(px(2.))
                        .text_color(icon_color),
                )
                .child(div().flex_1().child(who)),
        )
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{Confirm, confirmation, who_can_read};
    use workspace_core::*;

    #[test]
    fn choosing_a_repository_asks_naming_it_its_branch_and_who_can_read_it() {
        let mut repository = SettingsRepository {
            name: "MHohlios/wiffletree-storage".into(),
            push_url: "https://github.com/MHohlios/wiffletree-storage.git".into(),
            branch: "main".into(),
            readers: Readers::Invited,
        };
        for confirm in [Confirm::SetUp, Confirm::Replace] {
            let text = confirmation(confirm, &repository).join(" ");
            for said in [
                "MHohlios/wiffletree-storage",
                "to main",
                "pushes there by itself",
                "only to this repository’s main branch",
                "Owners you allowed for pull requests aren’t affected, and don’t cover this.",
                "Only you and people you invite can read",
            ] {
                assert!(text.contains(said), "{said} in {text}");
            }
        }
        repository.name = "Sage-Social/wiffletree-storage".into();
        repository.readers = Readers::EveryMember;
        let (who, warn) = who_can_read(&repository);
        assert!(
            warn && who.starts_with("Every member of Sage-Social can read it"),
            "{who}"
        );
        repository.readers = Readers::MaybeMembers;
        let (who, warn) = who_can_read(&repository);
        assert!(
            !warn && who.contains("Members of Sage-Social may be able to read it"),
            "{who}"
        );
    }
}
