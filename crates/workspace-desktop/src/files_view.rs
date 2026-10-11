//! The Files panel: the files an agent read and edited during its ticket, live, or every
//! agent's files rolled up for the project coordinator. Fetched only while the panel is open.
use super::*;
use activity::{StepFetch, paced_reply};
use std::time::Duration;
use ui::{empty_state, hint, icon, pill, segment};

/// File fetches start no closer together than this; each one reads Git off the host thread.
const FILES_PACE: Duration = Duration::from_millis(500);
const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 14.;
/// Live reads and writes listed in the strip before the rest are counted.
const MAX_LIVE: usize = 4;

/// A row of the file tree. A directory with a single directory inside is one row, "core/src".
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Node {
    name: String,
    /// The path from the tree's root; for a file, the path it was built from.
    path: String,
    dir: bool,
    children: Vec<Node>,
}

/// Builds the tree from flat paths: shared directories merge, and directories sort before
/// files, each by name, so rows stay put as activity changes.
pub(super) fn tree<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<Node> {
    #[derive(Default)]
    struct Dir<'a> {
        dirs: BTreeMap<&'a str, Dir<'a>>,
        files: BTreeSet<&'a str>,
    }
    fn nodes(dir: Dir, parent: &str) -> Vec<Node> {
        let under = |name: &str| match parent {
            "" => name.to_owned(),
            _ => format!("{parent}/{name}"),
        };
        let mut rows = vec![];
        for (name, mut child) in dir.dirs {
            let mut name = name.to_owned();
            while child.files.is_empty() && child.dirs.len() == 1 {
                let (only, inner) = child.dirs.pop_first().unwrap();
                name = format!("{name}/{only}");
                child = inner;
            }
            let path = under(&name);
            rows.push(Node {
                children: nodes(child, &path),
                name,
                path,
                dir: true,
            });
        }
        rows.extend(dir.files.into_iter().map(|name| Node {
            name: name.to_owned(),
            path: under(name),
            dir: false,
            children: vec![],
        }));
        rows
    }
    let mut root = Dir::default();
    for path in paths {
        // A path outside the worktree is absolute; its first row keeps the leading slash.
        let slashes = path.len() - path.trim_start_matches('/').len();
        let first = path[slashes..]
            .find('/')
            .map_or(path.len(), |at| slashes + at);
        let mut parts: Vec<&str> = vec![&path[..first]];
        parts.extend(path[first..].split('/').filter(|part| !part.is_empty()));
        let Some(file) = parts.pop() else {
            continue;
        };
        let mut dir = &mut root;
        for part in parts {
            dir = dir.dirs.entry(part).or_default();
        }
        dir.files.insert(file);
    }
    nodes(root, "")
}

/// What a file's row shows beside its name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Mark {
    reads: u32,
    edits: u32,
    reading: bool,
    writing: bool,
    /// Lines added and removed against the ticket base.
    lines: Option<(u32, u32)>,
    /// The agent on the file, or who shares it when `clash`.
    who: Option<String>,
    clash: bool,
}
impl Mark {
    fn add(&mut self, file: &FileActivity) {
        self.reads += file.reads;
        self.edits += file.edits;
        self.reading |= file.reading;
        self.writing |= file.writing;
    }
}

/// One tree in the panel: an agent's ticket, or a ticket or agent of the coordinator's rollup.
#[derive(Debug)]
struct Group {
    key: String,
    /// The rollup's heading; the agent view has one group and no heading.
    title: Option<String>,
    stat: String,
    marks: BTreeMap<String, Mark>,
    nodes: Vec<Node>,
    /// Lists the whole worktree, so only directories holding a marked file start open.
    whole: bool,
}

/// A read or write running now, for the strip under the tree.
#[derive(Debug, PartialEq, Eq)]
struct Live {
    who: Option<String>,
    writing: bool,
    file: String,
}

/// Who else touched the file: other agents on its ticket, or else another ticket in the same
/// repository.
fn shared(report: &[TicketFiles], ticket: &TicketFiles, path: &str) -> Option<String> {
    let agents_on = |t: &TicketFiles| {
        let touched = |a: &&AgentFiles| a.files.iter().any(|f| f.path == path);
        t.agents.iter().filter(touched).count()
    };
    let agents = agents_on(ticket);
    if agents > 1 {
        return Some(format!("{agents} agents"));
    }
    report
        .iter()
        .find(|t| {
            t.ticket_id != ticket.ticket_id
                && t.repository_id == ticket.repository_id
                && agents_on(t) > 0
        })
        .map(|t| format!("also in {}", t.title))
}

fn groups(report: &[TicketFiles], rollup: bool, by_agent: bool) -> Vec<Group> {
    let mut groups = vec![];
    for ticket in report {
        let lines = |path: &str| {
            let change = ticket.changes.iter().find(|c| c.path == path)?;
            Some((change.added, change.removed))
        };
        if rollup && by_agent {
            for agent in &ticket.agents {
                let mark = |file: &FileActivity| {
                    let mut mark = Mark::default();
                    mark.add(file);
                    if file.edits > 0 || file.writing {
                        mark.lines = lines(&file.path);
                    }
                    mark.who = shared(report, ticket, &file.path);
                    mark.clash = mark.who.is_some();
                    (file.path.clone(), mark)
                };
                let marks: BTreeMap<_, _> = agent.files.iter().map(mark).collect();
                groups.push(Group {
                    key: format!("{}:{}", ticket.ticket_id, agent.session_id),
                    title: Some(format!("{} · {}", agent.name, ticket.title)),
                    stat: format!("{} files", marks.len()),
                    nodes: tree(marks.keys().map(String::as_str)),
                    marks,
                    whole: false,
                });
            }
            continue;
        }
        let mut marks: BTreeMap<String, Mark> = BTreeMap::new();
        for agent in &ticket.agents {
            for file in &agent.files {
                let mark = marks.entry(file.path.clone()).or_default();
                mark.add(file);
                if rollup {
                    mark.who = Some(agent.name.clone());
                }
            }
        }
        if rollup {
            for (path, mark) in &mut marks {
                if let Some(others) = shared(report, ticket, path) {
                    (mark.who, mark.clash) = (Some(others), true);
                }
            }
        }
        // Only the implementer writes in the worktree, so changes no step named, such as a
        // shell command's, are its own. A verifier's view keeps to the files it touched.
        let writer = |a: &AgentFiles| a.role == Role::Implementer;
        let all_changes = rollup || ticket.agents.first().is_some_and(writer);
        for change in &ticket.changes {
            let lines = Some((change.added, change.removed));
            if all_changes {
                marks.entry(change.path.clone()).or_default().lines = lines;
            } else if let Some(mark) = marks.get_mut(&change.path) {
                mark.lines = lines;
            }
        }
        let listed = ticket.tree.iter().flatten().map(String::as_str);
        groups.push(Group {
            key: ticket.ticket_id.clone(),
            title: rollup.then(|| format!("{} · {}", ticket.title, ticket.repository)),
            stat: format!("{} changed", ticket.changes.len()),
            nodes: tree(marks.keys().map(String::as_str).chain(listed)),
            marks,
            whole: ticket.tree.is_some(),
        });
    }
    groups
}

fn live(report: &[TicketFiles], rollup: bool) -> Vec<Live> {
    let mut live = vec![];
    for agent in report.iter().flat_map(|t| &t.agents) {
        for file in agent.files.iter().filter(|f| f.reading || f.writing) {
            live.push(Live {
                who: rollup.then(|| agent.name.clone()),
                writing: file.writing,
                file: file.path.rsplit('/').next().unwrap_or_default().to_owned(),
            });
        }
    }
    live
}

/// The line under the panel's title: what the tree below adds up to.
fn summary(report: &[TicketFiles], rollup: bool) -> Option<String> {
    let ticket = report.first()?;
    if !rollup {
        let read = |f: &&FileActivity| f.reads > 0 || f.reading;
        let read = ticket.agents.iter().flat_map(|a| &a.files).filter(read);
        return Some(format!(
            "{} · {} read · {} changed this ticket",
            ticket.repository,
            read.count(),
            ticket.changes.len()
        ));
    }
    let agents = || report.iter().flat_map(|t| &t.agents);
    Some(format!(
        "{} tickets · {} agents · {} working now",
        report.len(),
        agents().count(),
        agents().filter(|a| a.working).count()
    ))
}

#[derive(Default)]
pub(super) struct Files {
    fetch: StepFetch,
    /// The session the report answers for; another selection shows nothing until its own.
    session_id: Option<String>,
    /// The report is a coordinator's rollup, not one agent's ticket.
    rollup: bool,
    report: Vec<TicketFiles>,
    groups: Vec<Group>,
    live: Vec<Live>,
    whole_tree: bool,
    by_agent: bool,
    /// Why the last request failed; shown until one succeeds.
    error: Option<String>,
    /// Groups and directories the user flipped from their default of open or closed.
    toggled: BTreeSet<String>,
}
impl Files {
    fn regroup(&mut self) {
        self.groups = groups(&self.report, self.rollup, self.by_agent);
        self.live = live(&self.report, self.rollup);
    }
    fn is_open(&self, group: &Group, dir: &Node) -> bool {
        let inside = format!("{}/", dir.path);
        let holds_mark = || {
            let mut marked = group.marks.range::<str, _>((
                std::ops::Bound::Included(inside.as_str()),
                std::ops::Bound::Unbounded,
            ));
            marked
                .next()
                .is_some_and(|(path, _)| path.starts_with(&inside))
        };
        let open = !group.whole || holds_mark();
        open != self.toggled.contains(&row_key(group, &dir.path))
    }
}

fn row_key(group: &Group, path: &str) -> String {
    format!("{}/{path}", group.key)
}

fn pulse(id: String, color: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(7.))
        .rounded_full()
        .bg(color)
        .with_animation(
            SharedString::from(id),
            Animation::new(Duration::from_millis(1100))
                .repeat()
                .with_easing(pulsating_between(0.25, 1.)),
            |dot, opacity| dot.opacity(opacity),
        )
}

fn file_name(name: &str, color: Hsla) -> Div {
    div()
        .min_w_0()
        .font_family(".SystemMonoFont")
        .text_size(px(12.))
        .text_color(color)
        .text_ellipsis()
        .child(name.to_owned())
}

/// The word and colour for a write or a read in progress.
fn doing(writing: bool, p: Palette) -> (&'static str, Hsla) {
    if writing {
        ("writing", p.accent)
    } else {
        ("reading", p.blue)
    }
}

/// "edited ×2", "read", or what is happening to the file now.
fn badge(mark: &Mark, p: Palette) -> Option<Div> {
    let counted = |verb: &str, count: u32| match count {
        1 => verb.to_owned(),
        _ => format!("{verb} ×{count}"),
    };
    let (text, color) = match mark {
        Mark { writing: true, .. } | Mark { reading: true, .. } => {
            let (verb, color) = doing(mark.writing, p);
            (format!("{verb}…"), color)
        }
        Mark { edits: 1.., .. } => (counted("edited", mark.edits), p.accent),
        Mark { reads: 1.., .. } => (counted("read", mark.reads), p.blue),
        _ => return None,
    };
    Some(pill(text, color))
}

impl Workspace {
    /// Asks for the selected session's files, only while the Files panel shows them.
    pub(super) fn fetch_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel != Some(Panel::Files) {
            return;
        }
        let Some(session) = self.selected_session() else {
            return;
        };
        let (session_id, agent) = (session.id.clone(), session.role.is_worker());
        let Some(wait) = self.files.fetch.begin(FILES_PACE) else {
            return;
        };
        let command = Command::Files {
            session_id: session_id.clone(),
            whole_tree: agent && self.files.whole_tree,
        };
        let reply = paced_reply(
            self.bridge.clone(),
            command,
            wait,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.accept_files(&session_id, result, window, cx)
                })
            });
        })
        .detach();
    }

    fn accept_files(
        &mut self,
        session_id: &str,
        result: Result<Vec<TicketFiles>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owed = self.files.fetch.finish();
        let selected = self.selected_session().filter(|s| s.id == session_id);
        match (result, selected.map(|s| s.role)) {
            (Ok(report), Some(role)) => {
                self.files.session_id = Some(session_id.to_owned());
                self.files.rollup = role == Role::ProjectOrchestrator;
                self.files.report = report;
                self.files.error = None;
                self.files.regroup();
                cx.notify();
            }
            (Err(error), Some(_)) => {
                eprintln!("Files: {error}");
                self.files.session_id = Some(session_id.to_owned());
                self.files.report.clear();
                self.files.error = Some(error);
                self.files.regroup();
                cx.notify();
            }
            (Ok(_), None) => {}
            (Err(error), None) => eprintln!("Files: {error}"),
        }
        if owed {
            self.fetch_files(window, cx);
        }
    }

    /// The loaded report, when it answers for the selected session.
    fn files(&self) -> Option<&Files> {
        Some(&self.files)
            .filter(|files| files.session_id.is_some() && files.session_id == self.selected)
    }

    /// The counts and the view choice, pinned above the tree.
    pub(super) fn files_head(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let files = self.files()?;
        let summary = summary(&files.report, files.rollup)?;
        let mut choices = div().flex().gap_1();
        let labels = if files.rollup {
            ["By ticket", "By agent"]
        } else {
            ["Touched", "Whole tree"]
        };
        let chosen = if files.rollup {
            files.by_agent
        } else {
            files.whole_tree
        };
        for (second, label) in [false, true].into_iter().zip(labels) {
            choices = choices.child(
                segment(
                    SharedString::from(format!("files-view-{label}")),
                    label,
                    chosen == second,
                )
                .on_click(cx.listener(move |v, _, w, c| {
                    if v.files.rollup {
                        v.files.by_agent = second;
                        v.files.regroup();
                    } else {
                        v.files.whole_tree = second;
                        v.fetch_files(w, c);
                    }
                    c.notify();
                })),
            );
        }
        Some(
            div()
                .flex_none()
                .px_5()
                .py_3()
                .flex()
                .flex_col()
                .gap_2()
                .border_b_1()
                .border_color(p.edge.opacity(0.3))
                .child(hint(summary, p))
                .child(choices),
        )
    }

    /// What is being read and written now, pinned under the tree.
    pub(super) fn files_live(&self, p: Palette) -> Option<Div> {
        let live = &self.files()?.live;
        if live.is_empty() {
            return None;
        }
        let mut strip = div()
            .flex_none()
            .px_5()
            .py_2()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.edge.opacity(0.3))
            .text_size(px(11.5))
            .text_color(p.subtle);
        for (ix, entry) in live.iter().take(MAX_LIVE).enumerate() {
            let (verb, color) = doing(entry.writing, p);
            let doing = match &entry.who {
                Some(who) => format!("{who} {verb}"),
                None => verb.to_owned(),
            };
            strip = strip.child(
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(pulse(format!("files-live-{ix}"), color))
                    .child(div().flex_none().child(doing))
                    .child(file_name(&entry.file, p.text)),
            );
        }
        if live.len() > MAX_LIVE {
            strip = strip.child(hint(format!("+{} more", live.len() - MAX_LIVE), p));
        }
        Some(strip)
    }

    pub(super) fn files_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(files) = self.files() else {
            return hint("Loading files…", p);
        };
        if let Some(error) = &files.error {
            return empty_state("folder", "Files unavailable", error.clone(), p);
        }
        if files.groups.is_empty() {
            return if files.rollup {
                empty_state(
                    "folder",
                    "No open tickets",
                    "Tickets appear here with the files their agents read and change.",
                    p,
                )
            } else {
                empty_state(
                    "folder",
                    "No ticket",
                    "Files appear here for agents working in a ticket's worktree.",
                    p,
                )
            };
        }
        if !files.rollup && files.groups[0].nodes.is_empty() {
            return empty_state(
                "folder",
                "Nothing touched yet",
                "Files this agent reads or edits appear here as it works.",
                p,
            );
        }
        let mut rows = vec![];
        for group in &files.groups {
            let Some(title) = &group.title else {
                self.file_rows(group, &group.nodes, 0, p, cx, &mut rows);
                continue;
            };
            let key = row_key(group, "");
            let open = !files.toggled.contains(&key);
            let heading = self
                .folder_row(key, title, 0, open, p, cx)
                .mt(px(6.))
                .text_color(p.text)
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(p.subtle)
                        .child(group.stat.clone()),
                );
            rows.push(heading.into_any_element());
            if open && group.nodes.is_empty() {
                rows.push(
                    hint("No files touched yet.", p)
                        .pl(px(8. + INDENT))
                        .into_any_element(),
                );
            } else if open {
                self.file_rows(group, &group.nodes, 1, p, cx, &mut rows);
            }
        }
        div().flex().flex_col().children(rows)
    }

    /// A ticket, agent or directory row; clicking it opens or closes what is under it.
    fn folder_row(
        &self,
        key: String,
        label: &str,
        depth: usize,
        open: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let chevron = if open {
            "chevron-down"
        } else {
            "chevron-right"
        };
        div()
            .id(SharedString::from(format!("files-row-{key}")))
            .h(px(ROW_HEIGHT))
            .pl(px(8. + INDENT * depth as f32))
            .pr_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_md()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(p.subtle)
            .hover(move |d| d.bg(p.overlay))
            .on_click(cx.listener(move |v, _, _, c| {
                if !v.files.toggled.remove(&key) {
                    v.files.toggled.insert(key.clone());
                }
                c.notify();
            }))
            .child(icon(chevron).size(px(12.)).flex_none())
            .child(div().min_w_0().text_ellipsis().child(label.to_owned()))
    }

    fn file_rows(
        &self,
        group: &Group,
        nodes: &[Node],
        depth: usize,
        p: Palette,
        cx: &mut Context<Self>,
        rows: &mut Vec<AnyElement>,
    ) {
        for node in nodes {
            if node.dir {
                let open = self.files.is_open(group, node);
                let key = row_key(group, &node.path);
                let row = self.folder_row(key, &node.name, depth, open, p, cx);
                rows.push(row.font_family(".SystemMonoFont").into_any_element());
                if open {
                    self.file_rows(group, &node.children, depth + 1, p, cx, rows);
                }
            } else {
                let mark = group.marks.get(&node.path);
                rows.push(Self::file_row(group, node, mark, depth, p).into_any_element());
            }
        }
    }

    fn file_row(group: &Group, node: &Node, mark: Option<&Mark>, depth: usize, p: Palette) -> Div {
        let unmarked = Mark::default();
        let mark = mark.unwrap_or(&unmarked);
        let live = mark.reading || mark.writing;
        let name_color = if *mark == unmarked { p.subtle } else { p.text };
        div()
            .h(px(ROW_HEIGHT))
            // Files line up with the names of the directories beside them, past the chevron.
            .pl(px(8. + INDENT * depth as f32 + if live { 0. } else { 18. }))
            .pr_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_md()
            .when(live, |d| {
                let (_, color) = doing(mark.writing, p);
                d.bg(p.accent.opacity(0.10))
                    .child(div().w(px(12.)).flex().justify_center().child(pulse(
                        format!("files-pulse-{}", row_key(group, &node.path)),
                        color,
                    )))
            })
            // The name takes what the counts, tag and badge leave, and is cut short last.
            .child(file_name(&node.name, name_color).flex_1())
            .when_some(mark.lines.filter(|_| !live), |d, (added, removed)| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .gap_1()
                        .text_size(px(10.5))
                        .child(div().text_color(p.green).child(format!("+{added}")))
                        .child(div().text_color(p.red).child(format!("−{removed}"))),
                )
            })
            .when_some(mark.who.clone(), |d, who| {
                let (text, fill) = if mark.clash {
                    (p.yellow, p.yellow.opacity(0.14))
                } else {
                    (p.subtle, p.overlay)
                };
                d.child(
                    div()
                        .flex_none()
                        .max_w(px(120.))
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_sm()
                        .bg(fill)
                        .text_color(text)
                        .text_size(px(10.5))
                        .text_ellipsis()
                        .child(who),
                )
            })
            .children(badge(mark, p))
    }
}

#[cfg(test)]
mod tests {
    use super::{Files, Group, Node, groups, row_key, tree};
    use workspace_core::{AgentFiles, FileActivity, LineCount, Role, TicketFiles};

    /// The tree as indented names, directories ending in "/".
    fn outline(nodes: &[Node], depth: usize, out: &mut Vec<String>) {
        for node in nodes {
            let slash = if node.dir { "/" } else { "" };
            out.push(format!("{}{}{slash}", "  ".repeat(depth), node.name));
            outline(&node.children, depth + 1, out);
        }
    }

    #[test]
    fn flat_paths_become_one_tree_with_shared_directories_merged_in_a_stable_order() {
        let paths = [
            "docs/PRD.md",
            "crates/host/src/stream.rs",
            "README.md",
            "crates/core/src/steps.rs",
            "crates/host/src/steps.rs",
            "crates/host/tests/files.rs",
            "/tmp/notes.txt",
        ];
        let mut reversed = paths;
        reversed.reverse();
        let built = tree(paths);
        assert_eq!(built, tree(reversed));
        let mut lines = vec![];
        outline(&built, 0, &mut lines);
        assert_eq!(
            lines,
            [
                "/tmp/",
                "  notes.txt",
                "crates/",
                "  core/src/",
                "    steps.rs",
                "  host/",
                "    src/",
                "      steps.rs",
                "      stream.rs",
                "    tests/",
                "      files.rs",
                "docs/",
                "  PRD.md",
                "README.md",
            ]
        );
        // A file's path is the one it came from, so its activity can be found again.
        let host_src = &built[1].children[1].children[0];
        assert_eq!(host_src.path, "crates/host/src");
        assert_eq!(host_src.children[1].path, "crates/host/src/stream.rs");
        assert_eq!(built[0].children[0].path, "/tmp/notes.txt");
    }

    fn agent(name: &str, files: &[(&str, u32, u32)]) -> AgentFiles {
        let file = |&(path, reads, edits): &(&str, u32, u32)| FileActivity {
            path: path.into(),
            reads,
            edits,
            ..Default::default()
        };
        AgentFiles {
            session_id: name.into(),
            name: name.into(),
            role: match name {
                "Implementer" => Role::Implementer,
                _ => Role::Reviewer,
            },
            working: false,
            files: files.iter().map(file).collect(),
        }
    }

    fn ticket(title: &str, repository: &str, agents: Vec<AgentFiles>) -> TicketFiles {
        TicketFiles {
            ticket_id: title.into(),
            title: title.into(),
            repository_id: repository.into(),
            repository: repository.into(),
            agents,
            changes: vec![],
            tree: None,
        }
    }

    fn who<'a>(group: &'a Group, path: &str) -> (Option<&'a str>, bool) {
        let mark = &group.marks[path];
        (mark.who.as_deref(), mark.clash)
    }

    #[test]
    fn the_rollup_names_one_agent_and_tags_a_file_shared_by_agents_or_tickets() {
        let report = [
            ticket(
                "Panel",
                "web",
                vec![
                    agent(
                        "Implementer",
                        &[("src/steps.rs", 1, 2), ("src/ui.rs", 0, 1)],
                    ),
                    agent("Reviewer", &[("src/steps.rs", 3, 0)]),
                ],
            ),
            ticket(
                "Paths",
                "web",
                vec![agent("Tester", &[("src/ui.rs", 1, 0)])],
            ),
            // The same relative path in another repository is another file.
            ticket(
                "Docs",
                "site",
                vec![agent("Writer", &[("src/ui.rs", 1, 0)])],
            ),
        ];
        let by_ticket = groups(&report, true, false);
        assert_eq!(who(&by_ticket[0], "src/steps.rs"), (Some("2 agents"), true));
        assert_eq!(by_ticket[0].marks["src/steps.rs"].reads, 4);
        assert_eq!(
            who(&by_ticket[0], "src/ui.rs"),
            (Some("also in Paths"), true)
        );
        assert_eq!(
            who(&by_ticket[1], "src/ui.rs"),
            (Some("also in Panel"), true)
        );
        assert_eq!(who(&by_ticket[2], "src/ui.rs"), (Some("Writer"), false));

        let by_agent = groups(&report, true, true);
        let titles: Vec<_> = by_agent
            .iter()
            .map(|g| g.title.as_deref().unwrap())
            .collect();
        assert_eq!(
            titles,
            [
                "Implementer · Panel",
                "Reviewer · Panel",
                "Tester · Paths",
                "Writer · Docs"
            ]
        );
        assert_eq!(who(&by_agent[1], "src/steps.rs"), (Some("2 agents"), true));
        assert_eq!(who(&by_agent[3], "src/ui.rs"), (None, false));
    }

    #[test]
    fn an_agent_view_lists_changes_no_step_named_and_opens_only_marked_directories() {
        let mut report = [ticket(
            "Panel",
            "web",
            vec![agent("Implementer", &[("src/a.rs", 1, 0)])],
        )];
        report[0].changes = vec![LineCount {
            path: "gen/out.rs".into(),
            added: 5,
            removed: 0,
        }];
        report[0].tree = Some(vec![
            "src/a.rs".into(),
            "gen/out.rs".into(),
            "docs/x.md".into(),
        ]);
        let mut files = Files {
            report: report.into(),
            ..Default::default()
        };
        files.regroup();
        let group = &files.groups[0];
        assert_eq!(group.marks["gen/out.rs"].lines, Some((5, 0)));
        assert_eq!(
            (group.marks["gen/out.rs"].reads, group.title.as_ref()),
            (0, None)
        );
        let open: Vec<_> = group
            .nodes
            .iter()
            .map(|dir| (dir.name.as_str(), files.is_open(group, dir)))
            .collect();
        assert_eq!(open, [("docs", false), ("gen", true), ("src", true)]);
        files.toggled.insert(row_key(group, "docs"));
        assert!(files.is_open(&files.groups[0], &files.groups[0].nodes[0]));
    }

    #[test]
    fn a_verifier_sees_only_its_own_files_while_the_implementer_and_rollup_see_every_change() {
        let changes = || {
            let count = |path: &str| LineCount {
                path: path.into(),
                added: 2,
                removed: 1,
            };
            vec![count("src/a.rs"), count("gen/out.rs")]
        };
        let paths = |agents: Vec<AgentFiles>, rollup: bool| {
            let mut report = [ticket("Panel", "web", agents)];
            report[0].changes = changes();
            let group = groups(&report, rollup, false).remove(0);
            let lines = |(path, mark): (&String, &super::Mark)| (path.clone(), mark.lines);
            group.marks.iter().map(lines).collect::<Vec<_>>()
        };
        let counted = Some((2, 1));
        let reviewer = || agent("Reviewer", &[("src/a.rs", 1, 0), ("docs/x.md", 1, 0)]);
        assert_eq!(
            paths(vec![reviewer()], false),
            [("docs/x.md".into(), None), ("src/a.rs".into(), counted)]
        );
        // With nothing touched, a verifier's tree is empty whatever the ticket changed.
        assert_eq!(paths(vec![agent("Reviewer", &[])], false), []);
        let every_change = [
            ("gen/out.rs".to_owned(), counted),
            ("src/a.rs".to_owned(), counted),
        ];
        assert_eq!(paths(vec![agent("Implementer", &[])], false), every_change);
        assert_eq!(paths(vec![agent("Reviewer", &[])], true), every_change);
    }
}
