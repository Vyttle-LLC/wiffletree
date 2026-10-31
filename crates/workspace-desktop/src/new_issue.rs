//! New issue (OpenSpec change `shared-issues`, task 2.4, mockup study e): the form, its Space
//! defaulting to the current one, and the warning that issues are permanent and shared, naming
//! the storage repository, or that a local tracker's issue stays on this Mac.
use super::*;
use gpui_component::{
    button::ButtonVariants,
    input::{Input, Textarea, TextareaState},
};
use issues_view::{PRIORITIES, picker, priority_label};
use ui::hint;
// GPUI exports a `Priority` too; the issue's wins over both globs.
use workspace_core::Priority;

/// One Space New issue can file in: it has a tracker with a prefix.
#[derive(Clone)]
pub(super) struct Trackable {
    pub(super) space: Space,
    pub(super) prefix: String,
}

pub(super) struct NewIssueForm {
    spaces: Vec<Trackable>,
    space: SpaceId,
    title: Entity<InputState>,
    description: Entity<TextareaState>,
    assignee: Entity<InputState>,
    labels: Entity<InputState>,
    state: IssueState,
    priority: Priority,
    repositories: Vec<String>,
    /// The human's `gh` login; without one nothing can be filed.
    login: Option<String>,
    pub(super) error: String,
}
impl NewIssueForm {
    pub(super) fn new(
        spaces: Vec<Trackable>,
        space: SpaceId,
        login: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            spaces,
            space,
            title: cx.new(|cx| InputState::new(window, cx).placeholder("What should change?")),
            description: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 14)
                    .placeholder("Markdown. Mention someone with @login, or link CEL-123.")
            }),
            assignee: cx.new(|cx| InputState::new(window, cx).placeholder("Nobody")),
            labels: cx.new(|cx| InputState::new(window, cx).placeholder("bug, webhooks")),
            state: IssueState::Triage,
            priority: Priority::None,
            repositories: vec![],
            login,
            error: String::new(),
        }
    }
    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        self.title.update(cx, |input, cx| input.focus(window, cx));
    }
    fn chosen_space(&self) -> Option<&Trackable> {
        self.spaces.iter().find(|t| t.space.id == self.space)
    }
    /// The Space and the issue the form holds, or why it cannot be filed.
    pub(super) fn chosen(&self, cx: &App) -> Result<(SpaceId, NewIssue), String> {
        if self.login.is_none() {
            return Err(SIGN_IN.into());
        }
        let title = self.title.read(cx).value().trim().to_owned();
        if title.is_empty() {
            return Err("Give the issue a title".into());
        }
        let assignee = self.assignee.read(cx).value().trim().to_owned();
        let issue = NewIssue {
            title,
            description: self.description.read(cx).value().trim().to_owned(),
            state: self.state,
            priority: self.priority,
            assignee: (!assignee.is_empty()).then_some(assignee),
            labels: parse_labels(&self.labels.read(cx).value()),
            repositories: self.repositories.clone(),
        };
        Ok((self.space.clone(), issue))
    }
    fn choose_space(&mut self, space: SpaceId, cx: &mut Context<Self>) {
        self.space = space;
        // Another Space lists other repositories.
        self.repositories.clear();
        cx.notify();
    }
    fn toggle_repository(&mut self, repository: &str, cx: &mut Context<Self>) {
        match self.repositories.iter().position(|r| r == repository) {
            Some(at) => drop(self.repositories.remove(at)),
            None => self.repositories.push(repository.to_owned()),
        }
        cx.notify();
    }

    fn chips(&self, cx: &mut Context<Self>) -> Div {
        let weak = cx.weak_entity();
        let states = IssueState::ALL.map(|state| (state, state.label().into()));
        let state = picker(
            "new-issue-state",
            self.state.label(),
            states.to_vec(),
            Some(self.state),
            {
                let weak = weak.clone();
                move |state, _, cx| {
                    let _ = weak.update(cx, |form, cx| {
                        form.state = state;
                        cx.notify();
                    });
                }
            },
        );
        let priorities = PRIORITIES.map(|p| (p, priority_label(p).into()));
        let priority = picker(
            "new-issue-priority",
            format!("Priority: {}", priority_label(self.priority)),
            priorities.to_vec(),
            Some(self.priority),
            move |priority, _, cx| {
                let _ = weak.update(cx, |form, cx| {
                    form.priority = priority;
                    cx.notify();
                });
            },
        );
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(state)
            .child(priority)
    }

    fn field(label: &'static str, input: impl IntoElement, p: Palette) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(px(font::SMALL))
                    .text_color(p.subtle)
                    .child(label),
            )
            .child(input)
    }
}

const SIGN_IN: &str = "Sign gh in to file issues: run gh auth login in Terminal, then try again. Issues can still be read.";

/// The Space New issue starts in: the one the Issues page is filtered to, else the Space of the
/// last issue filed here, else the first that has a tracker.
pub(super) fn default_space(
    filtered: Option<&SpaceId>,
    last_filed: Option<&SpaceId>,
    trackable: &[SpaceId],
) -> Option<SpaceId> {
    [filtered, last_filed]
        .into_iter()
        .flatten()
        .find(|id| trackable.contains(id))
        .or(trackable.first())
        .cloned()
}

/// What the form says above File: for a tracker on GitHub, that issues are permanent and read by
/// everyone with access to its storage repository, and to keep personal data out; for a local
/// tracker, that the issue stays on this Mac.
fn warning(space: &Space) -> (bool, String) {
    match &space.storage.repository {
        StorageRepository::GitHub { owner, name } => (
            true,
            format!(
                "Issues are kept in Git history for good, and everyone with access to {owner}/{name} can read them. Don’t include personal data such as people’s names, messages, phone numbers or locations."
            ),
        ),
        StorageRepository::Local { .. } => (
            false,
            format!(
                "This issue stays on this Mac, in {}’s local storage. It is never pushed.",
                space.name
            ),
        ),
    }
}

/// How the issue is named once filed: a temporary ID until a GitHub tracker numbers it.
fn naming_line(space: &Space, prefix: &str) -> String {
    match &space.storage.repository {
        StorageRepository::GitHub { owner, name } => format!(
            "It shows a temporary ID, such as {prefix}-new-qxvb, until it reaches {owner}/{name} and is numbered."
        ),
        StorageRepository::Local { .. } => format!("It is numbered {prefix}-… at once."),
    }
}

/// Labels as typed, split at commas, trimmed, without blanks or repeats.
fn parse_labels(text: &str) -> Vec<String> {
    let mut labels: Vec<String> = vec![];
    for label in text.split(',').map(str::trim).filter(|l| !l.is_empty()) {
        if !labels.iter().any(|l| l == label) {
            labels.push(label.to_owned());
        }
    }
    labels
}

impl Render for NewIssueForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let Some(chosen) = self.chosen_space().cloned() else {
            return div().child(hint("No Space here has a tracker yet.", p));
        };
        let weak = cx.weak_entity();
        let spaces: Vec<(SpaceId, SharedString)> = (self.spaces.iter())
            .map(|t| (t.space.id.clone(), t.space.name.clone().into()))
            .collect();
        let space = picker(
            "new-issue-space",
            format!("{} · {}", chosen.space.name, chosen.prefix),
            spaces,
            Some(self.space.clone()),
            move |space, _, cx| {
                let _ = weak.update(cx, |form, cx| form.choose_space(space, cx));
            },
        );
        let mut repositories = div().flex().flex_wrap().gap_1();
        for repository in &chosen.space.repositories {
            let ticked = self.repositories.contains(repository);
            let name = repository.clone();
            repositories = repositories.child(
                button(SharedString::from(format!("new-issue-repo-{repository}")))
                    .xsmall()
                    .when(ticked, |b| b.primary())
                    .when(!ticked, |b| b.ghost())
                    .label(issues_view::short_repository(repository).to_owned())
                    .on_click(cx.listener(move |form, _, _, cx| form.toggle_repository(&name, cx))),
            );
        }
        let me = self.login.clone().map(|login| {
            button("new-issue-assign-me")
                .xsmall()
                .ghost()
                .label("Assign to me")
                .on_click(cx.listener(move |form, _, window, cx| {
                    let login = login.clone();
                    form.assignee
                        .update(cx, |input, cx| input.set_value(login, window, cx));
                }))
        });
        let (shared, warning) = warning(&chosen.space);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Self::field("Space", div().child(space), p))
            .child(Self::field("Title", Input::new(&self.title).small(), p))
            .child(Self::field(
                "Description",
                Textarea::new(&self.description).small(),
                p,
            ))
            .child(self.chips(cx))
            .child(Self::field(
                "Assignee",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Input::new(&self.assignee).small().w(px(200.)))
                    .children(me),
                p,
            ))
            .child(Self::field("Labels", Input::new(&self.labels).small(), p))
            .when(!chosen.space.repositories.is_empty(), |d| {
                d.child(Self::field("Repositories", repositories, p))
            })
            .child(
                div()
                    .p_2()
                    .rounded_md()
                    .bg(if shared { p.yellow } else { p.blue }.opacity(0.1))
                    .text_size(px(font::SMALL))
                    .text_color(if shared { p.yellow } else { p.text })
                    .child(if shared {
                        format!("⚠ {warning}")
                    } else {
                        warning
                    }),
            )
            .child(hint(naming_line(&chosen.space, &chosen.prefix), p))
            .when(self.login.is_none(), |d| {
                d.child(hint(SIGN_IN, p).text_color(p.red))
            })
            .when(!self.error.is_empty(), |d| {
                d.child(hint(self.error.clone(), p).text_color(p.red))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{default_space, naming_line, parse_labels, warning};
    use workspace_core::*;

    fn space(name: &str, storage: StorageRepository) -> Space {
        Space {
            id: SpaceId(name.to_lowercase()),
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            brain: None,
            storage: SpaceStorage {
                repository: storage,
                folder: name.to_lowercase(),
            },
            repositories: vec![],
            models: SpaceModels::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }

    #[test]
    fn new_issue_starts_in_the_filtered_space_else_the_last_one_filed_in() {
        let [celadon, vyttle, personal] =
            ["celadon", "vyttle", "personal"].map(|id| SpaceId(id.into()));
        let trackable = [celadon.clone(), vyttle.clone()];
        let default = |filtered, last| default_space(filtered, last, &trackable);
        assert_eq!(default(Some(&vyttle), Some(&celadon)), Some(vyttle.clone()));
        assert_eq!(default(None, Some(&vyttle)), Some(vyttle.clone()));
        assert_eq!(default(None, None), Some(celadon.clone()));
        assert_eq!(
            default(Some(&personal), None),
            Some(celadon),
            "a Space without a tracker is passed over"
        );
    }

    #[test]
    fn the_warning_names_the_storage_repository_or_says_the_issue_stays_here() {
        let celadon = space(
            "Celadon",
            StorageRepository::GitHub {
                owner: "celadon-theme".into(),
                name: "storage".into(),
            },
        );
        let (shared, text) = warning(&celadon);
        assert!(shared);
        assert!(text.starts_with("Issues are kept in Git history for good, and everyone with access to celadon-theme/storage can read them."));
        assert!(text.contains("names, messages, phone numbers or locations"));
        assert!(naming_line(&celadon, "CEL").contains("temporary ID, such as CEL-new-qxvb"));
        let personal = space("Personal", StorageRepository::Local { id: "7k2q".into() });
        let (shared, text) = warning(&personal);
        assert!(!shared);
        assert_eq!(
            text,
            "This issue stays on this Mac, in Personal’s local storage. It is never pushed."
        );
    }

    #[test]
    fn labels_are_split_at_commas_without_blanks_or_repeats() {
        assert_eq!(parse_labels(" bug, webhooks,,bug ,"), ["bug", "webhooks"]);
        assert!(parse_labels("  ").is_empty());
    }
}
