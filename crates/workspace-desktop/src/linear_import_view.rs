//! Import from Linear… in a Space's Storage and issues box (OpenSpec change `shared-issues`,
//! task 6.2): the human's Linear key, the team, the summary of what would be written with how
//! Linear's states map and the GitHub login confirmed for each Linear user, then the import's
//! progress from the snapshot. Every step is the human's; the host keeps the key only in the
//! Keychain and only until the import ends.
use super::*;
use gpui_component::{Disableable, Sizable, button::ButtonVariants, input::Input};
use ui::hint;
use wording::counted;

/// The panel asks to be closed.
pub(super) struct Closed;

enum Stage {
    Key,
    Teams(Vec<LinearTeam>),
    /// The summary, with a login field for each Linear user, in the plan's order.
    Plan(LinearImportPlan, Vec<Entity<InputState>>),
    Started,
}

pub(super) struct LinearImportPanel {
    bridge: Bridge,
    shared: Shared,
    space_id: SpaceId,
    key: Entity<InputState>,
    stage: Stage,
    busy: bool,
    error: Option<String>,
}
impl EventEmitter<Closed> for LinearImportPanel {}
impl LinearImportPanel {
    pub(super) fn new(
        bridge: Bridge,
        shared: Shared,
        space_id: SpaceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let key = cx.new(|cx| (InputState::new(window, cx).masked(true)).placeholder("lin_api_…"));
        Self {
            bridge,
            shared,
            space_id,
            key,
            stage: Stage::Key,
            busy: false,
            error: None,
        }
    }

    pub(super) fn share(&mut self, shared: Shared, cx: &mut Context<Self>) {
        self.shared = shared;
        cx.notify();
    }

    /// This Space's import as the host last reported it.
    fn status(&self) -> Option<&LinearImportStatus> {
        (self.shared.snapshot()?.linear_import.as_ref()).filter(|s| s.space_id == self.space_id)
    }

    fn list_teams(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key.read(cx).value().trim().to_owned();
        if key.is_empty() {
            self.error = Some("Paste a Linear personal API key".into());
            return cx.notify();
        }
        self.busy = true;
        let ask = ask::linear_teams(self.space_id.clone(), LinearKey(key));
        self.ask(ask, window, cx, |view, teams, window, cx| {
            view.busy = false;
            match teams {
                Ok(teams) => {
                    view.error = None;
                    view.stage = Stage::Teams(teams);
                    // Kept by the host now, so the field forgets it.
                    view.key.update(cx, |key, cx| key.set_value("", window, cx));
                }
                Err(error) => view.error = Some(error),
            }
            cx.notify();
        });
    }

    fn preview(&mut self, team_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        let ask = ask::preview_linear_import(self.space_id.clone(), team_id);
        self.ask(ask, window, cx, |view, plan, window, cx| {
            view.busy = false;
            match plan {
                Ok(plan) => {
                    view.error = None;
                    let logins = (plan.users.iter())
                        .map(|user| {
                            let login = user.login.clone().unwrap_or_default();
                            cx.new(|cx| {
                                let input = InputState::new(window, cx).placeholder("GitHub login");
                                input.default_value(login)
                            })
                        })
                        .collect();
                    view.stage = Stage::Plan(plan, logins);
                }
                Err(error) => view.error = Some(error),
            }
            cx.notify();
        });
    }

    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Stage::Plan(plan, fields) = &self.stage else {
            return;
        };
        let typed = fields
            .iter()
            .map(|field| field.read(cx).value().to_string());
        let logins = confirmed(&plan.users, typed);
        self.busy = true;
        let ask = ask::import_from_linear(self.space_id.clone(), logins);
        self.ask(ask, window, cx, |view, started, _, cx| {
            view.busy = false;
            match started {
                Ok(()) => {
                    view.error = None;
                    view.stage = Stage::Started;
                }
                Err(error) => view.error = Some(error),
            }
            cx.notify();
        });
    }

    /// Ends the import, which removes the key, and closes the panel.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(ask::cancel_linear_import(), window, cx, |_, _, _, cx| {
            cx.emit(Closed)
        });
    }

    fn key_step(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(hint(KEY_NOTE, p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Input::new(&self.key).small().w(px(280.)))
                    .child(
                        ui::gated(
                            button("linear-teams")
                                .xsmall()
                                .primary()
                                .label("List teams")
                                .loading(self.busy)
                                .disabled(self.busy),
                            &self.shared.host,
                        )
                        .on_click(cx.listener(|view, _, window, cx| view.list_teams(window, cx))),
                    ),
            )
    }

    fn teams_step(&self, teams: &[LinearTeam], p: Palette, cx: &mut Context<Self>) -> Div {
        if teams.is_empty() {
            return hint("This key reads no Linear team.", p);
        }
        let mut list = div().flex().flex_wrap().gap_2();
        for team in teams {
            let id = team.id.clone();
            list = list.child(
                ui::gated(
                    button(SharedString::from(format!("linear-team-{}", team.id)))
                        .xsmall()
                        .label(format!("{} · {}", team.key, team.name))
                        .disabled(self.busy),
                    &self.shared.host,
                )
                .on_click(
                    cx.listener(move |view, _, window, cx| view.preview(id.clone(), window, cx)),
                ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(hint(
                "Choose the team to import. Nothing is written yet.",
                p,
            ))
            .child(list)
    }

    fn plan_step(
        &self,
        plan: &LinearImportPlan,
        logins: &[Entity<InputState>],
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut states = div().flex().flex_col();
        for mapping in &plan.states {
            states = states.child(hint(state_line(mapping), p));
        }
        let mut users = div().flex().flex_col().gap_1();
        for (user, login) in plan.users.iter().zip(logins) {
            let name = match &user.email {
                Some(email) => format!("{} · {email}", user.name),
                None => user.name.clone(),
            };
            users = users.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(260.)).child(hint(name, p)))
                    .child(Input::new(login).small().w(px(180.))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(summary(plan)))
            .child(hint("Linear’s states become:", p))
            .child(states)
            .when(!plan.users.is_empty(), |d| {
                d.child(hint(USERS_NOTE, p)).child(users)
            })
            .child(
                div().flex().justify_end().child(
                    ui::gated(
                        button("linear-import")
                            .xsmall()
                            .primary()
                            .label(format!("Import {}", counted(plan.issues, "issue")))
                            .loading(self.busy)
                            .disabled(self.busy || plan.issues == 0),
                        &self.shared.host,
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.import(window, cx))),
                ),
            )
    }
}

const KEY_NOTE: &str = "Paste a Linear personal API key. Wiffletree keeps it in your Keychain only until the import ends, and only reads Linear: nothing there changes.";
const USERS_NOTE: &str = "Confirm each person’s GitHub login. What someone without one wrote is written as yours, with a first line naming them.";

/// The summary's first line, such as "240 issues and 512 comments, 240 keeping their numbers".
pub(super) fn summary(plan: &LinearImportPlan) -> String {
    let mut line = format!(
        "{} and {} from {}",
        counted(plan.issues, "issue"),
        counted(plan.comments, "comment"),
        plan.team.key
    );
    if plan.keeping_numbers > 0 {
        line.push_str(&format!(", {} keeping their numbers", plan.keeping_numbers));
    }
    if plan.imported_before > 0 {
        let before = counted(plan.imported_before, "issue");
        line.push_str(&format!("; {before} imported before are skipped"));
    }
    line
}

fn state_line(mapping: &LinearStateMapping) -> String {
    let issues = counted(mapping.issues, "issue");
    format!("{} → {} ({issues})", mapping.name, mapping.state.label())
}

/// The progress line, such as "Importing CEL: 100 of 240 issues written".
pub(super) fn progress_line(status: &LinearImportStatus) -> String {
    let written = format!("{} of {}", status.written, counted(status.total, "issue"));
    match (&status.error, status.finished) {
        (Some(error), _) => {
            format!("Stopped after {written}: {error}. Importing again adds the rest.")
        }
        (None, true) => format!("Imported {written} from {}.", status.team),
        (None, false) => format!("Importing {}: {written} written", status.team),
    }
}

/// The logins the human confirmed, by Linear user ID; an empty field confirms none.
pub(super) fn confirmed(
    users: &[LinearUser],
    typed: impl Iterator<Item = String>,
) -> BTreeMap<String, String> {
    (users.iter().zip(typed))
        .filter_map(|(user, login)| {
            let login = login.trim().trim_start_matches('@').to_owned();
            (!login.is_empty()).then(|| (user.id.clone(), login))
        })
        .collect()
}

impl Requests for LinearImportPanel {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

impl Render for LinearImportPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Import from Linear"),
            )
            .child(div().flex_1())
            .child(
                button("linear-close")
                    .xsmall()
                    .ghost()
                    .label(match self.status() {
                        Some(status) if !status.finished => "Stop",
                        Some(_) => "Done",
                        None => "Cancel",
                    })
                    .on_click(cx.listener(|view, _, window, cx| view.close(window, cx))),
            );
        let body = match (self.status(), &self.stage) {
            (Some(status), _) => hint(progress_line(status), p).text_color(match status.error {
                Some(_) => p.red,
                None => p.text,
            }),
            (None, Stage::Key) => self.key_step(p, cx),
            (None, Stage::Teams(teams)) => self.teams_step(teams, p, cx),
            (None, Stage::Plan(plan, logins)) => self.plan_step(plan, logins, p, cx),
            (None, Stage::Started) => hint("Starting…", p),
        };
        let mut panel = ui::card(p).child(head).child(body);
        if let Some(error) = &self.error {
            panel = panel.child(hint(error.clone(), p).text_color(p.red));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::{confirmed, progress_line, summary};
    use workspace_core::*;

    fn plan() -> LinearImportPlan {
        LinearImportPlan {
            space_id: SpaceId("sage".into()),
            team: LinearTeam {
                id: "team-cel".into(),
                key: "CEL".into(),
                name: "Celadon".into(),
            },
            issues: 240,
            imported_before: 0,
            comments: 512,
            keeping_numbers: 240,
            states: vec![],
            users: vec![],
        }
    }

    #[test]
    fn the_summary_counts_what_would_be_written_before_anything_is() {
        assert_eq!(
            summary(&plan()),
            "240 issues and 512 comments from CEL, 240 keeping their numbers"
        );
        let again = LinearImportPlan {
            issues: 1,
            comments: 0,
            keeping_numbers: 0,
            imported_before: 239,
            ..plan()
        };
        assert_eq!(
            summary(&again),
            "1 issue and 0 comments from CEL; 239 issues imported before are skipped"
        );
    }

    #[test]
    fn progress_says_how_far_the_import_got_and_why_it_stopped() {
        let mut status = LinearImportStatus {
            space_id: SpaceId("sage".into()),
            team: "CEL".into(),
            written: 100,
            total: 240,
            finished: false,
            error: None,
        };
        assert_eq!(
            progress_line(&status),
            "Importing CEL: 100 of 240 issues written"
        );
        status.finished = true;
        status.error = Some("GitHub was not updated: offline".into());
        assert_eq!(
            progress_line(&status),
            "Stopped after 100 of 240 issues: GitHub was not updated: offline. Importing again adds the rest."
        );
    }

    #[test]
    fn only_filled_logins_are_confirmed() {
        let user = |id: &str| LinearUser {
            id: id.into(),
            name: id.into(),
            email: None,
            login: None,
        };
        let users = [user("u-1"), user("u-2"), user("u-3")];
        let typed = ["@pshah ", "", "jlin"].map(String::from).into_iter();
        let logins = confirmed(&users, typed);
        let pairs: Vec<(&str, &str)> = (logins.iter())
            .map(|(id, login)| (id.as_str(), login.as_str()))
            .collect();
        assert_eq!(pairs, [("u-1", "pshah"), ("u-3", "jlin")]);
    }
}
