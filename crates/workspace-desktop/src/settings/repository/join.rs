//! Joining a repository another Mac set up: what it holds beside this Mac, both ways to join,
//! and afterwards what this Mac still needs.
use super::*;
use chrono::{DateTime, Local};

const STEPS: [&str; 3] = ["1 · Name it", "2 · Compare", "3 · What this Mac needs"];
/// How this Mac joined, which says whose settings are in use.
pub(crate) enum Joined {
    /// With the repository's settings, which the machine `from` last changed.
    Repository { from: String },
    /// With this Mac's, which replaced the repository's.
    ThisMac,
}

const KEPT: &str = "This Mac’s current settings are kept in a backup, and its skill history is added to the repository’s, so nothing is lost.";

impl Settings {
    pub(super) fn join_view(
        &self,
        section: Div,
        check: &SettingsRepositoryCheck,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(joining) = &check.joining else {
            return section;
        };
        let last = format!(
            "Last changed by {}, {}.",
            joining.last_machine,
            day_and_time(joining.last_changed_at)
        );
        let mut table = div().flex().flex_col().text_size(px(12.)).child(
            comparison_row(["SETTING", "IN THE REPOSITORY", "ON THIS MAC"], p)
                .text_size(px(10.))
                .text_color(p.subtle),
        );
        for setting in &joining.settings {
            let same = setting.repository == setting.this_machine;
            let this_machine = if same { "Same" } else { &setting.this_machine };
            let row = comparison_row([&setting.setting, &setting.repository, this_machine], p);
            table = table.child(match same {
                true => row.text_color(p.subtle),
                false => row,
            });
        }
        let card = repository_card(
            &check.repository,
            check.organization,
            pill("Set up", p.green),
            Some(last),
            p,
        );
        section
            .child(steps(2, p))
            .child(heading("Not syncing", p.subtle, p))
            .child(card)
            .child(table)
            .child(hint(KEPT, p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        button("replace-repository")
                            .link()
                            .label("Replace the repository’s settings with this Mac’s…")
                            .disabled(self.asking)
                            .on_click(
                                cx.listener(|v, _, _, c| v.confirm(Some(Confirm::Replace), c)),
                            ),
                    )
                    .child(
                        button("use-repository")
                            .primary()
                            .label("Use the repository’s settings")
                            .disabled(self.asking)
                            .on_click(cx.listener(|v, _, _, c| v.join(c))),
                    ),
            )
    }

    fn join(&mut self, cx: &mut Context<Self>) {
        if let Some((input, _)) = &self.checked {
            let repository = input.clone();
            let with = JoinWith::Repository;
            self.ask_repository(Command::JoinSettingsRepository { repository, with }, cx);
        }
    }

    /// Whose settings are in use after joining, and what this Mac still needs to run them.
    pub(super) fn joined(&self, how: &Joined, shown: &SyncShown, p: Palette) -> Vec<Div> {
        let needs = needs(shown);
        let in_use = in_use(how, !needs.is_empty());
        let needs = (needs.into_iter()).map(|(icon_name, what, why)| need(icon_name, what, why, p));
        [hint(in_use, p)].into_iter().chain(needs).collect()
    }
}

/// Whose settings are in use, and that this Mac still needs something when it does.
fn in_use(how: &Joined, needs_more: bool) -> String {
    let in_use = match how {
        Joined::Repository { from } => format!("Your settings from {from} are in use."),
        Joined::ThisMac => "This Mac’s settings are now the repository’s.".into(),
    };
    match needs_more {
        true => format!("{in_use} This Mac still needs:"),
        false => in_use,
    }
}

/// "1 · Name it", "2 · Compare" and "3 · What this Mac needs", with `current` marked.
pub(super) fn steps(current: usize, p: Palette) -> Div {
    let step = |(index, label): (usize, &'static str)| {
        let on = index + 1 == current;
        div()
            .px_2()
            .rounded_full()
            .border_1()
            .border_color(if on { p.focus } else { p.edge.opacity(0.6) })
            .text_color(if on { p.text } else { p.subtle })
            .child(label)
    };
    div()
        .flex()
        .gap_1p5()
        .text_size(px(11.))
        .children(STEPS.into_iter().enumerate().map(step))
}

/// "today 14:41", or "12 Oct 14:41" on another day.
fn day_and_time(at: i64) -> String {
    let Some(at) = DateTime::from_timestamp_millis(at).map(|at| at.with_timezone(&Local)) else {
        return String::new();
    };
    match at.date_naive() == Local::now().date_naive() {
        true => at.format("today %H:%M").to_string(),
        false => at.format("%-d %b %H:%M").to_string(),
    }
}

fn comparison_row(cells: [&str; 3], p: Palette) -> Div {
    let [setting, repository, this_machine] = cells.map(|cell| SharedString::from(cell.to_owned()));
    div()
        .flex()
        .gap_2()
        .py_1()
        .border_b_1()
        .border_color(p.edge.opacity(0.25))
        .child(
            div()
                .w(px(150.))
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .child(setting),
        )
        .child(div().flex_1().min_w_0().child(repository))
        .child(div().flex_1().min_w_0().child(this_machine))
}

/// What this Mac lacks to run the shared settings, each with an icon and why.
fn needs(shown: &SyncShown) -> Vec<(&'static str, StillNeed, Option<&'static str>)> {
    let mut needs: Vec<_> = (shown.still_needs.iter())
        .map(|need| ("alert", need.clone(), None))
        .collect();
    if !shown.owners_allowed {
        let owners = StillNeed {
            text: "Owners allowed for pull requests".into(),
            page: WorkspacePage::Repositories,
        };
        needs.push((
            "laptop",
            owners,
            Some("These stay on each Mac. This one allows none yet, so every push asks first."),
        ));
    }
    needs
}

/// What one card lacks, why, and a link to the main window's page that provides it.
fn need(icon_name: &'static str, what: StillNeed, why: Option<&'static str>, p: Palette) -> Div {
    let (label, open): (_, Box<dyn Action>) = match what.page {
        WorkspacePage::Models => ("Open Models", Box::new(OpenModels)),
        WorkspacePage::Repositories => ("Open Repositories", Box::new(OpenRepositories)),
    };
    let link = button(SharedString::from(format!("need-{}", what.text)))
        .link()
        .label(label)
        .on_click(move |_, window, cx| window.dispatch_action(open.boxed_clone(), cx));
    let color = if icon_name == "alert" {
        p.yellow
    } else {
        p.subtle
    };
    div()
        .p_2()
        .flex()
        .items_start()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(p.edge.opacity(0.55))
        .bg(p.surface)
        .text_size(px(12.))
        .child(icon(icon_name).size(px(14.)).mt(px(2.)).text_color(color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::MEDIUM).child(what.text))
                .children(why.map(|why| hint(why, p))),
        )
        .child(link)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{Joined, SyncShown, day_and_time, in_use, needs};
    use workspace_core::{StillNeed, WorkspacePage};

    #[test]
    fn after_joining_the_section_lists_what_this_mac_still_needs() {
        let held = StillNeed {
            text: "On from another machine, off on this Mac: \"Try to resolve conflicts\" needs a PR keeper model: choose one in Models first".into(),
            page: WorkspacePage::Models,
        };
        let shown = SyncShown {
            still_needs: vec![held.clone()],
            ..SyncShown::default()
        };
        let listed = needs(&shown);
        assert_eq!(listed[0].1, held);
        let owners = &listed[1].1;
        assert_eq!(
            (&*owners.text, owners.page),
            (
                "Owners allowed for pull requests",
                WorkspacePage::Repositories
            )
        );
        let allowed = SyncShown {
            owners_allowed: true,
            ..shown
        };
        assert_eq!(needs(&allowed).len(), 1);
    }

    #[test]
    fn the_repositorys_last_change_is_a_time_in_milliseconds() {
        let now = chrono::Utc::now().timestamp_millis();
        assert!(day_and_time(now).starts_with("today "));
        assert!(!day_and_time(now - 3 * 24 * 60 * 60_000).starts_with("today "));
    }

    #[test]
    fn replacing_the_repositorys_settings_does_not_say_the_other_macs_are_in_use() {
        let from = Joined::Repository {
            from: "Mike’s MacBook Pro".into(),
        };
        assert_eq!(
            in_use(&from, false),
            "Your settings from Mike’s MacBook Pro are in use."
        );
        assert_eq!(
            in_use(&Joined::ThisMac, true),
            "This Mac’s settings are now the repository’s. This Mac still needs:"
        );
    }
}
