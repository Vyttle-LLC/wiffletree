//! Move storage… on a Space's page (OpenSpec change `move-space-storage`, screens 2 to 5 of its
//! mockup): this Mac or a GitHub repository, who can read it and what moves, or why it is
//! refused. Only the human moves storage, and the host checks everything again at Move.
use super::*;
use gpui_component::{Disableable, Sizable, button::ButtonVariants, input::Input};
use spaces_view::{is_shared, readers_sentence};
use ui::{banner, hint};
use wording::counted;

const SCANNED: &str = "Files are scanned before they are pushed.";
const PRIVATE: &str = "A private repository. Nothing is written until you click Move.";

/// The Spaces moved; the page reads its storage again.
pub(super) struct Moved;

pub(super) struct MoveStorageSheet {
    bridge: Bridge,
    shared: Shared,
    space: Space,
    on_github: bool,
    repository: Entity<InputState>,
    /// What the host last said a move would do, with the repository as named, `None` for this
    /// Mac. Move needs this to match what the sheet shows.
    checked: Option<(Option<String>, StorageMove)>,
    /// Why the host refused, or why the Spaces stayed where they were.
    refused: Option<String>,
    busy: bool,
}
impl EventEmitter<Moved> for MoveStorageSheet {}

/// Opens Move storage… for `space` over `window`.
pub(super) fn open(
    bridge: Bridge,
    shared: Shared,
    space: Space,
    window: &mut Window,
    cx: &mut App,
) -> Entity<MoveStorageSheet> {
    let sheet = cx.new(|cx| MoveStorageSheet::new(bridge, shared, space, window, cx));
    let shown = sheet.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = sheet.read(cx).title();
        dialog.title(title).width(px(620.)).child(sheet.clone())
    });
    shown
}

impl MoveStorageSheet {
    fn new(
        bridge: Bridge,
        shared: Shared,
        space: Space,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repository =
            cx.new(|cx| InputState::new(window, cx).placeholder("owner/name or a GitHub URL"));
        let on_github = starts_on_github(&space);
        let mut sheet = Self {
            bridge,
            shared,
            space,
            on_github,
            repository,
            checked: None,
            refused: None,
            busy: false,
        };
        if !on_github {
            sheet.check(window, cx);
        }
        sheet
    }
    fn title(&self) -> String {
        format!("Move {}’s storage", self.space.name)
    }
    /// The repository as named now, `None` for this Mac.
    fn target(&self, cx: &App) -> Option<String> {
        self.on_github
            .then(|| self.repository.read(cx).value().trim().to_owned())
    }
    /// What the host said of the target shown now.
    fn plan(&self, cx: &App) -> Option<&StorageMove> {
        let target = self.target(cx);
        (self.checked.as_ref())
            .filter(|(checked, _)| *checked == target)
            .map(|(_, plan)| plan)
    }
    fn choose_side(&mut self, on_github: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.on_github = on_github;
        self.checked = None;
        self.refused = None;
        if !on_github {
            self.check(window, cx);
        }
        cx.notify();
    }
    /// Asks the host what a move to the target shown would do, writing nothing.
    fn check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.target(cx);
        if target.as_ref().is_some_and(String::is_empty) {
            return;
        }
        self.busy = true;
        self.refused = None;
        let asked = target.clone();
        let ask = ask::check_storage_move(self.space.id.clone(), target);
        self.ask(ask, window, cx, move |view, plan, _, cx| {
            view.busy = false;
            match plan {
                Ok(plan) => view.checked = Some((asked, plan)),
                Err(error) => {
                    view.checked = None;
                    view.refused = Some(error);
                }
            }
            cx.notify();
        });
        cx.notify();
    }
    /// Moves the Spaces; the host checks again first, as the answer can go stale.
    fn start_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.refused = None;
        let name = self.space.name.clone();
        let ask = ask::move_storage(self.space.id.clone(), self.target(cx));
        self.ask(ask, window, cx, move |view, moved, window, cx| {
            view.busy = false;
            match moved {
                Ok(_) => {
                    window.close_dialog(cx);
                    toast(format!("Moved {name}’s storage"), window, cx);
                    cx.emit(Moved);
                }
                Err(error) => {
                    view.checked = None;
                    view.refused = Some(error);
                }
            }
            cx.notify();
        });
        cx.notify();
    }
    fn sides(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .gap_1()
            .child(
                ui::segment("move-to-mac", "This Mac", !self.on_github).on_click(
                    cx.listener(|view, _, window, cx| view.choose_side(false, window, cx)),
                ),
            )
            .child(
                ui::segment("move-to-github", "A GitHub repository", self.on_github).on_click(
                    cx.listener(|view, _, window, cx| view.choose_side(true, window, cx)),
                ),
            )
    }
    fn github_field(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(Input::new(&self.repository).small().flex_1().min_w_0())
            .child(
                ui::gated(
                    button("check-move").label("Check").disabled(self.busy),
                    &self.shared.host,
                )
                .on_click(cx.listener(|view, _, window, cx| view.check(window, cx))),
            )
    }
    fn footer(&self, plan: bool, p: Palette, cx: &mut Context<Self>) -> Div {
        let label = match (self.busy && plan, self.on_github) {
            (true, _) => "Moving…",
            (false, true) => "Move to GitHub",
            (false, false) => "Move to this Mac",
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(hint(if plan && self.on_github { SCANNED } else { "" }, p)),
            )
            .child(
                button("cancel-move")
                    .ghost()
                    .label("Cancel")
                    .disabled(self.busy)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                ui::gated(
                    button("confirm-move")
                        .primary()
                        .label(label)
                        .disabled(self.busy || !plan),
                    &self.shared.host,
                )
                .on_click(cx.listener(|view, _, window, cx| view.start_move(window, cx))),
            )
    }
}

impl Requests for MoveStorageSheet {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

impl Render for MoveStorageSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let plan = self.plan(cx).cloned();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(font::SMALL))
            .child(self.sides(cx));
        if self.on_github {
            body = body.child(self.github_field(cx));
            if plan.is_none() && self.refused.is_none() {
                body = body.child(hint(PRIVATE, p));
            }
        }
        if let Some(plan) = &plan {
            body = body.children(plan_banners(plan, p));
        }
        if let Some(refused) = &self.refused {
            let (lead, rest) = refusal_parts(refused);
            body = body.child(notice(p.yellow, p.yellow, lead, rest));
        }
        body.child(self.footer(plan.is_some(), p, cx))
    }
}

/// A tinted banner with a bold lead in `lead_color`.
fn notice(color: Hsla, lead_color: Hsla, lead: String, rest: String) -> Div {
    let bold = 0..lead.len();
    banner(color).child(div().flex_1().min_w_0().child(
        StyledText::new(format!("{lead}{rest}")).with_highlights([(
            bold,
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                color: Some(lead_color),
                ..Default::default()
            },
        )]),
    ))
}

/// The side the dialog starts on: the one the Space is not on.
fn starts_on_github(space: &Space) -> bool {
    !is_shared(space)
}

/// The banners of a checked move, as screens 3 and 5 draw them.
fn plan_banners(plan: &StorageMove, p: Palette) -> Vec<Div> {
    let Some(repository) = &plan.repository else {
        let (lead, rest) = to_this_mac(plan);
        let (kept, stays) = KEPT_ON_GITHUB;
        return vec![
            notice(p.subtle, p.text, lead, rest),
            notice(p.yellow, p.yellow, kept.into(), format!(" {stays}")),
        ];
    };
    let mut lines = what_moves(plan).into_iter().fold(
        div()
            .flex_1()
            .min_w_0()
            .child(div().font_weight(FontWeight::SEMIBOLD).child("What moves")),
        |lines, line| lines.child(format!("• {line}")),
    );
    if let Some(line) = keeps_reading(plan) {
        lines = lines.child(line);
    }
    vec![
        notice(
            p.yellow,
            p.yellow,
            "Who can read it:".into(),
            who_can_read(plan, repository),
        ),
        banner(p.subtle).child(lines),
    ]
}

const KEPT_ON_GITHUB: (&str, &str) = (
    "Teammates keep the GitHub repository.",
    "It stays as it is, and this Mac stops syncing it, so later changes on either side do not reach the other.",
);

fn joined(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn who_can_read(plan: &StorageMove, repository: &SettingsRepository) -> String {
    let names = joined(&plan.spaces);
    format!(
        " {} They can read {names} and its issues, and anyone who can push can change {names}’s additions and Behaviors for everyone.",
        readers_sentence(repository.readers, &repository.name)
    )
}

/// One line per Space that moves, then its tracker.
fn what_moves(plan: &StorageMove) -> Vec<String> {
    let spaces = (plan.spaces.iter())
        .map(|name| format!("{name}: its settings, repositories, jobs and boards"));
    let tracker = match (&plan.adopts, &plan.prefix) {
        (Some(prefix), _) => Some(format!(
            "Its tracker stays behind, with no issues, and it takes on {prefix}"
        )),
        (None, Some(prefix)) => Some(format!(
            "Its tracker: prefix {prefix} and {}",
            counted(plan.issues, "issue")
        )),
        (None, None) => None,
    };
    spaces.chain(tracker).collect()
}

/// What a move between two GitHub repositories leaves behind.
fn keeps_reading(plan: &StorageMove) -> Option<String> {
    let (leaves, repository) = (plan.leaves.as_ref()?, plan.repository.as_ref()?);
    Some(format!(
        "Teammates keep reading {leaves} until they add the Space from {}.",
        repository.name
    ))
}

fn to_this_mac(plan: &StorageMove) -> (String, String) {
    let verb = if plan.spaces.len() == 1 {
        "moves"
    } else {
        "move"
    };
    (
        format!(
            "{} {verb} to a new local repository on this Mac.",
            joined(&plan.spaces)
        ),
        " Its tracker becomes private to this Mac.".into(),
    )
}

/// A refusal's first sentence, which leads in bold, and the rest.
fn refusal_parts(refused: &str) -> (String, String) {
    match refused.split_once(". ") {
        Some((lead, rest)) => (format!("{lead}."), format!(" {rest}")),
        None => (refused.to_owned(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{keeps_reading, refusal_parts, starts_on_github, what_moves};
    use workspace_core::*;

    fn repository(name: &str) -> SettingsRepository {
        SettingsRepository {
            name: name.into(),
            push_url: format!("https://github.com/{name}.git"),
            branch: "main".into(),
            readers: Readers::Invited,
        }
    }

    fn plan() -> StorageMove {
        StorageMove {
            spaces: vec!["Vyttle".into()],
            repository: Some(repository("Vyttle-LLC/wiffletree-spaces")),
            prefix: Some("VYT".into()),
            issues: 0,
            adopts: None,
            leaves: None,
        }
    }

    fn space(repository: StorageRepository) -> Space {
        Space {
            id: SpaceId("vyttle".into()),
            name: "Vyttle".into(),
            description: String::new(),
            additions: String::new(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository,
                folder: "vyttle".into(),
            },
            repositories: vec![],
            models: Default::default(),
            reviews: Default::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }

    #[test]
    fn the_dialog_starts_on_the_side_the_space_is_not_on() {
        let local = StorageRepository::Local { id: "7k2q".into() };
        let github = StorageRepository::GitHub {
            owner: "Vyttle-LLC".into(),
            name: "storage".into(),
        };
        assert!(starts_on_github(&space(local)));
        assert!(!starts_on_github(&space(github)));
    }

    #[test]
    fn what_moves_names_every_space_and_the_tracker() {
        let two = StorageMove {
            spaces: vec!["Sage".into(), "Sage Watch".into()],
            prefix: Some("SAGE".into()),
            issues: 12,
            ..plan()
        };
        assert_eq!(
            what_moves(&two),
            [
                "Sage: its settings, repositories, jobs and boards",
                "Sage Watch: its settings, repositories, jobs and boards",
                "Its tracker: prefix SAGE and 12 issues",
            ]
        );
        let adopting = StorageMove {
            adopts: Some("SAGE".into()),
            ..plan()
        };
        assert!(what_moves(&adopting)[1].contains("takes on SAGE"));
    }

    #[test]
    fn only_a_move_between_two_github_repositories_says_teammates_keep_the_old_one() {
        assert_eq!(keeps_reading(&plan()), None);
        let between = StorageMove {
            leaves: Some("Vyttle-LLC/old".into()),
            ..plan()
        };
        assert_eq!(
            keeps_reading(&between).unwrap(),
            "Teammates keep reading Vyttle-LLC/old until they add the Space from Vyttle-LLC/wiffletree-spaces."
        );
    }

    #[test]
    fn a_refusal_leads_with_its_first_sentence() {
        assert_eq!(
            refusal_parts("x is Vyttle’s brain. A repository is a brain or storage, not both."),
            (
                "x is Vyttle’s brain.".to_owned(),
                " A repository is a brain or storage, not both.".to_owned()
            )
        );
        assert_eq!(
            refusal_parts("Sync is running"),
            ("Sync is running".to_owned(), String::new())
        );
    }
}
