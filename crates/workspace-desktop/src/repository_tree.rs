//! What Choose repositories… holds, with no drawing: the workspace's repositories by folder, the
//! filter over them, and the ticks and followed roots the human has chosen so far. Ticks are
//! `owner/name`, compared ignoring case as GitHub does, so two clones tick together.
use std::collections::BTreeSet;
use std::path::Path;
use workspace_core::{Repository, Space, SpaceId};

pub(super) const NOT_CLONED: &str = "Not cloned on this Mac";

/// A Space's form as the dialog reads and returns it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Choice {
    pub(super) repositories: Vec<String>,
    /// Root folders, as the host stores them.
    pub(super) followed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) name: String,
    /// `None` for a repository not on GitHub, which cannot be in a Space.
    pub(super) identity: Option<String>,
    /// As shown, with `~`; empty for a repository not cloned here.
    pub(super) path: String,
    /// The other Spaces that list it.
    pub(super) others: Vec<SpaceId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Folder {
    /// Its path as the host stores it, or `NOT_CLONED`.
    pub(super) key: String,
    pub(super) label: String,
    /// One of the root folders, which alone can be followed.
    pub(super) root: bool,
    /// It held a ticked repository when the dialog opened, so it came first.
    first: bool,
    pub(super) rows: Vec<Row>,
}
impl Folder {
    pub(super) fn is_not_cloned(&self) -> bool {
        self.key == NOT_CLONED
    }
    /// Where it stands among folders: ticked ones, other roots, Not cloned, then the rest.
    fn rank(&self) -> u8 {
        match (self.first, self.root, self.is_not_cloned()) {
            (_, _, true) => 2,
            (true, _, _) => 0,
            (_, true, _) => 1,
            _ => 3,
        }
    }
}

/// What the filter box says, read once for all the rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    Everything,
    /// Part of a name, `owner/name` or shown path, in lower case.
    Text(String),
    /// `~/dev/vyttle/*`: exactly the rows of that folder, in lower case.
    Folder(String),
}
impl Filter {
    pub(super) fn new(text: &str, home: Option<&str>) -> Self {
        let text = shown_path(text.trim(), home).to_lowercase();
        if text.is_empty() {
            return Self::Everything;
        }
        match text.strip_suffix("/*") {
            Some(folder) => Self::Folder(folder.to_owned()),
            None => Self::Text(text.trim_end_matches('/').to_owned()),
        }
    }
    fn matches(&self, folder: &Folder, row: &Row) -> bool {
        match self {
            Self::Everything => true,
            Self::Folder(path) => folder.label.to_lowercase() == *path,
            Self::Text(text) => {
                let holds = |field: &str| field.to_lowercase().contains(text);
                holds(&row.name) || holds(&row.path) || row.identity.as_deref().is_some_and(holds)
            }
        }
    }
}

/// A folder's tick over the rows shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tick {
    Off,
    Some,
    On,
    /// No row shown can be ticked.
    Unavailable,
}

/// A folder as the filter leaves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ShownFolder<'a> {
    pub(super) folder: &'a Folder,
    pub(super) rows: Vec<&'a Row>,
    pub(super) tick: Tick,
    /// Such as "4 of 25", or "7 shown · 0 of 7" while the filter hides some.
    pub(super) count: String,
    pub(super) open: bool,
    /// Whether the Space follows it, when Follow is offered.
    pub(super) follow: Option<bool>,
}

pub(super) struct Tree {
    folders: Vec<Folder>,
    open: BTreeSet<String>,
    started: Choice,
    choice: Choice,
    home: Option<String>,
}
impl Tree {
    /// The repositories of `space`'s dialog: its form's `choice`, over the workspace.
    pub(super) fn new(
        spaces: &[Space],
        space: &SpaceId,
        repositories: &[Repository],
        roots: &[String],
        choice: Choice,
        home: Option<&str>,
    ) -> Self {
        let ticked = |identity: &str| listed(&choice.repositories, identity);
        let mut folders: Vec<Folder> = vec![];
        for repository in repositories {
            let holder = Path::new(&repository.path).parent();
            let key = holder.map_or(String::new(), |p| p.to_string_lossy().into_owned());
            if !folders.iter().any(|f| f.key == key) {
                folders.push(Folder {
                    label: shown_path(&key, home),
                    root: roots.contains(&key),
                    first: false,
                    rows: vec![],
                    key: key.clone(),
                });
            }
            let identity = repository.github_identity();
            let others = (spaces.iter())
                .filter(|s| s.id != *space && identity.as_ref().is_some_and(|i| s.lists_named(i)))
                .map(|s| s.id.clone())
                .collect();
            let folder = (folders.iter_mut().find(|f| f.key == key)).expect("pushed above");
            folder.first |= identity.as_deref().is_some_and(ticked);
            folder.rows.push(Row {
                name: repository.name.clone(),
                identity,
                path: shown_path(&repository.path, home),
                others,
            });
        }
        let not_cloned: Vec<Row> = (choice.repositories.iter())
            .filter(|identity| !repositories.iter().any(|r| r.is_github(identity)))
            .map(|identity| not_cloned_row(identity))
            .collect();
        if !not_cloned.is_empty() {
            folders.push(not_cloned_folder(not_cloned));
        }
        for folder in &mut folders {
            folder.rows.sort_by_key(|row| row.name.to_lowercase());
        }
        folders.sort_by_cached_key(|f| (f.rank(), f.label.to_lowercase()));
        let open = (folders.iter())
            .filter(|f| f.first || f.is_not_cloned())
            .map(|f| f.key.clone())
            .collect();
        Self {
            folders,
            open,
            started: choice.clone(),
            choice,
            home: home.map(str::to_owned),
        }
    }

    pub(super) fn filter(&self, text: &str) -> Filter {
        Filter::new(text, self.home.as_deref())
    }
    pub(super) fn is_ticked(&self, identity: &str) -> bool {
        listed(&self.choice.repositories, identity)
    }
    /// Ticked since the dialog opened.
    pub(super) fn is_new(&self, identity: &str) -> bool {
        self.is_ticked(identity) && !listed(&self.started.repositories, identity)
    }

    /// The folders with a row the filter shows, in order. A filter opens them all.
    pub(super) fn shown(&self, filter: &Filter) -> Vec<ShownFolder<'_>> {
        (self.folders.iter())
            .filter_map(|folder| {
                let rows: Vec<&Row> = (folder.rows.iter())
                    .filter(|row| filter.matches(folder, row))
                    .collect();
                if rows.is_empty() {
                    return None;
                }
                let all = rows.len() == folder.rows.len();
                let ticked =
                    |row: &&&Row| row.identity.as_deref().is_some_and(|i| self.is_ticked(i));
                let on = rows.iter().filter(ticked).count();
                let tickable = rows.iter().filter(|row| row.identity.is_some()).count();
                let tick = match on {
                    _ if tickable == 0 => Tick::Unavailable,
                    0 => Tick::Off,
                    on if on == tickable => Tick::On,
                    _ => Tick::Some,
                };
                let count = match all {
                    true => format!("{on} of {}", rows.len()),
                    false => format!("{} shown · {on} of {}", rows.len(), rows.len()),
                };
                Some(ShownFolder {
                    open: *filter != Filter::Everything || self.open.contains(&folder.key),
                    follow: (folder.root && all).then(|| self.follows(&folder.key)),
                    folder,
                    rows,
                    tick,
                    count,
                })
            })
            .collect()
    }

    fn follows(&self, root: &str) -> bool {
        self.choice.followed.iter().any(|followed| followed == root)
    }

    pub(super) fn toggle_open(&mut self, key: &str) {
        if !self.open.remove(key) {
            self.open.insert(key.to_owned());
        }
    }

    pub(super) fn set_ticked(&mut self, identity: &str, on: bool) {
        match on {
            true if !self.is_ticked(identity) => self.choice.repositories.push(identity.to_owned()),
            true => {}
            false => (self.choice.repositories).retain(|r| !r.eq_ignore_ascii_case(identity)),
        }
    }

    /// Ticks the rows the filter shows in a folder, or unticks them when all are ticked.
    pub(super) fn toggle_folder(&mut self, key: &str, filter: &Filter) {
        let Some(shown) = self.shown(filter).into_iter().find(|f| f.folder.key == key) else {
            return;
        };
        let on = shown.tick != Tick::On;
        let identities: Vec<String> = (shown.rows.iter())
            .filter_map(|row| row.identity.clone())
            .collect();
        for identity in identities {
            self.set_ticked(&identity, on);
        }
    }

    /// Following ticks every repository in the folder; stopping leaves the ticks.
    pub(super) fn set_following(&mut self, root: &str, on: bool) {
        if !on {
            return self.choice.followed.retain(|followed| followed != root);
        }
        if !self.follows(root) {
            self.choice.followed.push(root.to_owned());
        }
        let identities: Vec<String> = (self.folders.iter())
            .filter(|f| f.key == root)
            .flat_map(|f| &f.rows)
            .filter_map(|row| row.identity.clone())
            .collect();
        for identity in identities {
            self.set_ticked(&identity, true);
        }
    }

    /// Ticks a typed `owner/name`, which has a row in Not cloned on this Mac unless a repository
    /// here already is it.
    pub(super) fn add(&mut self, identity: &str) {
        let known = (self.folders.iter().flat_map(|f| &f.rows))
            .filter_map(|row| row.identity.as_deref())
            .find(|known| known.eq_ignore_ascii_case(identity))
            .map(str::to_owned);
        if known.is_none() {
            let row = not_cloned_row(identity);
            match self.folders.iter_mut().find(|f| f.is_not_cloned()) {
                Some(folder) => folder.rows.push(row),
                None => {
                    let folder = not_cloned_folder(vec![row]);
                    let at = self.folders.partition_point(|f| f.rank() < folder.rank());
                    self.folders.insert(at, folder);
                }
            }
            self.open.insert(NOT_CLONED.to_owned());
        }
        self.set_ticked(known.as_deref().unwrap_or(identity), true);
    }

    /// The footer's line: what is selected and new, what Follow will do, and what a folder's
    /// tick covers while the filter hides some of its rows.
    pub(super) fn footer(&self, filter: &Filter) -> String {
        let selected = self.choice.repositories.len();
        let new = (self.choice.repositories.iter())
            .filter(|r| !listed(&self.started.repositories, r))
            .count();
        let mut line = match new {
            0 => format!("{selected} selected."),
            new => format!("{selected} selected, {new} new."),
        };
        let followed: Vec<String> = (self.choice.followed.iter())
            .filter(|root| !self.started.followed.contains(root))
            .map(|root| shown_path(root, self.home.as_deref()))
            .collect();
        if !followed.is_empty() {
            let folders = followed.join(" and ");
            line.push_str(&format!(
                " New repositories in {folders} join by themselves."
            ));
        }
        let shown = self.shown(filter);
        let partial: Vec<_> = (shown.iter())
            .filter(|f| f.rows.len() < f.folder.rows.len())
            .collect();
        match partial[..] {
            [] => {}
            [only] => line.push_str(&format!(
                " The folder’s tick covers the {} shown.",
                only.rows.len()
            )),
            _ => line.push_str(" Each folder’s tick covers the rows shown."),
        }
        line
    }

    /// The form's new draft over `form`, its repositories now, which a root check may have added
    /// to while the dialog was open.
    pub(super) fn finish(&self, form: &[String]) -> Choice {
        Choice {
            repositories: merged_list(&self.started.repositories, &self.choice.repositories, form),
            followed: self.choice.followed.clone(),
        }
    }
}

fn listed(repositories: &[String], identity: &str) -> bool {
    (repositories.iter()).any(|listed| listed.eq_ignore_ascii_case(identity))
}

/// `latest` with the edits that turned `read` into `edited`, so a repository a rule, a chip or
/// another Mac added meanwhile stays: what either side took out of `read` goes, and what the
/// edit added follows, keeping `latest`'s order and spelling.
pub(super) fn merged_list(read: &[String], edited: &[String], latest: &[String]) -> Vec<String> {
    let kept = latest
        .iter()
        .filter(|r| !listed(read, r) || listed(edited, r));
    let added = edited
        .iter()
        .filter(|r| !listed(read, r) && !listed(latest, r));
    kept.chain(added).cloned().collect()
}

fn not_cloned_row(identity: &str) -> Row {
    Row {
        name: identity.to_owned(),
        identity: Some(identity.to_owned()),
        path: String::new(),
        others: vec![],
    }
}

fn not_cloned_folder(rows: Vec<Row>) -> Folder {
    Folder {
        key: NOT_CLONED.to_owned(),
        label: NOT_CLONED.to_owned(),
        root: false,
        first: false,
        rows,
    }
}

/// `path` with a leading `home` written as `~`.
pub(super) fn shown_path(path: &str, home: Option<&str>) -> String {
    match home.and_then(|home| Path::new(path).strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(_) => format!("~{}", &path[home.map_or(0, str::len)..]),
        None => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_core::*;

    const HOME: Option<&str> = Some("/Users/m");

    fn repository(path: &str, identity: Option<&str>) -> Repository {
        let github = identity.and_then(|i| i.split_once('/'));
        Repository {
            id: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            path: path.into(),
            base: "origin/main".into(),
            github: github.map(|(owner, name)| GitHubRepository {
                owner: owner.into(),
                name: name.into(),
                facts: None,
                problem: None,
            }),
            origin_problem: None,
            checks: None,
            protected_branches: vec![],
        }
    }

    fn space(id: &str, repositories: &[&str]) -> Space {
        Space {
            id: SpaceId(id.into()),
            name: id.into(),
            description: String::new(),
            additions: String::new(),
            behaviors: Default::default(),
            repository_settings: Default::default(),
            brain: None,
            storage: SpaceStorage {
                repository: StorageRepository::Local { id: id.into() },
                folder: id.into(),
                root: String::new(),
            },
            repositories: repositories.iter().map(|r| r.to_string()).collect(),
            models: SpaceModels::default(),
            reviews: Default::default(),
            sources: vec![],
            extra: Extra::new(),
        }
    }

    /// Two roots and a folder that is not one. Vyttle lists wiffletree and example-web, which is
    /// not cloned here; Personal lists pica-api.
    fn tree() -> Tree {
        let repositories = [
            repository("/Users/m/dev/celadon/theme", Some("celadon/theme")),
            repository(
                "/Users/m/dev/vyttle/wiffletree",
                Some("Vyttle-LLC/wiffletree"),
            ),
            repository("/Users/m/dev/vyttle/pica-api", Some("Vyttle-LLC/pica-api")),
            repository("/Users/m/dev/vyttle/pica-ios", Some("Vyttle-LLC/pica-ios")),
            repository(
                "/Users/m/dev/vyttle-llc/pica-web",
                Some("Vyttle-LLC/pica-web"),
            ),
            repository("/Users/m/dev/scratch/sketches", None),
        ];
        let listing = ["Vyttle-LLC/wiffletree", "Vyttle-LLC/example-web"];
        let spaces = [
            space("vyttle", &listing),
            space("personal", &["vyttle-llc/PICA-API"]),
        ];
        let roots = [
            "/Users/m/dev/celadon".to_owned(),
            "/Users/m/dev/vyttle".to_owned(),
            "/Users/m/dev/vyttle-llc".to_owned(),
        ];
        let choice = Choice {
            repositories: listing.map(str::to_owned).to_vec(),
            followed: vec![],
        };
        let id = SpaceId("vyttle".into());
        Tree::new(&spaces, &id, &repositories, &roots, choice, HOME)
    }

    fn shown<'a>(tree: &'a Tree, filter: &Filter, label: &str) -> ShownFolder<'a> {
        let shown = tree.shown(filter).into_iter();
        (shown.into_iter().find(|f| f.folder.label == label)).expect(label)
    }
    fn names(folder: &ShownFolder) -> Vec<String> {
        folder.rows.iter().map(|r| r.name.clone()).collect()
    }

    #[test]
    fn folders_with_a_tick_come_first_and_open_and_the_rest_start_closed() {
        let tree = tree();
        let shown = tree.shown(&Filter::Everything);
        let order: Vec<(&str, bool)> = (shown.iter())
            .map(|f| (f.folder.label.as_str(), f.open))
            .collect();
        assert_eq!(
            order,
            [
                ("~/dev/vyttle", true),
                ("~/dev/celadon", false),
                ("~/dev/vyttle-llc", false),
                (NOT_CLONED, true),
                ("~/dev/scratch", false),
            ]
        );
        assert_eq!(shown[0].count, "1 of 3");
        assert_eq!(shown[0].tick, Tick::Some);
        assert_eq!(names(&shown[0]), ["pica-api", "pica-ios", "wiffletree"]);
    }

    #[test]
    fn a_row_names_the_other_spaces_listing_it_ignoring_case() {
        let tree = tree();
        let folder = shown(&tree, &Filter::Everything, "~/dev/vyttle");
        assert_eq!(folder.rows[0].others, [SpaceId("personal".into())]);
        assert!(folder.rows[2].others.is_empty(), "not this Space");
    }

    #[test]
    fn the_filter_matches_name_identity_and_path_ignoring_case() {
        let tree = tree();
        for text in [
            "PICA-a",
            "vyttle-llc/pica-a",
            "~/DEV/VYTTLE/pica-a",
            " pica-a ",
        ] {
            let filter = tree.filter(text);
            assert_eq!(names(&shown(&tree, &filter, "~/dev/vyttle")), ["pica-api"]);
        }
        let not_cloned = tree.filter("example-web");
        assert_eq!(tree.shown(&not_cloned).len(), 1);
        assert_eq!(tree.shown(&tree.filter("nothing-called-this")), vec![]);
    }

    #[test]
    fn a_path_filter_reads_with_or_without_its_star_and_from_the_full_path() {
        let tree = tree();
        let folders = |text: &str| {
            let shown = tree.shown(&tree.filter(text));
            shown
                .iter()
                .map(|f| f.folder.label.clone())
                .collect::<Vec<_>>()
        };
        let exact = ["~/dev/vyttle"];
        assert_eq!(folders("~/dev/vyttle/*"), exact);
        assert_eq!(folders("/Users/m/dev/vyttle/*"), exact);
        assert_eq!(
            folders("~/dev/vyttle"),
            ["~/dev/vyttle", "~/dev/vyttle-llc"]
        );
    }

    #[test]
    fn a_matching_folder_opens_and_counts_only_the_rows_shown() {
        let tree = tree();
        let filter = tree.filter("pica");
        let folder = shown(&tree, &filter, "~/dev/vyttle");
        assert!(folder.open);
        assert_eq!(folder.count, "2 shown · 0 of 2");
        assert_eq!(folder.tick, Tick::Off);
    }

    #[test]
    fn a_folders_tick_covers_only_the_rows_shown() {
        let mut tree = tree();
        let filter = tree.filter("pica");
        tree.toggle_folder("/Users/m/dev/vyttle", &filter);
        assert_eq!(shown(&tree, &filter, "~/dev/vyttle").tick, Tick::On);
        let whole = shown(&tree, &Filter::Everything, "~/dev/vyttle");
        assert_eq!(whole.count, "3 of 3", "wiffletree was already ticked");

        tree.toggle_folder("/Users/m/dev/vyttle", &filter);
        let whole = shown(&tree, &Filter::Everything, "~/dev/vyttle");
        assert_eq!(whole.count, "1 of 3");
        assert!(tree.is_ticked("Vyttle-LLC/wiffletree"), "it was not shown");
    }

    #[test]
    fn follow_is_on_root_folders_only_and_only_with_all_their_rows_shown() {
        let tree = tree();
        let follow = |filter: &Filter, label: &str| shown(&tree, filter, label).follow;
        assert_eq!(follow(&Filter::Everything, "~/dev/vyttle"), Some(false));
        assert_eq!(follow(&Filter::Everything, "~/dev/scratch"), None);
        assert_eq!(follow(&Filter::Everything, NOT_CLONED), None);
        assert_eq!(follow(&tree.filter("pica"), "~/dev/vyttle"), None);
        assert_eq!(
            follow(&tree.filter("~/dev/vyttle/*"), "~/dev/vyttle"),
            Some(false)
        );
    }

    #[test]
    fn following_ticks_the_folder_and_a_row_can_be_unticked_afterwards() {
        let mut tree = tree();
        tree.set_following("/Users/m/dev/vyttle", true);
        let folder = shown(&tree, &Filter::Everything, "~/dev/vyttle");
        assert_eq!((folder.follow, folder.tick), (Some(true), Tick::On));
        assert_eq!(
            tree.footer(&Filter::Everything),
            "4 selected, 2 new. New repositories in ~/dev/vyttle join by themselves."
        );

        tree.set_ticked("Vyttle-LLC/pica-ios", false);
        tree.set_following("/Users/m/dev/vyttle-llc", true);
        let finished = tree.finish(&tree.started.repositories);
        assert!(!listed(&finished.repositories, "Vyttle-LLC/pica-ios"));
        assert_eq!(
            finished.followed,
            ["/Users/m/dev/vyttle", "/Users/m/dev/vyttle-llc"]
        );
    }

    #[test]
    fn stopping_following_leaves_the_ticks() {
        let mut tree = tree();
        tree.set_following("/Users/m/dev/vyttle", true);
        tree.set_following("/Users/m/dev/vyttle", false);
        let folder = shown(&tree, &Filter::Everything, "~/dev/vyttle");
        assert_eq!((folder.follow, folder.tick), (Some(false), Tick::On));
    }

    #[test]
    fn a_repository_not_on_github_is_shown_but_cannot_be_ticked() {
        let mut tree = tree();
        tree.toggle_folder("/Users/m/dev/scratch", &Filter::Everything);
        let folder = shown(&tree, &Filter::Everything, "~/dev/scratch");
        assert_eq!(folder.rows[0].identity, None);
        assert_eq!(folder.tick, Tick::Unavailable);
        assert_eq!(folder.count, "0 of 1");
    }

    #[test]
    fn a_row_not_on_github_does_not_keep_its_folders_tick_from_filling() {
        let repositories = [
            repository("/Users/m/dev/a", Some("o/a")),
            repository("/Users/m/dev/b", None),
        ];
        let choice = Choice {
            repositories: vec!["o/a".into()],
            followed: vec![],
        };
        let spaces = [space("s", &["o/a"])];
        let tree = Tree::new(&spaces, &spaces[0].id, &repositories, &[], choice, HOME);
        let folder = shown(&tree, &Filter::Everything, "~/dev");
        assert_eq!((folder.tick, folder.count.as_str()), (Tick::On, "1 of 2"));
    }

    #[test]
    fn two_clones_of_one_identity_tick_together_and_count_once() {
        let repositories = [
            repository("/Users/m/one/site", Some("o/site")),
            repository("/Users/m/two/site", Some("O/SITE")),
        ];
        let spaces = [space("s", &[])];
        let mut tree = Tree::new(
            &spaces,
            &spaces[0].id,
            &repositories,
            &[],
            Choice::default(),
            HOME,
        );
        tree.set_ticked("o/site", true);
        assert_eq!(shown(&tree, &Filter::Everything, "~/one").tick, Tick::On);
        assert_eq!(shown(&tree, &Filter::Everything, "~/two").tick, Tick::On);
        tree.set_following("/Users/m/two", true);
        assert_eq!(
            tree.footer(&Filter::Everything),
            "1 selected, 1 new. New repositories in ~/two join by themselves."
        );
        assert_eq!(tree.finish(&[]).repositories, ["o/site"]);
    }

    #[test]
    fn a_typed_repository_ticks_what_is_here_or_joins_not_cloned() {
        let mut tree = tree();
        tree.add("vyttle-llc/PICA-IOS");
        assert!(tree.is_ticked("Vyttle-LLC/pica-ios"));
        assert_eq!(shown(&tree, &Filter::Everything, NOT_CLONED).rows.len(), 1);

        tree.add("Sage-Social/api");
        let folder = shown(&tree, &Filter::Everything, NOT_CLONED);
        assert_eq!(folder.count, "2 of 2");
        assert!(folder.open);
    }

    #[test]
    fn the_first_typed_repository_starts_not_cloned_in_its_place() {
        let repositories = [
            repository("/Users/m/root/a", Some("o/a")),
            repository("/Users/m/other/b", Some("o/b")),
        ];
        let spaces = [space("s", &[])];
        let roots = ["/Users/m/root".to_owned()];
        let mut tree = Tree::new(
            &spaces,
            &spaces[0].id,
            &repositories,
            &roots,
            Choice::default(),
            HOME,
        );
        tree.add("o/elsewhere");
        let shown = tree.shown(&Filter::Everything);
        let order: Vec<&str> = shown.iter().map(|f| f.folder.label.as_str()).collect();
        assert_eq!(order, ["~/root", NOT_CLONED, "~/other"]);
    }

    #[test]
    fn unticking_a_not_cloned_entry_keeps_its_row_and_takes_it_out() {
        let mut tree = tree();
        tree.set_ticked("Vyttle-LLC/example-web", false);
        assert_eq!(
            shown(&tree, &Filter::Everything, NOT_CLONED).tick,
            Tick::Off
        );
        let form = tree.started.repositories.clone();
        assert_eq!(tree.finish(&form).repositories, ["Vyttle-LLC/wiffletree"]);
    }

    #[test]
    fn the_footer_says_what_a_folders_tick_covers_while_some_rows_are_hidden() {
        let mut tree = tree();
        assert_eq!(tree.footer(&Filter::Everything), "2 selected.");
        let filter = tree.filter("pica-api");
        assert_eq!(
            tree.footer(&filter),
            "2 selected. The folder’s tick covers the 1 shown."
        );
        let filter = tree.filter("pica");
        tree.toggle_folder("/Users/m/dev/vyttle", &filter);
        assert_eq!(
            tree.footer(&filter),
            "4 selected, 2 new. The folder’s tick covers the 2 shown."
        );
    }

    #[test]
    fn finishing_keeps_what_the_form_listed_in_its_order_and_spelling() {
        let spaces = [space("s", &["O/B", "o/a"])];
        let repositories = [
            repository("/Users/m/dev/a", Some("o/a")),
            repository("/Users/m/dev/b", Some("o/b")),
            repository("/Users/m/dev/c", Some("o/c")),
        ];
        let choice = Choice {
            repositories: spaces[0].repositories.clone(),
            followed: vec![],
        };
        let mut tree = Tree::new(&spaces, &spaces[0].id, &repositories, &[], choice, HOME);
        tree.set_ticked("o/c", true);
        tree.set_ticked("o/b", false);
        tree.set_ticked("o/b", true);
        assert_eq!(
            tree.finish(&spaces[0].repositories).repositories,
            ["O/B", "o/a", "o/c"]
        );
    }

    #[test]
    fn a_repository_that_joined_while_the_dialog_was_open_survives_done() {
        let mut tree = tree();
        tree.set_ticked("Vyttle-LLC/pica-ios", true);
        tree.set_ticked("Vyttle-LLC/example-web", false);
        let form = [
            "Vyttle-LLC/wiffletree".to_owned(),
            "Vyttle-LLC/example-web".to_owned(),
            "Vyttle-LLC/joined".to_owned(),
        ];
        assert_eq!(
            tree.finish(&form).repositories,
            [
                "Vyttle-LLC/wiffletree",
                "Vyttle-LLC/joined",
                "Vyttle-LLC/pica-ios"
            ]
        );
    }

    #[test]
    fn merging_keeps_what_arrived_and_honors_what_either_side_took_out() {
        let list = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let read = list(&["o/a", "o/b"]);
        let edited = list(&["o/a", "o/new"]);
        let latest = list(&["o/a", "o/b", "o/joined"]);
        assert_eq!(
            merged_list(&read, &edited, &latest),
            ["o/a", "o/joined", "o/new"]
        );
        let taken_elsewhere = list(&["o/b"]);
        assert_eq!(merged_list(&read, &read, &taken_elsewhere), ["o/b"]);
        let re_cased = list(&["O/A", "o/b"]);
        assert_eq!(merged_list(&read, &read, &re_cased), ["O/A", "o/b"]);
    }

    #[test]
    fn a_path_shows_with_a_tilde_only_under_home() {
        assert_eq!(shown_path("/Users/m", HOME), "~");
        assert_eq!(shown_path("/Users/m/dev", HOME), "~/dev");
        assert_eq!(shown_path("/Users/mike/dev", HOME), "/Users/mike/dev");
        assert_eq!(shown_path("/Users/m/dev", None), "/Users/m/dev");
    }
}
