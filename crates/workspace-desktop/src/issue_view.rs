//! One issue (OpenSpec change `shared-issues`, task 2.3, mockup study c): its description and one
//! timeline of comments and events, a composer, and beside them its properties, Work box and
//! related issues. An `issue.md` that does not parse is shown raw with a warning. Issue text is
//! untrusted: everything a teammate wrote is shown as plain text, never rendered.
use super::*;
use chrono::DateTime;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    input::{Textarea, TextareaState},
};
use issues_view::{PRIORITIES, label_color, picker, priority_label, short_repository};
use ui::{card, hint, mono, pill};
// GPUI exports a `Priority` too; the issue's wins over both globs.
use workspace_core::Priority;

/// What the issue asks of the Issues page around it.
pub(super) enum IssueViewEvent {
    /// Back to the list.
    Back,
    /// The human changed the issue, so the list is out of date.
    Changed,
    Open(IssueId),
    /// Start project from issue, for the Issues page to offer its Spaces.
    StartProject {
        id: IssueId,
        title: String,
    },
    /// Opens a linked project's coordinator.
    OpenSession(String),
}

pub(super) struct IssueView {
    bridge: Bridge,
    host: HostView,
    /// The workspace as last shared, for the linked projects and tickets.
    index: Option<Rc<WorkspaceIndex>>,
    pub(super) id: IssueId,
    record: Option<Result<IssueRecord, String>>,
    composer: Entity<TextareaState>,
    /// The human's `gh` login, which writes need.
    login: Option<String>,
    /// Who the assignee picker offers, once the host answered.
    people: Vec<IssuePerson>,
    /// A change is waiting for the host.
    busy: bool,
}
impl EventEmitter<IssueViewEvent> for IssueView {}
impl IssueView {
    /// Reads the issue, and marks it read when it was unread.
    pub(super) fn new(
        bridge: Bridge,
        id: IssueId,
        unread: bool,
        shared: &Shared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("Write a comment… Markdown, @mention, link CEL-123 or a PR")
        });
        // The mentions and links under the composer follow the draft.
        cx.subscribe(&composer, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let login =
            (shared.snapshot()).and_then(|s| s.issue_trackers.iter().find_map(|t| t.login.clone()));
        let space = (shared.snapshot()).and_then(|s| {
            (s.issue_trackers.iter())
                .find(|t| t.prefix.as_deref() == Some(id.prefix()))
                .map(|t| t.space_id.clone())
        });
        let mut view = Self {
            bridge,
            host: shared.host.clone(),
            index: shared.index.clone(),
            id,
            record: None,
            composer,
            login,
            people: vec![],
            busy: false,
        };
        view.load(window, cx);
        if let Some(space) = space {
            view.ask(
                ask::issue_people(space),
                window,
                cx,
                |view, people, _, cx| {
                    view.people = people.unwrap_or_default();
                    cx.notify();
                },
            );
        }
        if unread {
            view.ask(
                ask::mark_issue_read(view.id.clone()),
                window,
                cx,
                |_, _, _, cx| cx.emit(IssueViewEvent::Changed),
            );
        }
        view
    }
    /// Reads the issue again from its files.
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read(false, window, cx);
    }
    /// Takes the workspace as it is now, whose tickets and PRs the Work box lists.
    pub(super) fn share(&mut self, shared: &Shared, cx: &mut Context<Self>) {
        let changed = !matches!(
            (&self.index, &shared.index),
            (Some(old), Some(new)) if Rc::ptr_eq(old, new)
        );
        if changed {
            self.index = shared.index.clone();
            cx.notify();
        }
    }
    /// Reads the issue again after its tracker changed, such as when a pass pulled a teammate's
    /// change, and marks what it holds now read: the human is looking at it.
    pub(super) fn reload_seen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(window, cx);
        self.ask(
            ask::mark_issue_read(self.id.clone()),
            window,
            cx,
            |_, _, _, _| {},
        );
    }
    /// Reads the issue; `after_change` ends the change waiting on it, so the next edit starts
    /// from what the host wrote. An issue numbered while open is followed to its number.
    fn read(&mut self, after_change: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(
            ask::issue(self.id.clone()),
            window,
            cx,
            move |view, record, _, cx| {
                if let Ok(record) = &record {
                    view.id = record.id.clone();
                }
                view.record = Some(record);
                view.busy &= !after_change;
                cx.notify();
            },
        );
    }
    /// Sends one change the human made, then reads the issue and the list again.
    fn change<R: serde::de::DeserializeOwned + 'static>(
        &mut self,
        change: Ask<R>,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        // One change at a time: each edit sends the whole issue, so it must start from the last.
        if self.busy {
            return;
        }
        self.busy = true;
        self.ask(
            change,
            window,
            cx,
            move |view, reply, window, cx| match reply {
                Ok(_) => {
                    done(view, window, cx);
                    view.read(true, window, cx);
                    cx.emit(IssueViewEvent::Changed);
                }
                Err(error) => {
                    view.busy = false;
                    toast_error(error, window, cx);
                }
            },
        );
        cx.notify();
    }
    fn comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value().trim().to_owned();
        if body.is_empty() || self.busy {
            return;
        }
        let ask = ask::comment_on_issue(self.id.clone(), body);
        self.change(ask, window, cx, |view, window, cx| {
            (view.composer).update(cx, |input, cx| input.set_value("", window, cx));
        });
    }
    fn edit(&mut self, edit: impl FnOnce(&mut Issue), window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(IssueRecord {
            issue: Some(issue), ..
        })) = &self.record
        else {
            return;
        };
        let mut issue = issue.clone();
        edit(&mut issue);
        self.change(ask::edit_issue(issue), window, cx, |_, _, _| {});
    }

    fn properties(&self, issue: &Issue, p: Palette, cx: &mut Context<Self>) -> Div {
        let weak = cx.weak_entity();
        let states = IssueState::ALL.map(|state| (state, state.label().into()));
        let state = picker(
            "issue-state",
            issue.state.label(),
            states.to_vec(),
            Some(issue.state),
            {
                let weak = weak.clone();
                move |state, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        let ask = ask::move_issue(view.id.clone(), state);
                        view.change(ask, window, cx, |_, _, _| {});
                    });
                }
            },
        );
        let priorities = PRIORITIES.map(|p| (p, priority_label(p).into()));
        let priority = picker(
            "issue-priority",
            priority_label(issue.priority),
            priorities.to_vec(),
            Some(issue.priority),
            move |priority, window, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.edit(|issue| issue.priority = priority, window, cx)
                });
            },
        );
        let assignee = picker(
            "issue-assignee",
            match &issue.assignee {
                Some(login) => person_label(&self.people, login),
                None => "Nobody".into(),
            },
            assignee_options(&self.people, issue.assignee.as_deref()),
            Some(issue.assignee.clone()),
            {
                let weak = cx.weak_entity();
                move |assignee, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.edit(|issue| issue.assignee = assignee, window, cx)
                    });
                }
            },
        );
        let mine = self.login.is_some() && issue.assignee == self.login;
        let assign = self.login.clone().map(|login| {
            let label = if mine { "Unassign" } else { "Assign to me" };
            ui::gated(
                button("issue-assign").xsmall().ghost().label(label),
                &self.host,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                let assignee = (!mine).then(|| login.clone());
                view.edit(|issue| issue.assignee = assignee, window, cx)
            }))
        });
        let labels = (issue.labels.iter()).map(|label| pill(label.clone(), label_color(label, p)));
        let repositories = (issue.repositories.iter())
            .map(|repository| pill(short_repository(repository).to_owned(), p.subtle));
        let related = related(issue);
        div()
            .w(px(260.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .child(prop("State", div().child(state), p))
            .child(prop("Priority", div().child(priority), p))
            .child(prop(
                "Assignee",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(assignee)
                    .children(assign),
                p,
            ))
            .child(prop("Inbox", self.follow_button(cx), p))
            .when(!issue.labels.is_empty(), |d| {
                d.child(prop(
                    "Labels",
                    div().flex().flex_wrap().gap_1().children(labels),
                    p,
                ))
            })
            .when(!issue.repositories.is_empty(), |d| {
                d.child(prop(
                    "Repositories",
                    div().flex().flex_wrap().gap_1().children(repositories),
                    p,
                ))
            })
            .child(prop("Work", self.work_box(p, cx), p))
            .when(!related.is_empty(), |d| {
                d.child(prop("Related", self.related_links(related, p, cx), p))
            })
    }

    /// Follow, or Following to stop: whether the issue's new comments reach the Inbox.
    fn follow_button(&self, cx: &mut Context<Self>) -> Button {
        let following = matches!(&self.record, Some(Ok(record)) if record.following);
        let label = if following { "Following" } else { "Follow" };
        let button =
            (button("issue-follow").xsmall().ghost().label(label)).tooltip(match following {
                true => "New comments reach your Inbox. Click to stop.",
                false => "Get new comments in your Inbox.",
            });
        ui::gated(button, &self.host).on_click(cx.listener(move |view, _, window, cx| {
            let ask = ask::follow_issue(view.id.clone(), !following);
            view.change(ask, window, cx, |_, _, _| {});
        }))
    }

    fn related_links(
        &self,
        related: Vec<(&'static str, String)>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut list = div().flex().flex_col().gap_1();
        for (at, (how, id)) in related.into_iter().enumerate() {
            let line = div()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
                .child(how);
            list = list.child(match IssueId::parse(&id) {
                Ok(parsed) => line.child(
                    button(("issue-related", at))
                        .xsmall()
                        .ghost()
                        .label(id)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(IssueViewEvent::Open(parsed.clone()))
                        })),
                ),
                // Written by hand, not an ID: shown as written.
                Err(_) => line.child(mono(id, p)),
            });
        }
        list
    }

    /// The Work box: the linked project, its tickets and their PRs; who started work on another
    /// Mac and its PRs; or, with no work yet, Start project from issue.
    fn work_box(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(Ok(record)) = &self.record else {
            return card(p);
        };
        let line = |text: String| hint(text, p).whitespace_normal();
        match work(record, self.index.as_deref()) {
            Work::Nothing => {
                let id = self.id.clone();
                let title = (record.issue.as_ref()).map_or_else(String::new, |i| i.title.clone());
                let start = ui::gated(
                    button("issue-start-project")
                        .primary()
                        .label("Start project from issue"),
                    &self.host,
                )
                .disabled(self.login.is_none())
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(IssueViewEvent::StartProject {
                        id: id.clone(),
                        title: title.clone(),
                    })
                }));
                let start = match self.login {
                    Some(_) => start,
                    None => start.tooltip("Sign gh in to start work on an issue"),
                };
                card(p)
                    .child(hint("No project yet.", p))
                    .child(start)
                    .child(line(
                        "Opens a coordinator with this issue as its goal.".into(),
                    ))
            }
            Work::Here(projects) => {
                let mut list = card(p);
                for (at, project) in projects.into_iter().enumerate() {
                    let open = project.coordinator.clone();
                    list = list.child(
                        button(("issue-work-project", at))
                            .xsmall()
                            .ghost()
                            .label(project.name)
                            .disabled(open.is_none())
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if let Some(id) = &open {
                                    cx.emit(IssueViewEvent::OpenSession(id.clone()));
                                }
                            })),
                    );
                    for (title, pr) in project.tickets {
                        list = list.child(line(match pr {
                            Some((number, state)) => {
                                format!("{title} · PR #{number} {}", pr_state_label(state))
                            }
                            None => title,
                        }));
                    }
                }
                list
            }
            Work::Elsewhere { started_by, prs } => {
                let mut list = card(p);
                for by in started_by {
                    list = list.child(line(format!("{by} started a project")));
                }
                for (url, status) in prs {
                    list = list.child(line(format!("{url} · {status}")));
                }
                list
            }
        }
    }

    fn composer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let draft = self.composer.read(cx).value().to_string();
        let (mentions, links) = references(&draft);
        let mut named = vec![];
        if !mentions.is_empty() {
            named.push(format!("Mentions {}", mentions.join(", ")));
        }
        if !links.is_empty() {
            named.push(format!("Links {}", links.join(", ")));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(edge::FIRM))
            .bg(p.surface)
            .child(
                Textarea::new(&self.composer)
                    .w_full()
                    .appearance(false)
                    .bordered(false),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(hint(named.join(" · "), p).flex_1())
                    .child(
                        ui::gated(
                            button("issue-comment")
                                .ghost()
                                .label("Comment")
                                .disabled(self.busy || draft.trim().is_empty()),
                            &self.host,
                        )
                        .on_click(cx.listener(|view, _, window, cx| view.comment(window, cx))),
                    ),
            )
    }
}

/// A login as the picker names it: "Priya Shah (pshah)" once the profile's name is known.
fn person_label(people: &[IssuePerson], login: &str) -> String {
    let person = people.iter().find(|p| p.login.eq_ignore_ascii_case(login));
    match person.and_then(|person| person.name.as_deref()) {
        Some(name) => format!("{name} ({login})"),
        None => login.to_owned(),
    }
}

/// Nobody, then each person the host offers, and the current assignee when the host does not
/// name them.
fn assignee_options(
    people: &[IssuePerson],
    current: Option<&str>,
) -> Vec<(Option<String>, SharedString)> {
    let mut options = vec![(None, "Nobody".into())];
    let listed = |login: &str| people.iter().any(|p| p.login.eq_ignore_ascii_case(login));
    if let Some(current) = current.filter(|current| !listed(current)) {
        options.push((Some(current.to_owned()), current.to_owned().into()));
    }
    for person in people {
        let label = person_label(people, &person.login);
        options.push((Some(person.login.clone()), label.into()));
    }
    options
}

fn prop(label: &'static str, value: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(font::CAPTION))
                .text_color(p.subtle)
                .child(label),
        )
        .child(value)
}

/// What the Work box shows.
#[derive(Debug, PartialEq)]
enum Work {
    /// Nothing linked anywhere, so it offers Start project from issue.
    Nothing,
    /// Linked work on this Mac: each project, its coordinator and its linked tickets.
    Here(Vec<LinkedProject>),
    /// Work started on another Mac: who started it, and each linked PR's last status.
    Elsewhere {
        started_by: Vec<String>,
        prs: Vec<(String, String)>,
    },
}

#[derive(Debug, PartialEq)]
struct LinkedProject {
    id: String,
    name: String,
    coordinator: Option<String>,
    /// Each linked ticket's title, and its PR's number and state when it has one.
    tickets: Vec<(String, Option<(u64, PrState)>)>,
}

/// The issue's work: its links on this Mac with what they are here, else what the tracker's
/// `work` and `pr` events say another Mac started.
fn work(record: &IssueRecord, index: Option<&WorkspaceIndex>) -> Work {
    if let Some(index) = index.filter(|_| !record.links.is_empty()) {
        let mut projects: Vec<LinkedProject> = vec![];
        for link in &record.links {
            let Some(project) = index.project(&link.project_id) else {
                continue;
            };
            let coordinator = index.roots(&project.id).next().map(|s| s.id.clone());
            let tickets = (coordinator.iter())
                .flat_map(|root| index.tickets_owned_by(root))
                .filter(|t| link.ticket_id.as_ref().is_none_or(|only| *only == t.id))
                .map(|t| {
                    let pr = t.pull_request.as_ref().map(|pr| (pr.number, pr.state));
                    (t.title.clone(), pr)
                })
                .collect::<Vec<_>>();
            match projects.iter_mut().find(|p| p.id == project.id) {
                Some(listed) => listed.tickets.extend(tickets),
                None => projects.push(LinkedProject {
                    id: project.id.clone(),
                    name: project.name.clone(),
                    coordinator,
                    tickets,
                }),
            }
        }
        // A ticket linked by itself and through its project shows once.
        for project in &mut projects {
            let mut shown = vec![];
            project.tickets.retain(|ticket| {
                let new = !shown.contains(ticket);
                shown.push(ticket.clone());
                new
            });
        }
        return Work::Here(projects);
    }
    let mut started_by: Vec<String> = vec![];
    let mut prs: Vec<(String, String)> = vec![];
    for event in &record.events {
        let field = |key: &str| event.fields.get(key).and_then(|v| v.as_str());
        match (event.kind.as_str(), field("url"), field("status")) {
            ("work", ..) if !started_by.contains(&event.by) => started_by.push(event.by.clone()),
            ("pr", Some(url), Some(status)) => {
                prs.retain(|(seen, _)| seen != url);
                prs.push((url.to_owned(), status.to_owned()));
            }
            _ => {}
        }
    }
    match started_by.is_empty() && prs.is_empty() {
        true => Work::Nothing,
        false => Work::Elsewhere { started_by, prs },
    }
}

fn pr_state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Closed => "closed",
        PrState::Merged => "merged",
    }
}

/// The issues this one names, with how: `blocks`, `blocked by`, `related to`, `duplicate of`.
fn related(issue: &Issue) -> Vec<(&'static str, String)> {
    let mut related = vec![];
    for (how, ids) in [
        ("blocks", &issue.blocks),
        ("blocked by", &issue.blocked_by),
        ("related to", &issue.related),
    ] {
        related.extend(ids.iter().map(|id| (how, id.clone())));
    }
    related.extend((issue.duplicate_of.iter()).map(|id| ("duplicate of", id.clone())));
    related
}

/// The `@login` mentions and issue IDs a draft names, each once, in order.
fn references(text: &str) -> (Vec<String>, Vec<String>) {
    let (mut mentions, mut ids): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let words = text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '@'));
    for word in words {
        let word = word.trim_matches('-');
        if let Some(login) = word.strip_prefix('@')
            && !login.is_empty()
            && !login.contains('@')
        {
            let mention = word.to_owned();
            if !mentions.contains(&mention) {
                mentions.push(mention);
            }
        } else if IssueId::parse(word).is_ok() && !ids.iter().any(|id| id == word) {
            ids.push(word.to_owned());
        }
    }
    (mentions, ids)
}

/// One line of the timeline, in time order.
#[derive(Debug, PartialEq)]
enum Entry<'a> {
    Filed {
        by: &'a str,
        at: &'a str,
    },
    Comment(&'a Comment),
    Event {
        by: &'a str,
        at: &'a str,
        text: String,
    },
}
impl Entry<'_> {
    fn at(&self) -> &str {
        match self {
            Self::Filed { at, .. } | Self::Event { at, .. } => at,
            Self::Comment(comment) => &comment.created,
        }
    }
}

/// The issue's filing, comments and events as one timeline, oldest first.
fn timeline<'a>(issue: &'a Issue, record: &'a IssueRecord) -> Vec<Entry<'a>> {
    let mut entries = vec![Entry::Filed {
        by: &issue.author,
        at: &issue.created,
    }];
    entries.extend(record.comments.iter().map(Entry::Comment));
    entries.extend(record.events.iter().map(|event| Entry::Event {
        by: &event.by,
        at: &event.at,
        text: event_text(event),
    }));
    // Stable, so the filing stays first among entries of the same second.
    entries.sort_by(|a, b| a.at().cmp(b.at()));
    entries
}

/// What an event did, after who did it: "moved this from Todo to In progress".
fn event_text(event: &IssueEvent) -> String {
    let field = |name: &str| event.fields.get(name).cloned().unwrap_or_default();
    let state = |name: &str| {
        serde_json::from_value::<IssueState>(field(name)).map_or("another state", |s| s.label())
    };
    let automatic = field("auto") == serde_json::json!(true);
    match event.kind.as_str() {
        "state" => format!(
            "moved this from {} to {}{}",
            state("from"),
            state("to"),
            if automatic { " automatically" } else { "" }
        ),
        "priority" => {
            let priority = serde_json::from_value::<Priority>(field("to")).unwrap_or_default();
            format!("set priority to {}", priority_label(priority))
        }
        "assignee" => match field("to").as_str() {
            Some(login) => format!("assigned {login}"),
            None => "removed the assignee".into(),
        },
        "work" => "started a project".into(),
        "pr" => match (field("url").as_str(), field("status").as_str()) {
            (Some(url), Some(status)) => format!("linked {url} · {status}"),
            _ => "linked a pull request".into(),
        },
        "link" => "linked work".into(),
        other => format!("changed {other}"),
    }
}

/// How long ago an RFC 3339 time was, or the time as written when it does not parse.
pub(super) fn ago(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map_or_else(|_| at.to_owned(), |at| wording::age(at.timestamp_millis()))
}

impl Render for IssueView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let crumbs = div()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(font::SMALL))
            .text_color(p.subtle)
            .child(
                button("issue-back")
                    .xsmall()
                    .ghost()
                    .label("Issues")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(IssueViewEvent::Back))),
            )
            .child("›")
            .child(issue_id(&self.id, p));
        let record = match &self.record {
            None => return div().child(crumbs).child(hint("Reading…", p)),
            Some(Err(error)) => {
                return div()
                    .child(crumbs)
                    .child(hint(error.clone(), p).text_color(p.red));
            }
            Some(Ok(record)) => record.clone(),
        };
        let paused = (!record.paused.is_empty()).then(|| {
            hint(
                format!(
                    "{} of this issue’s files changed on this Mac outside Wiffletree or on two Macs at once. They stay here, unpushed, until you settle them in the Inbox.",
                    wording::counted(record.paused.len(), "file")
                ),
                p,
            )
            .text_color(p.yellow)
        });
        let Some(issue) = &record.issue else {
            let problem = record.problem.clone().unwrap_or_default();
            return div()
                .flex()
                .flex_col()
                .gap_3()
                .child(crumbs)
                .children(paused)
                .child(
                    ui::banner(p.yellow).child(format!(
                        "This issue.md can’t be read: {problem}. It is shown as written, and Wiffletree won’t change it."
                    )),
                )
                .child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(p.surface)
                        .font_family(".SystemMonoFont")
                        .text_size(px(font::SMALL))
                        .whitespace_normal()
                        .child(record.raw.clone().unwrap_or_default()),
                );
        };
        let mut thread = div().flex().flex_col().gap_2();
        for entry in timeline(issue, &record) {
            thread = thread.child(match entry {
                Entry::Filed { by, at } => event_line(by, "filed this", at, p),
                Entry::Event { by, at, text } => event_line(by, &text, at, p),
                Entry::Comment(comment) => comment_card(comment, p),
            });
        }
        let main = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(font::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(issue.title.clone()),
            )
            .child(if issue.description.is_empty() {
                hint("No description.", p)
            } else {
                div().whitespace_normal().child(issue.description.clone())
            })
            .child(thread)
            .child(self.composer(p, cx));
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(crumbs)
            .children(paused)
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_6()
                    .child(main)
                    .child(self.properties(issue, p, cx)),
            )
    }
}

/// An issue's ID, greyed while it is temporary.
pub(super) fn issue_id(id: &IssueId, p: Palette) -> Div {
    let shown = mono(id.to_string(), p);
    match id.is_temporary() {
        true => shown.opacity(0.6),
        false => shown,
    }
}

fn event_line(by: &str, text: &str, at: &str, p: Palette) -> Div {
    div()
        .text_size(px(font::SMALL))
        .text_color(p.subtle)
        .child(format!("{by} {text} · {}", ago(at)))
}

fn comment_card(comment: &Comment, p: Palette) -> Div {
    let who = match &comment.assistant_for {
        Some(login) => format!("Assistant for {login}"),
        None => comment.author.clone(),
    };
    let edited = comment.edited.as_ref().map_or("", |_| " · edited");
    card(p)
        .when(comment.assistant_for.is_some(), |d| {
            d.border_color(p.magenta.opacity(0.4))
        })
        .child(
            div()
                .text_size(px(font::SMALL))
                .text_color(p.subtle)
                .child(format!("{who} · {}{edited}", ago(&comment.created))),
        )
        .child(div().whitespace_normal().child(comment.body.clone()))
}

impl Requests for IssueView {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Entry, LinkedProject, Work, assignee_options, event_text, references, related, timeline,
        work,
    };
    use serde_json::json;
    use workspace_core::*;

    #[test]
    fn the_assignee_picker_names_people_by_profile_and_keeps_an_assignee_it_does_not_list() {
        let people = [
            IssuePerson {
                login: "jlin".into(),
                name: None,
            },
            IssuePerson {
                login: "pshah".into(),
                name: Some("Priya Shah".into()),
            },
        ];
        let labels: Vec<(Option<String>, String)> = (assignee_options(&people, Some("okim")))
            .into_iter()
            .map(|(login, label)| (login, label.to_string()))
            .collect();
        assert_eq!(
            labels,
            [
                (None, "Nobody".into()),
                (Some("okim".into()), "okim".into()),
                (Some("jlin".into()), "jlin".into()),
                (Some("pshah".into()), "Priya Shah (pshah)".into()),
            ]
        );
        assert_eq!(assignee_options(&people, Some("PShah")).len(), 3);
    }

    fn issue() -> Issue {
        Issue {
            id: IssueId::parse("CEL-151").unwrap(),
            title: "Delivery webhook retries forever on a malformed payload".into(),
            state: IssueState::Todo,
            priority: Priority::Urgent,
            assignee: None,
            labels: vec!["bug".into()],
            repositories: vec!["celadon-theme/celadon-chat".into()],
            blocks: vec!["CEL-148".into()],
            blocked_by: vec![],
            related: vec!["see the wiki".into()],
            duplicate_of: None,
            author: "treyes".into(),
            created: "2026-10-29T14:00:00Z".into(),
            description: "A payload without message.id gets 500.".into(),
            extra: Extra::new(),
        }
    }
    fn event(at: &str, kind: &str, fields: serde_json::Value) -> IssueEvent {
        IssueEvent {
            at: at.into(),
            by: "jlin".into(),
            kind: kind.into(),
            fields: fields.as_object().unwrap().clone(),
        }
    }

    #[test]
    fn the_timeline_puts_the_filing_comments_and_events_in_time_order() {
        let issue = issue();
        let comment = Comment {
            file: "2026-10-29T14-08-00Z-pshah.md".into(),
            author: "pshah".into(),
            created: "2026-10-29T14:08:00Z".into(),
            edited: None,
            assistant_for: None,
            body: "Seeing it too.".into(),
        };
        let record = IssueRecord {
            tracker: "celadon-theme/storage".into(),
            id: issue.id.clone(),
            issue: Some(issue.clone()),
            raw: None,
            problem: None,
            comments: vec![comment.clone()],
            events: vec![event(
                "2026-10-29T14:17:00Z",
                "priority",
                json!({"from": "none", "to": "urgent"}),
            )],
            paused: vec![],
            links: vec![],
            following: false,
        };
        let entries = timeline(&issue, &record);
        assert_eq!(
            entries,
            [
                Entry::Filed {
                    by: "treyes",
                    at: "2026-10-29T14:00:00Z"
                },
                Entry::Comment(&comment),
                Entry::Event {
                    by: "jlin",
                    at: "2026-10-29T14:17:00Z",
                    text: "set priority to Urgent".into()
                },
            ]
        );
    }

    #[test]
    fn events_read_as_what_changed() {
        let at = "2026-10-29T14:17:00Z";
        let moved = event(at, "state", json!({"from": "todo", "to": "in_progress"}));
        assert_eq!(event_text(&moved), "moved this from Todo to In progress");
        let auto = event(
            at,
            "state",
            json!({"from": "in_progress", "to": "in_review", "auto": true}),
        );
        assert_eq!(
            event_text(&auto),
            "moved this from In progress to In review automatically"
        );
        let assigned = event(at, "assignee", json!({"from": null, "to": "pshah"}));
        assert_eq!(event_text(&assigned), "assigned pshah");
        let unassigned = event(at, "assignee", json!({"from": "pshah", "to": null}));
        assert_eq!(event_text(&unassigned), "removed the assignee");
        let pr = event(
            at,
            "pr",
            json!({"url": "https://github.com/o/r/pull/412", "status": "open"}),
        );
        assert_eq!(
            event_text(&pr),
            "linked https://github.com/o/r/pull/412 · open"
        );
        assert_eq!(
            event_text(&event(at, "estimate", json!({}))),
            "changed estimate"
        );
    }

    #[test]
    fn a_draft_names_its_mentions_and_issue_ids_once_each() {
        let (mentions, ids) =
            references("@pshah can you check CEL-148 and CEL-new-qxvb? cc @pshah, CEL-148.");
        assert_eq!(mentions, ["@pshah"]);
        assert_eq!(ids, ["CEL-148", "CEL-new-qxvb"]);
        let (mentions, ids) = references("mail me@example.com about cel-1");
        assert!(
            mentions.is_empty() && ids.is_empty(),
            "{mentions:?} {ids:?}"
        );
    }

    #[test]
    fn related_issues_say_how_and_keep_what_was_written_by_hand() {
        assert_eq!(
            related(&issue()),
            [
                ("blocks", "CEL-148".to_owned()),
                ("related to", "see the wiki".to_owned())
            ]
        );
    }

    fn record(links: Vec<IssueLink>, events: Vec<IssueEvent>) -> IssueRecord {
        IssueRecord {
            tracker: "celadon-theme/storage".into(),
            id: issue().id,
            issue: Some(issue()),
            raw: None,
            problem: None,
            comments: vec![],
            events,
            paused: vec![],
            links,
            following: false,
        }
    }

    #[test]
    fn the_work_box_names_who_started_work_elsewhere_and_each_prs_last_status() {
        assert_eq!(work(&record(vec![], vec![]), None), Work::Nothing);
        let url = "https://github.com/celadon-theme/celadon-chat/pull/412";
        let mut started = event("2026-10-29T15:00:00Z", "work", json!({"action":"started"}));
        started.by = "pshah".into();
        let events = vec![
            started,
            event(
                "2026-10-29T16:00:00Z",
                "pr",
                json!({"url":url,"status":"open"}),
            ),
            event(
                "2026-10-30T09:00:00Z",
                "pr",
                json!({"url":url,"status":"merged"}),
            ),
        ];
        assert_eq!(
            work(&record(vec![], events), None),
            Work::Elsewhere {
                started_by: vec!["pshah".into()],
                prs: vec![(url.into(), "merged".into())],
            }
        );
    }

    #[test]
    fn the_work_box_lists_the_linked_project_here_with_its_tickets_once() {
        let (_home, host, root, ticket) = crate::test_support::ticket_store();
        let index = WorkspaceIndex::new(host.snapshot().unwrap(), Default::default());
        let link = |ticket_id: Option<String>| IssueLink {
            tracker: "celadon-theme/storage".into(),
            issue: issue().id,
            project_id: root.project_id.clone(),
            ticket_id,
        };
        let linked = record(vec![link(None), link(Some(ticket.id.clone()))], vec![]);
        assert_eq!(
            work(&linked, Some(&index)),
            Work::Here(vec![LinkedProject {
                id: root.project_id.clone(),
                name: "Calls".into(),
                coordinator: Some(root.id.clone()),
                tickets: vec![("Fix inbound calls".into(), None)],
            }])
        );
    }
}
