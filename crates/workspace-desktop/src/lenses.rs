//! The sidebar's App, Layer and Me lenses: in each Space's section, buckets of project rows,
//! each row with a muted line beneath it. They are drawn over the host's records alone, and no
//! row in them is dragged.
use crate::preferences::Lens;
use crate::tree;
use std::collections::{BTreeMap, BTreeSet};
use workspace_core::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Bucket {
    /// An app or a layer of the Space's vocabulary.
    Tag(String),
    /// Projects with tickets but no app, or no layer.
    Untagged,
    NotStarted,
    Questions,
    MergeOnGitHub,
    GoingQuiet,
    ReadyToClose,
}

impl Bucket {
    const ME: [Bucket; 4] = [
        Bucket::Questions,
        Bucket::MergeOnGitHub,
        Bucket::GoingQuiet,
        Bucket::ReadyToClose,
    ];
    pub(crate) fn label(&self, lens: Lens) -> String {
        match self {
            Bucket::Tag(value) => value.clone(),
            Bucket::Untagged => tag_kind(lens).map_or("", TagKind::none_label).into(),
            Bucket::NotStarted => "not started".into(),
            Bucket::Questions => "Questions".into(),
            Bucket::MergeOnGitHub => "Merge on GitHub".into(),
            Bucket::GoingQuiet => "Going quiet".into(),
            Bucket::ReadyToClose => "Ready to close".into(),
        }
    }
    /// The App and Layer lenses show their buckets as chips.
    pub(crate) fn is_tag(&self) -> bool {
        matches!(self, Bucket::Tag(_) | Bucket::Untagged | Bucket::NotStarted)
    }
}

pub(crate) fn tag_kind(lens: Lens) -> Option<TagKind> {
    match lens {
        Lens::App => Some(TagKind::App),
        Lens::Layer => Some(TagKind::Layer),
        Lens::Outcome | Lens::Me => None,
    }
}

/// A project's row in a bucket. It owns its coordinator, so the sidebar can keep the Me lens's
/// rows from one snapshot to the next.
#[derive(Debug)]
pub(crate) struct Listed {
    pub root: Session,
    /// It is filed under a parent, so it has the branch icon.
    pub child: bool,
    /// The muted line beneath it: its parent's name, or why the Me lens lists it.
    pub line: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Group {
    pub bucket: Bucket,
    pub projects: Vec<Listed>,
}

/// One Space's buckets, each only when it holds a project.
#[derive(Debug)]
pub(crate) struct LensSection {
    pub space: SpaceId,
    pub name: String,
    pub groups: Vec<Group>,
}

impl LensSection {
    /// Its rows, a project once for each bucket it is in.
    pub(crate) fn entries(&self) -> usize {
        self.groups.iter().map(|group| group.projects.len()).sum()
    }
    /// Its projects' coordinators, each once, for a collapsed section's state.
    pub(crate) fn projects(&self) -> Vec<&Session> {
        let mut seen = BTreeSet::new();
        (self.groups.iter())
            .flat_map(|group| group.projects.iter().map(|listed| &listed.root))
            .filter(|root| seen.insert(root.project_id.as_str()))
            .collect()
    }
}

/// The App or Layer lens: in each Space's section, a bucket for each of its values of `kind`
/// that a shown project has, in the Space's order, then "no app" or "no layer", then "not
/// started". A project is listed in the host's order under every bucket its tags put it in, a
/// child with its parent's name beneath it.
pub(crate) fn tag_sections(
    index: &WorkspaceIndex,
    kind: TagKind,
    show_archived: bool,
) -> Vec<LensSection> {
    let snapshot = index.snapshot();
    let sections = tree::spaces(index).into_iter().map(|(space, name)| {
        let shown = tree::shown(index, &space, show_archived);
        let vocabulary = (snapshot.spaces.iter())
            .find(|listed| listed.id == space)
            .map_or(&[][..], |listed| listed.values(kind));
        let buckets = (vocabulary.iter().cloned().map(Bucket::Tag))
            .chain([Bucket::Untagged, Bucket::NotStarted]);
        let groups = buckets.map(|bucket| {
            let projects = (shown.iter())
                .filter(|(project, _)| holds(snapshot.project_tags.get(&project.id), kind, &bucket))
                .map(|&(project, root)| Listed {
                    root: root.clone(),
                    child: project.parent_id.is_some(),
                    line: (project.parent_id.as_deref())
                        .and_then(|parent| Some(index.project(parent)?.name.clone())),
                })
                .collect();
            Group { bucket, projects }
        });
        LensSection {
            space,
            name,
            groups: groups.filter(|group| !group.projects.is_empty()).collect(),
        }
    });
    sections
        .filter(|section| !section.groups.is_empty())
        .collect()
}

/// Whether a project with `tags` is listed under `bucket`. One a host older than tags sent none
/// for has not started as far as the lens can tell.
fn holds(tags: Option<&ProjectTags>, kind: TagKind, bucket: &Bucket) -> bool {
    match bucket {
        Bucket::Tag(value) => tags.is_some_and(|tags| tags.of(kind).contains(value)),
        Bucket::Untagged => tags.is_some_and(|tags| tags.has_none(kind)),
        Bucket::NotStarted => tags.is_none_or(|tags| !tags.started),
        _ => false,
    }
}

const DAY: i64 = 24 * 60 * 60 * 1000;
/// How long without a turn a project goes before the Me lens calls it quiet.
const QUIET_DAYS: i64 = 5;

/// The Me lens at `now`, in milliseconds: in each Space's section, its unarchived projects under
/// Questions, Merge on GitHub, Going quiet and Ready to close, each with its reason.
pub(crate) fn me_sections(index: &WorkspaceIndex, now: i64) -> Vec<LensSection> {
    let family_turns = family_turns(index);
    let sections = tree::spaces(index).into_iter().map(|(space, name)| {
        let shown = tree::shown(index, &space, false);
        let groups = Bucket::ME.into_iter().map(|bucket| {
            let projects = (shown.iter())
                .filter_map(|&(project, root)| {
                    let reason = reason(index, project, &bucket, &family_turns, now)?;
                    Some(Listed {
                        root: root.clone(),
                        child: project.parent_id.is_some(),
                        line: Some(reason),
                    })
                })
                .collect();
            Group { bucket, projects }
        });
        LensSection {
            space,
            name,
            groups: groups.filter(|group| !group.projects.is_empty()).collect(),
        }
    });
    sections
        .filter(|section| !section.groups.is_empty())
        .collect()
}

/// What the Me segment's badge counts: the Me lens's rows across every Space.
pub(crate) fn me_count(sections: &[LensSection]) -> usize {
    sections.iter().map(LensSection::entries).sum()
}

/// By project, when a session of it or of its children last started a turn, archived workers
/// included, as only the host knows without the archive.
fn family_turns(index: &WorkspaceIndex) -> BTreeMap<&str, i64> {
    let snapshot = index.snapshot();
    let mut latest = BTreeMap::new();
    for project in &snapshot.projects {
        let Some(turn) = (snapshot.me_facts.get(&project.id)).and_then(|f| f.last_turn_started)
        else {
            continue;
        };
        let family = std::iter::once(&project.id).chain(&project.parent_id);
        for id in family {
            let entry = latest.entry(id.as_str()).or_insert(turn);
            *entry = (*entry).max(turn);
        }
    }
    latest
}

/// Why the Me lens lists `project` under `bucket`; none when it does not.
fn reason(
    index: &WorkspaceIndex,
    project: &Project,
    bucket: &Bucket,
    family_turns: &BTreeMap<&str, i64>,
    now: i64,
) -> Option<String> {
    let snapshot = index.snapshot();
    let facts = snapshot.me_facts.get(&project.id);
    let ready_to_close = facts.is_some_and(|facts| facts.ready_to_close);
    match bucket {
        Bucket::Questions => questions(index, project),
        Bucket::MergeOnGitHub => (facts?.merges.first())
            .map(|merge| format!("PR #{} into {} passed review", merge.number, merge.base)),
        Bucket::GoingQuiet => {
            if ready_to_close {
                return None;
            }
            let latest = family_turns.get(project.id.as_str())?;
            let days = (now - latest) / DAY;
            let started = (index.roots(&project.id))
                .any(|root| snapshot.tickets_owned.get(&root.id).is_some_and(|&n| n > 0));
            let not_started = if started { "" } else { ", not started" };
            (days >= QUIET_DAYS).then(|| format!("No turn for {days} days{not_started}"))
        }
        Bucket::ReadyToClose => {
            ready_to_close.then(|| "Every ticket accepted or closed, PRs merged".into())
        }
        _ => None,
    }
}

/// The project's Inbox items with the human, each listed once where the human answers it: a
/// question its parent sent on is the parent's, with the sub-projects' questions it sent on.
/// One's prompt, or how many, and a former parent that escalated its own.
fn questions(index: &WorkspaceIndex, project: &Project) -> Option<String> {
    use crate::inbox::escalated_via;
    let sent_on = |item| (project.parent_id.as_deref()).is_some_and(|p| escalated_via(item, p));
    let items: Vec<&Attention> = (index.snapshot().attention.iter())
        .filter(|item| {
            (item.project_id == project.id && !sent_on(item)) || escalated_via(item, &project.id)
        })
        .collect();
    let asked = match items[..] {
        [] => return None,
        [only] => only.prompt.lines().next().unwrap_or_default().to_owned(),
        _ => format!("{} questions", items.len()),
    };
    let escalated_by = items.iter().find_map(|item| match &item.route {
        QuestionRoute::Escalated { parent_id, .. } if *parent_id != project.id => {
            index.project(parent_id)
        }
        _ => None,
    });
    Some(match escalated_by {
        Some(parent) => format!("{asked} · via {}", parent.name),
        None => asked,
    })
}

/// What is collapsed in one lens, kept for as long as the app runs.
#[derive(Debug, Default)]
pub(crate) struct Folds {
    pub spaces: BTreeSet<SpaceId>,
    /// The Outcome lens's collapsed parents, by project id.
    pub parents: BTreeSet<String>,
    pub buckets: BTreeSet<(SpaceId, Bucket)>,
}

impl Folds {
    /// The Me lens's folds at its start: every section collapsed but the selected project's.
    pub(crate) fn me(index: &WorkspaceIndex, selected_project: &str) -> Self {
        let Some(project) = index.project(selected_project) else {
            return Self::default();
        };
        let open = tree::section_of(index, project);
        let spaces = tree::spaces(index).into_iter().map(|(space, _)| space);
        Self {
            spaces: spaces.filter(|space| *space != open).collect(),
            ..Self::default()
        }
    }
    /// Opens the Space section, and the parent, that hide `project_id`'s row.
    pub(crate) fn reveal(&mut self, index: &WorkspaceIndex, project_id: &str) {
        let Some(project) = index.project(project_id) else {
            return;
        };
        if let Some(parent) = &project.parent_id {
            self.parents.remove(parent);
        }
        self.spaces.remove(&tree::section_of(index, project));
    }
    pub(crate) fn open_bucket(&mut self, space: &SpaceId, bucket: Bucket) {
        self.spaces.remove(space);
        self.buckets.remove(&(space.clone(), bucket));
    }
}

pub(crate) fn toggle<T: Ord>(folded: &mut BTreeSet<T>, item: T) {
    if !folded.remove(&item) {
        folded.insert(item);
    }
}

/// A collapsed bucket's line: its first three projects' names and how many more it holds.
pub(crate) fn folded_line(projects: &[Listed]) -> String {
    let names: Vec<&str> = (projects.iter().take(3))
        .map(|listed| listed.root.name.as_str())
        .collect();
    match projects.len().saturating_sub(3) {
        0 => names.join(", "),
        more => format!("{} +{more}", names.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use workspace_host::Host;

    /// A store whose projects read, top to bottom, as `names`, all in one Space.
    fn listed(names: &[&str]) -> (tempfile::TempDir, Host) {
        let directory = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(directory.path()).unwrap();
        for name in names.iter().rev() {
            host.create_project(name).unwrap();
        }
        (directory, host)
    }

    fn id(snapshot: &Snapshot, name: &str) -> String {
        let project = snapshot.projects.iter().find(|p| p.name == name);
        project.unwrap().id.clone()
    }

    fn file(host: &mut Host, child: &str, parent: &str) {
        let snapshot = host.snapshot().unwrap();
        let (child, parent) = (id(&snapshot, child), id(&snapshot, parent));
        host.set_project_parent(&child, Some(&parent)).unwrap();
    }

    fn index(snapshot: Snapshot) -> WorkspaceIndex {
        WorkspaceIndex::new(snapshot, Rc::new(Archive::default()))
    }

    /// Each section as its name in capitals, then each bucket's label and count, and its rows
    /// with the line beneath each.
    fn drawn(sections: &[LensSection], lens: Lens) -> Vec<String> {
        let mut lines = vec![];
        for section in sections {
            lines.push(section.name.to_uppercase());
            for group in &section.groups {
                let label = group.bucket.label(lens);
                lines.push(format!("[{label}] {}", group.projects.len()));
                for listed in &group.projects {
                    lines.push(match &listed.line {
                        Some(line) => format!("  {} — {line}", listed.root.name),
                        None => format!("  {}", listed.root.name),
                    });
                }
            }
        }
        lines
    }

    fn tags(started: bool, apps: &[&str], layers: &[&str]) -> ProjectTags {
        ProjectTags {
            started,
            apps: apps.iter().map(|&a| a.into()).collect(),
            layers: layers.iter().map(|&l| l.into()).collect(),
        }
    }

    #[test]
    fn the_app_and_layer_lenses_list_a_project_under_every_tag_it_has() {
        let names = [
            "Accessible contrast",
            "Readable diff colours",
            "DNS: website Terraform",
            "Recipes",
            "Dimmed panes",
        ];
        let (_directory, mut host) = listed(&names);
        file(&mut host, "Readable diff colours", "Accessible contrast");
        let mut snapshot = host.snapshot().unwrap();
        snapshot.spaces[0].apps = vec!["nvim".into(), "chrome".into(), "unused".into()];
        snapshot.spaces[0].layers = vec!["frontend".into(), "backend".into()];
        for (name, tags) in [
            (
                "Accessible contrast",
                tags(true, &["chrome"], &["frontend"]),
            ),
            (
                "Readable diff colours",
                tags(true, &["nvim", "chrome"], &["frontend", "backend"]),
            ),
            ("DNS: website Terraform", tags(true, &[], &[])),
            ("Recipes", tags(false, &[], &[])),
        ] {
            snapshot.project_tags.insert(id(&snapshot, name), tags);
        }
        // "Dimmed panes" has no entry, as from a host older than tags.
        let index = index(snapshot);

        let apps = tag_sections(&index, TagKind::App, false);
        assert_eq!(
            drawn(&apps, Lens::App),
            [
                "PERSONAL",
                "[nvim] 1",
                "  Readable diff colours — Accessible contrast",
                "[chrome] 2",
                "  Accessible contrast",
                "  Readable diff colours — Accessible contrast",
                "[no app] 1",
                "  DNS: website Terraform",
                "[not started] 2",
                "  Recipes",
                "  Dimmed panes",
            ]
        );
        assert_eq!(apps[0].entries(), 6);
        assert_eq!(apps[0].projects().len(), 5);
        let layers = tag_sections(&index, TagKind::Layer, false);
        assert_eq!(
            drawn(&layers, Lens::Layer)[..6],
            [
                "PERSONAL",
                "[frontend] 2",
                "  Accessible contrast",
                "  Readable diff colours — Accessible contrast",
                "[backend] 1",
                "  Readable diff colours — Accessible contrast",
            ]
        );
        assert_eq!(drawn(&layers, Lens::Layer)[6], "[no layer] 1");
    }

    #[test]
    fn a_folded_bucket_names_its_first_three_projects() {
        let names = ["a", "b", "c", "d", "e", "f"];
        let (_directory, host) = listed(&names);
        let index = index(host.snapshot().unwrap());
        let sections = tag_sections(&index, TagKind::Layer, false);
        let projects = &sections[0].groups[0].projects;
        assert_eq!(folded_line(projects), "a, b, c +3");
        assert_eq!(folded_line(&projects[..2]), "a, b");
    }

    const NOW: i64 = 100 * DAY;

    /// A Celadon-like store: every project's coordinator last started a turn an hour ago.
    struct Me {
        _directory: tempfile::TempDir,
        snapshot: Snapshot,
    }

    impl Me {
        fn new(names: &[&str]) -> Self {
            let (directory, host) = listed(names);
            let mut snapshot = host.snapshot().unwrap();
            for project in &snapshot.projects {
                let facts = snapshot.me_facts.entry(project.id.clone()).or_default();
                facts.last_turn_started = Some(NOW - DAY / 24);
            }
            Self {
                _directory: directory,
                snapshot,
            }
        }
        fn id(&self, name: &str) -> String {
            id(&self.snapshot, name)
        }
        fn coordinator(&self, name: &str) -> String {
            let project = self.id(name);
            let mut sessions = self.snapshot.sessions.iter();
            sessions
                .find(|s| s.project_id == project)
                .unwrap()
                .id
                .clone()
        }
        /// No session of `name`, archived ones included, has started a turn for `days` days.
        fn quiet_for(&mut self, name: &str, days: i64) {
            self.facts(name).last_turn_started = Some(NOW - days * DAY - 1);
        }
        fn file(&mut self, child: &str, parent: &str) {
            let parent = self.id(parent);
            let child = self.id(child);
            let mut projects = self.snapshot.projects.iter_mut();
            projects.find(|p| p.id == child).unwrap().parent_id = Some(parent);
        }
        fn ask(&mut self, name: &str, prompt: &str, route: QuestionRoute) {
            let project_id = self.id(name);
            self.snapshot.attention.push(Attention {
                id: format!("{name}-{prompt}"),
                session_id: self.coordinator(name),
                project_id,
                host: "local".into(),
                operation_id: format!("question:{prompt}"),
                prompt: prompt.into(),
                options: vec![],
                answer: None,
                repository_request: None,
                route,
            });
        }
        /// `name` has a ticket, so it has started.
        fn started(&mut self, name: &str) {
            let coordinator = self.coordinator(name);
            self.snapshot.tickets_owned.insert(coordinator, 1);
        }
        /// The host's Me facts for `name`.
        fn facts(&mut self, name: &str) -> &mut MeFacts {
            let project = self.id(name);
            self.snapshot.me_facts.entry(project).or_default()
        }
        fn archive(&mut self, name: &str) {
            let coordinator = self.coordinator(name);
            let mut sessions = self.snapshot.sessions.iter_mut();
            sessions.find(|s| s.id == coordinator).unwrap().archived = true;
        }
        fn drawn(&self) -> Vec<String> {
            let index = index(self.snapshot.clone());
            drawn(&me_sections(&index, NOW), Lens::Me)[1..].to_vec()
        }
    }

    #[test]
    fn questions_list_the_prompt_or_how_many_once_where_the_human_answers_them() {
        let mut me = Me::new(&["Accessible contrast", "Contrast warnings", "Recipes"]);
        me.file("Contrast warnings", "Accessible contrast");
        let escalated = QuestionRoute::Escalated {
            parent_id: me.id("Accessible contrast"),
            note: None,
        };
        me.ask("Contrast warnings", "Warn on comments?", escalated.clone());
        me.ask("Contrast warnings", "Which minimum?", escalated);
        me.ask(
            "Recipes",
            "Keep the old format?\nIt is still read.",
            QuestionRoute::Human,
        );
        // The questions its parent sent on are answered in the parent, so only it lists them.
        assert_eq!(
            me.drawn(),
            [
                "[Questions] 2",
                "  Accessible contrast — 2 questions",
                "  Recipes — Keep the old format?",
            ]
        );

        // The child lists a question of its own.
        me.ask(
            "Contrast warnings",
            "Rename the setting?",
            QuestionRoute::Human,
        );
        assert_eq!(
            me.drawn(),
            [
                "[Questions] 3",
                "  Accessible contrast — 2 questions",
                "  Contrast warnings — Rename the setting?",
                "  Recipes — Keep the old format?",
            ]
        );
    }

    #[test]
    fn merge_on_github_names_the_first_pull_request_the_host_left_to_the_human() {
        let mut me = Me::new(&["Redo sponsor page", "Unreviewed"]);
        let merge = |number, base: &str| HumanMerge {
            ticket_id: format!("t{number}"),
            number,
            base: base.into(),
        };
        me.facts("Redo sponsor page").merges = vec![merge(41, "main"), merge(45, "release")];
        assert_eq!(
            me.drawn(),
            [
                "[Merge on GitHub] 1",
                "  Redo sponsor page — PR #41 into main passed review"
            ]
        );
    }

    #[test]
    fn a_project_goes_quiet_after_five_days_without_a_turn_in_it_or_its_children() {
        let names = ["Colour-blind palettes", "Recipes", "Platform", "CI", "Busy"];
        let mut me = Me::new(&names);
        me.started("Colour-blind palettes");
        me.quiet_for("Colour-blind palettes", 6);
        me.quiet_for("Recipes", 7);
        // A standing parent is quiet only when its children are too.
        me.snapshot.projects[2].standing = true;
        me.started("Platform");
        me.file("CI", "Platform");
        me.quiet_for("Platform", 5);
        me.quiet_for("CI", 5);
        me.started("Busy");
        me.quiet_for("Busy", 4);
        assert_eq!(
            me.drawn(),
            [
                "[Going quiet] 4",
                "  Colour-blind palettes — No turn for 6 days",
                "  Recipes — No turn for 7 days, not started",
                "  Platform — No turn for 5 days",
                "  CI — No turn for 5 days, not started",
            ]
        );
        me.quiet_for("CI", 1);
        assert_eq!(
            me.drawn(),
            [
                "[Going quiet] 2",
                "  Colour-blind palettes — No turn for 6 days",
                "  Recipes — No turn for 7 days, not started",
            ]
        );
    }

    #[test]
    fn a_project_the_host_finds_ready_to_close_is_listed_there_and_never_as_quiet() {
        let mut me = Me::new(&["CI runners", "Light themes", "Old"]);
        me.started("CI runners");
        me.facts("CI runners").ready_to_close = true;
        me.quiet_for("CI runners", 9);
        // An archived project is not listed, whatever its facts say.
        me.facts("Old").ready_to_close = true;
        me.archive("Old");
        assert_eq!(
            me.drawn(),
            [
                "[Ready to close] 1",
                "  CI runners — Every ticket accepted or closed, PRs merged",
            ]
        );
    }

    #[test]
    fn the_me_badge_counts_every_row_across_the_spaces() {
        let mut me = Me::new(&["Contrast warnings", "Recipes"]);
        me.ask("Contrast warnings", "Warn?", QuestionRoute::Human);
        me.ask("Recipes", "Keep?", QuestionRoute::Human);
        me.quiet_for("Recipes", 7);
        let celadon = me.snapshot.spaces[0].clone();
        me.snapshot.spaces.push(Space {
            id: SpaceId("vyttle".into()),
            name: "Vyttle".into(),
            ..celadon
        });
        me.snapshot.projects[0].space = SpaceId("vyttle".into());
        let index = index(me.snapshot.clone());
        let sections = me_sections(&index, NOW);
        assert_eq!(
            drawn(&sections, Lens::Me),
            [
                "PERSONAL",
                "[Questions] 1",
                "  Recipes — Keep?",
                "[Going quiet] 1",
                "  Recipes — No turn for 7 days, not started",
                "VYTTLE",
                "[Questions] 1",
                "  Contrast warnings — Warn?",
            ]
        );
        assert_eq!(me_count(&sections), 3);
    }

    #[test]
    fn the_me_lens_starts_with_only_the_selected_projects_space_open() {
        let mut me = Me::new(&["Contrast warnings", "Recipes"]);
        let personal = me.snapshot.spaces[0].clone();
        let vyttle = SpaceId("vyttle".into());
        me.snapshot.spaces.push(Space {
            id: vyttle.clone(),
            name: "Vyttle".into(),
            ..personal.clone()
        });
        me.snapshot.projects[0].space = vyttle.clone();
        let index = index(me.snapshot.clone());

        let folds = Folds::me(&index, &me.id("Recipes"));
        assert_eq!(
            folds.spaces,
            BTreeSet::from([vyttle.clone(), SpaceId::default()])
        );
        let folds = Folds::me(&index, &me.id("Contrast warnings"));
        assert!(folds.spaces.contains(&personal.id) && !folds.spaces.contains(&vyttle));
    }

    #[test]
    fn revealing_a_hidden_child_opens_its_parent_and_space_and_opening_a_bucket_its_section() {
        let mut me = Me::new(&["Accessible contrast", "Contrast warnings"]);
        me.file("Contrast warnings", "Accessible contrast");
        let index = index(me.snapshot.clone());
        let space = me.snapshot.spaces[0].id.clone();
        let parent = me.id("Accessible contrast");
        let mut folds = Folds {
            spaces: BTreeSet::from([space.clone()]),
            parents: BTreeSet::from([parent.clone()]),
            buckets: BTreeSet::new(),
        };
        let hidden = tree::rows(&index, false, &folds.spaces, &folds.parents);
        assert_eq!(hidden.len(), 1, "only the Space's row");

        folds.reveal(&index, &me.id("Contrast warnings"));

        assert!(folds.spaces.is_empty() && folds.parents.is_empty());
        let rows = tree::rows(&index, false, &folds.spaces, &folds.parents);
        assert!(matches!(
            rows.last(),
            Some(tree::Row::Project { root, child: true }) if root.name == "Contrast warnings"
        ));

        let backend = (space.clone(), Bucket::Tag("backend".into()));
        folds.spaces.insert(space.clone());
        folds.buckets.insert(backend.clone());
        folds.open_bucket(&space, Bucket::Tag("backend".into()));
        assert!(folds.spaces.is_empty() && folds.buckets.is_empty());
    }
}
