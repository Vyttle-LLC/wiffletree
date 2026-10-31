//! Issue threads in the assistant view (OpenSpec change `assistant-spaces-jobs-boards`, task 6.5,
//! mockup section 8): Ask the assistant docks the issue's thread beside the Issues page, with a
//! notice naming who wrote the text it reads and a card for each proposal, which the human
//! applies, edits or discards. Issue text is untrusted, so it is shown as plain text.
use super::*;
use workspace_core::assistant::proposal::{
    Attached, DiffLine, Outcome, Proposal, ProposalChange, diff,
};

/// How many authors the notice names before "and N others".
const NAMED_AUTHORS: usize = 3;

/// A proposal the human is editing before applying it: its title or the issue it names, and
/// its text, as the matching form holds them.
pub(super) struct ProposalEdit {
    proposal: String,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
}

impl AssistantPage {
    /// The issue thread docked beside the Issues page, while it is in the strip.
    pub(crate) fn docked(&self) -> Option<&str> {
        let docked = self.docked.as_deref()?;
        self.tabs()
            .iter()
            .any(|tab| tab.thread.id == docked)
            .then_some(docked)
    }
    /// Ask the assistant: opens the issue thread of `attached` in `space`'s terms and docks it.
    pub(crate) fn ask_about(
        &mut self,
        attached: Attached,
        space: Option<SpaceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::open_issue_thread(attached, space);
        self.ask(ask, window, cx, |view, thread, window, cx| match thread {
            Ok(thread) => {
                view.docked = Some(thread.id.clone());
                view.select(Some(thread.id), window, cx);
            }
            Err(error) => toast_error(error, window, cx),
        });
    }
    /// The Space of the thread shown on `page`, in the assistant or docked beside the Issues
    /// page, which brain search searches.
    pub(crate) fn shown_space(&self, page: Page) -> Option<SpaceId> {
        let shown = page == Page::Assistant || (page == Page::Issues && self.docked().is_some());
        shown.then(|| self.open_tab().map(|tab| tab.thread.space_id.clone()))?
    }
    fn undock(&mut self, cx: &mut Context<Self>) {
        self.docked = None;
        cx.notify();
    }

    /// Reads the open issue thread's proposal cards and its authors again when they changed.
    pub(super) fn read_issue_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.open_tab().filter(|tab| tab.thread.attached.is_some()) else {
            return;
        };
        let (thread, attached) = (tab.thread.id.clone(), tab.thread.attached.clone());
        let space = Some(tab.thread.space_id.clone());
        let key = (thread.clone(), proposals_key(&tab.thread.proposals));
        if self.proposals_read.as_ref() != Some(&key) {
            self.proposals_read = Some(key.clone());
            self.ask(
                ask::proposals(thread.clone()),
                window,
                cx,
                move |view, views, _, cx| {
                    if view.proposals_read.as_ref() == Some(&key) {
                        match views {
                            Ok(views) => view.proposal_views = views,
                            Err(_) => view.proposals_read = None,
                        }
                        cx.notify();
                    }
                },
            );
        }
        if self
            .authors
            .as_ref()
            .is_some_and(|(read, _)| *read == thread)
        {
            return;
        }
        self.authors = Some((thread.clone(), vec![]));
        let keep = move |view: &mut Self, authors: Vec<String>, cx: &mut Context<Self>| {
            if view
                .authors
                .as_ref()
                .is_some_and(|(read, _)| *read == thread)
            {
                view.authors = Some((thread.clone(), authors));
                cx.notify();
            }
        };
        match attached {
            Some(Attached::Issue { id }) => {
                self.ask(ask::issue(id), window, cx, move |view, record, _, cx| {
                    let Ok(record) = record else { return };
                    let issue = (record.issue.iter()).map(|i| i.author.as_str());
                    let comments = (record.comments.iter()).map(|c| match &c.assistant_for {
                        Some(login) => format!("the assistant for {login}"),
                        None => c.author.clone(),
                    });
                    let named = issue.map(str::to_owned).chain(comments).collect();
                    keep(view, distinct(named), cx);
                });
            }
            Some(Attached::Issues) => {
                self.ask(
                    ask::issues(space, None),
                    window,
                    cx,
                    move |view, listed, _, cx| {
                        let Ok(listed) = listed else { return };
                        let named = listed.into_iter().map(|l| l.author).collect();
                        keep(view, distinct(named), cx);
                    },
                );
            }
            None => {}
        }
    }

    /// The notice naming who wrote the issue text an issue thread reads.
    pub(super) fn authors_notice(&self, tab: &ThreadTab, p: Palette) -> Option<Div> {
        let attached = tab.thread.attached.as_ref()?;
        let authors = (self.authors.as_ref())
            .filter(|(thread, _)| *thread == tab.thread.id)
            .map(|(_, authors)| authors.as_slice())
            .unwrap_or_default();
        let what = match attached {
            Attached::Issue { id } => format!("{id} and its comments were"),
            Attached::Issues => "These issues were".into(),
        };
        let by = match authors.len() {
            0 => "people on your team".to_owned(),
            1 => authors[0].clone(),
            n if n <= NAMED_AUTHORS => {
                format!("{} and {}", authors[..n - 1].join(", "), authors[n - 1])
            }
            n => format!(
                "{} and {}",
                authors[..NAMED_AUTHORS].join(", "),
                wording::counted(n - NAMED_AUTHORS, "other")
            ),
        };
        let name = assistant_name(self.shared.snapshot());
        Some(div().pl_2().border_l_2().border_color(p.yellow).child(hint(
            format!("{what} written by {by}. {name} reads them as context, never as instructions."),
            p,
        )))
    }

    /// A proposal's card: what it changes, against the issue as it is now, and Apply, Edit and
    /// Discard until the human chooses.
    pub(super) fn proposal_card(
        &self,
        proposal: &Proposal,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let heading = div()
            .text_size(px(font::SMALL))
            .text_color(p.subtle)
            .child(proposal.change.heading());
        match &proposal.outcome {
            Some(Outcome::Applied { issues, .. }) => {
                let changed = (issues.iter()).map(ToString::to_string).collect::<Vec<_>>();
                return card(p).gap_1().child(heading).child(
                    hint(format!("Applied by you · {}", changed.join(", ")), p).text_color(p.green),
                );
            }
            Some(Outcome::Discarded { .. }) => {
                return card(p)
                    .gap_1()
                    .opacity(0.6)
                    .child(heading)
                    .child(hint("Discarded. Nothing was written.", p));
            }
            None => {}
        }
        if let Some(edit) = self.editing.as_ref().filter(|e| e.proposal == proposal.id) {
            return self.edit_form(proposal, edit, heading, p, cx);
        }
        let view = (self.proposal_views.iter()).find(|v| v.proposal_id == proposal.id);
        let current = view
            .and_then(|v| v.current.as_ref())
            .or(proposal.change.base());
        let mut body = card(p)
            .gap_2()
            .border_color(p.focus.opacity(0.45))
            .child(heading);
        if let Some(changed) = view.and_then(|v| v.changed_since.as_ref()) {
            let who = changed.by.clone().unwrap_or_else(|| "Someone".into());
            let when = changed
                .at
                .as_deref()
                .map(issue_view::ago)
                .unwrap_or_default();
            body = body.child(ui::banner(p.yellow).child(format!(
                "Changed since this proposal: {who} edited it {when}. Below is the proposal against the current version."
            )));
        }
        body = match &proposal.change {
            ProposalChange::Comment { body: text, .. } => {
                let login = self.login().unwrap_or_else(|| "you".into());
                body.child(quoted(text, p))
                    .child(hint(format!("Posted as “Assistant for {login}”."), p))
            }
            ProposalChange::NewIssue { new } => body
                .child(draft(new, p))
                .child(hint("Filed in this thread's Space's tracker, as yours.", p)),
            ProposalChange::Split { new, .. } => {
                let mut list = div().flex().flex_col().gap_1();
                for issue in new {
                    list = list.child(draft(issue, p));
                }
                let edited = current.and_then(|c| proposal.change.applied_to(c));
                body.child(list).children(
                    current
                        .zip(edited)
                        .map(|(c, e)| diff_lines(&diff(c, &e), p)),
                )
            }
            ProposalChange::Edit { .. } | ProposalChange::Duplicate { .. } => {
                let edited = current.and_then(|c| proposal.change.applied_to(c));
                let lines = current.zip(edited).map(|(c, e)| diff(c, &e));
                let note = matches!(proposal.change, ProposalChange::Duplicate { .. })
                    .then(|| hint("A link only: no state changes.", p));
                body.children(lines.map(|lines| diff_lines(&lines, p)))
                    .children(note)
            }
        };
        let id = proposal.id.clone();
        let (discard, edit, apply) = (id.clone(), id.clone(), id);
        body.child(
            div()
                .flex()
                .gap_2()
                .justify_end()
                .child(
                    button(SharedString::from(format!("discard-{discard}")))
                        .ghost()
                        .xsmall()
                        .label("Discard")
                        .on_click(cx.listener(move |v, _, w, c| v.discard(&discard, w, c))),
                )
                .child(
                    button(SharedString::from(format!("edit-{edit}")))
                        .xsmall()
                        .label("Edit")
                        .on_click(cx.listener(move |v, _, w, c| v.start_edit(&edit, w, c))),
                )
                .child(
                    button(SharedString::from(format!("apply-{apply}")))
                        .primary()
                        .xsmall()
                        .label(match view.and_then(|v| v.changed_since.as_ref()) {
                            Some(_) => "Apply to the current version",
                            None => "Apply",
                        })
                        .on_click(cx.listener(move |v, _, w, c| v.apply(&apply, None, w, c))),
                ),
        )
    }

    /// The proposal in the form its kind matches: a title, or for a duplicate the issue it
    /// names, and its text; saving there is the apply.
    fn edit_form(
        &self,
        proposal: &Proposal,
        edit: &ProposalEdit,
        heading: Div,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (title, body) = form_labels(&proposal.change);
        let id = proposal.id.clone();
        card(p)
            .gap_2()
            .child(heading)
            .children(title.map(|label| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(hint(label, p))
                    .child(Input::new(&edit.title).small())
            }))
            .children(body.map(|label| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(hint(label, p))
                    .child(Textarea::new(&edit.body).w_full())
            }))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        button("proposal-edit-cancel")
                            .ghost()
                            .xsmall()
                            .label("Cancel")
                            .on_click(cx.listener(|v, _, _, c| {
                                v.editing = None;
                                c.notify();
                            })),
                    )
                    .child(
                        button("proposal-edit-apply")
                            .primary()
                            .xsmall()
                            .label("Save and apply")
                            .on_click(cx.listener(move |v, _, w, c| v.apply_edited(&id, w, c))),
                    ),
            )
    }

    fn start_edit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(proposal) = self.open_proposal(id) else {
            return;
        };
        let (title, body) = form_values(&proposal.change);
        let title = cx.new(|cx| InputState::new(window, cx).default_value(title));
        let body = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx).auto_grow(3, 12);
            state.set_value(body, window, cx);
            state
        });
        self.editing = Some(ProposalEdit {
            proposal: id.to_owned(),
            title,
            body,
        });
        cx.notify();
    }
    fn apply_edited(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(proposal), Some(edit)) = (self.open_proposal(id), &self.editing) else {
            return;
        };
        let title = edit.title.read(cx).value().to_string();
        let body = edit.body.read(cx).value().to_string();
        match edited(&proposal.change, title.trim(), &body) {
            Ok(change) => self.apply(id, Some(change), window, cx),
            Err(error) => toast_error(error, window, cx),
        }
    }
    fn apply(
        &mut self,
        id: &str,
        edited: Option<ProposalChange>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.open.clone() else {
            return;
        };
        // The card must show the issue as it is now: one that changed since it was drawn is
        // drawn again, with its banner, and applied only on the next click.
        let shown = (self.proposal_views.iter())
            .find(|v| v.proposal_id == id)
            .cloned();
        let id = id.to_owned();
        let ask = ask::proposals(thread.clone());
        self.ask(ask, window, cx, move |view, views, window, cx| {
            let views = match views {
                Ok(views) => views,
                Err(error) => return toast_error(error, window, cx),
            };
            let now = views.iter().find(|v| v.proposal_id == id).cloned();
            if now != shown {
                view.proposal_views = views;
                toast_error(
                    "The issue changed since this card was drawn. Check it again, then apply.",
                    window,
                    cx,
                );
                cx.notify();
                return;
            }
            let ask = ask::apply_proposal(thread, id, edited);
            view.ask(ask, window, cx, |view, applied, window, cx| match applied {
                Ok(_) => {
                    view.editing = None;
                    cx.emit(Intent::IssuesChanged);
                    cx.notify();
                }
                Err(error) => toast_error(error, window, cx),
            });
        });
    }
    fn discard(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.open.clone() else {
            return;
        };
        let ask = ask::discard_proposal(thread, id.to_owned());
        self.ask(ask, window, cx, |_, discarded, window, cx| {
            if let Err(error) = discarded {
                toast_error(error, window, cx);
            }
        });
    }
    fn open_proposal(&self, id: &str) -> Option<Proposal> {
        let tab = self.open_tab()?;
        tab.thread.proposals.iter().find(|p| p.id == id).cloned()
    }
    /// The human's GitHub login, as the trackers last read it.
    fn login(&self) -> Option<String> {
        let trackers = self.shared.snapshot().map(|s| &s.issue_trackers)?;
        trackers.iter().find_map(|t| t.login.clone())
    }

    /// An issue thread beside the Issues page: its title, Space, model and "Web off", its
    /// conversation with the notice and its cards, and a composer that sends to it alone.
    pub(super) fn docked_body(&self, tab: &ThreadTab, p: Palette, cx: &mut Context<Self>) -> Div {
        let space = (self.spaces().iter())
            .find(|s| s.id == tab.thread.space_id)
            .map(|s| s.name.clone());
        let line = [
            space,
            tab.profile.as_ref().map(ModelProfile::short),
            Some("Web off".into()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(p.edge.opacity(edge::LIGHT))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(tab.thread.title.clone()),
                    )
                    .child(hint(line, p)),
            )
            .child(
                button("undock-thread")
                    .ghost()
                    .xsmall()
                    .icon(icon("close"))
                    .tooltip("Close")
                    .on_click(cx.listener(|v, _, _, c| v.undock(c))),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.surface)
            .border_l_1()
            .border_color(p.edge)
            .child(header)
            .child(
                div()
                    .id("docked-conversation")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.conversation(tab, p, cx).px_3()),
            )
            .child(self.composer_box(p, cx))
    }
}

/// What tells the proposals' cards to read again: how many there are and how many wait.
fn proposals_key(proposals: &[Proposal]) -> usize {
    let waiting = proposals.iter().filter(|p| p.outcome.is_none()).count();
    proposals.len() * 1000 + waiting
}

/// Each name once, in the order first named.
fn distinct(names: Vec<String>) -> Vec<String> {
    let mut seen: Vec<String> = vec![];
    for name in names {
        if !name.is_empty() && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

fn quoted(text: &str, p: Palette) -> Div {
    div()
        .p_2()
        .rounded_md()
        .bg(p.surface)
        .border_1()
        .border_color(p.edge.opacity(edge::LIGHT))
        .whitespace_normal()
        .child(text.to_owned())
}

/// A proposed new issue: its title, its fields and its description.
fn draft(new: &workspace_core::NewIssue, p: Palette) -> Div {
    let mut fields = vec![new.state.label().to_owned()];
    fields.extend(new.labels.iter().cloned());
    fields.extend(new.repositories.iter().cloned());
    quoted("", p)
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(new.title.clone()),
        )
        .child(hint(fields.join(" · "), p))
        .when(!new.description.is_empty(), |d| {
            d.child(div().whitespace_normal().child(new.description.clone()))
        })
}

fn diff_lines(lines: &[DiffLine], p: Palette) -> Div {
    let mut shown = quoted("", p).flex().flex_col();
    for line in lines {
        let (mark, text, color) = match line {
            DiffLine::Same(text) => ("  ", text, p.subtle),
            DiffLine::Removed(text) => ("− ", text, p.red),
            DiffLine::Added(text) => ("+ ", text, p.green),
        };
        shown = shown.child(
            ui::mono(format!("{mark}{text}"), p)
                .text_color(color)
                .when(matches!(line, DiffLine::Removed(_)), |d| d.line_through()),
        );
    }
    shown
}

/// The edit form's labels, for its title field and its text, by kind.
fn form_labels(change: &ProposalChange) -> (Option<&'static str>, Option<&'static str>) {
    match change {
        ProposalChange::Edit { .. } | ProposalChange::NewIssue { .. } => {
            (Some("Title"), Some("Description"))
        }
        ProposalChange::Comment { .. } => (None, Some("Comment")),
        ProposalChange::Split { .. } => (None, Some("New issues, one title per line")),
        ProposalChange::Duplicate { .. } => (Some("Duplicate of"), None),
    }
}

/// What the edit form starts with.
fn form_values(change: &ProposalChange) -> (String, String) {
    match change {
        ProposalChange::Edit { base, changes, .. } => (
            changes.title.clone().unwrap_or_else(|| base.title.clone()),
            (changes.description.clone()).unwrap_or_else(|| base.description.clone()),
        ),
        ProposalChange::NewIssue { new } => (new.title.clone(), new.description.clone()),
        ProposalChange::Comment { body, .. } => (String::new(), body.clone()),
        ProposalChange::Split { new, .. } => {
            let titles: Vec<&str> = new.iter().map(|n| n.title.as_str()).collect();
            (String::new(), titles.join("\n"))
        }
        ProposalChange::Duplicate { of, .. } => (of.to_string(), String::new()),
    }
}

/// The proposal as the human edited it: the same kind for the same issue, with the form's
/// title and text in place of the thread's.
fn edited(change: &ProposalChange, title: &str, body: &str) -> Result<ProposalChange, String> {
    let mut change = change.clone();
    match &mut change {
        ProposalChange::Edit { base, changes, .. } => {
            changes.title = (title != base.title).then(|| title.to_owned());
            changes.description = (body != base.description).then(|| body.to_owned());
        }
        ProposalChange::NewIssue { new } => {
            new.title = title.to_owned();
            new.description = body.to_owned();
        }
        ProposalChange::Comment { body: text, .. } => *text = body.to_owned(),
        ProposalChange::Split { new, .. } => {
            let titles: Vec<&str> = body
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            if titles.is_empty() {
                return Err("A split files at least one new issue".into());
            }
            let template = new.first().cloned().unwrap_or_default();
            *new = (titles.iter())
                .enumerate()
                .map(|(at, title)| workspace_core::NewIssue {
                    title: (*title).to_owned(),
                    ..new.get(at).cloned().unwrap_or_else(|| template.clone())
                })
                .collect();
        }
        ProposalChange::Duplicate { of, .. } => {
            *of = workspace_core::IssueId::parse(title).map_err(|e| e.to_string())?;
        }
    }
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::{distinct, edited, form_values};
    use workspace_core::assistant::proposal::{IssueChanges, ProposalChange};
    use workspace_core::{Extra, Issue, IssueId, IssueState, Priority};

    fn issue() -> Issue {
        Issue {
            id: IssueId::parse("SAGE-151").unwrap(),
            title: "Delivery webhook retries forever".into(),
            state: IssueState::Todo,
            priority: Priority::High,
            assignee: None,
            labels: vec![],
            repositories: vec![],
            blocks: vec![],
            blocked_by: vec![],
            related: vec![],
            duplicate_of: None,
            author: "treyes".into(),
            created: "2026-10-31T12:00:00Z".into(),
            description: "Since Monday it retries.".into(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn an_edited_proposal_keeps_its_kind_and_takes_the_forms_text() {
        let base = issue();
        let edit = ProposalChange::Edit {
            id: base.id.clone(),
            base: Box::new(base.clone()),
            changes: IssueChanges::default(),
        };
        let (title, body) = form_values(&edit);
        assert_eq!(
            (title.as_str(), body.as_str()),
            (base.title.as_str(), base.description.as_str())
        );
        let ProposalChange::Edit { changes, .. } =
            edited(&edit, &title, "Since Monday it retries.\n## Done when").unwrap()
        else {
            panic!("an edit");
        };
        assert_eq!(changes.title, None, "an unchanged title is not proposed");
        assert!(changes.description.unwrap().ends_with("## Done when"));
        let split = ProposalChange::Split {
            id: base.id.clone(),
            base: Box::new(base.clone()),
            changes: IssueChanges::default(),
            new: vec![],
        };
        assert!(edited(&split, "", "  \n").is_err());
        let ProposalChange::Split { new, .. } = edited(&split, "", "One\nTwo").unwrap() else {
            panic!("a split");
        };
        assert_eq!(new.len(), 2);
        let duplicate = ProposalChange::Duplicate {
            id: base.id.clone(),
            of: IssueId::parse("SAGE-150").unwrap(),
            base: Box::new(base),
        };
        assert!(edited(&duplicate, "not an id", "").is_err());
    }

    #[test]
    fn the_notice_names_each_author_once() {
        let named = distinct(vec![
            "treyes".into(),
            "pshah".into(),
            "treyes".into(),
            String::new(),
        ]);
        assert_eq!(named, ["treyes", "pshah"]);
    }
}
