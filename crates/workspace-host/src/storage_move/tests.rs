//! Moves against local bare repositories standing in for GitHub. No test reaches GitHub.
use super::*;
use crate::storage::tests::{host, join_sage, sage, shared_storage, space_named, sync};
use crate::sync::tests::{GitHub, NAME};
use crate::test_support::git;
use std::sync::Arc;
use workspace_core::job::{Schedule, job_id};

const OTHER: &str = "MHohlios/other-storage";
/// A value the `sensitive-info` check holds.
const PHONE: &str = "Call the parent at (415) 555-0134 tonight.\n";

fn empty_github(folder: &Path) -> Arc<GitHub> {
    Arc::new(GitHub::new(folder, false))
}

/// A second repository, for a move from one GitHub repository to another.
fn other_github(folder: &Path) -> Arc<GitHub> {
    let mut github = GitHub::new(folder, false);
    github.repository["full_name"] = json!(OTHER);
    Arc::new(github)
}

/// Tools that reach `other` for `OTHER` and `github` for anything else.
fn both(github: &Arc<GitHub>, other: &Arc<GitHub>) -> Tools {
    let (first, second) = (github.tools(), other.tools());
    let (first_gh, second_gh) = (first.gh.clone(), second.gh.clone());
    let names = |args: &[&str]| args.iter().any(|a| a.contains(OTHER));
    Tools {
        gh: Arc::new(move |at, args| match names(args) {
            true => second_gh(at, args),
            false => first_gh(at, args),
        }),
        git: Arc::new(move |at, args| match names(args) {
            true => (second.git)(at, args),
            false => (first.git)(at, args),
        }),
    }
}

fn personal(host: &Host) -> Space {
    space_named(host, PERSONAL)
}

fn file(host: &mut Host, space: &Space, title: &str, description: &str) -> Issue {
    host.gh_login = Some("mhohlios".into());
    let new = NewIssue {
        title: title.into(),
        description: description.into(),
        ..NewIssue::default()
    };
    host.create_issue(&space.id, new).unwrap()
}

fn job(host: &mut Host, space: &Space, name: &str) -> Recipe {
    let recipe = Recipe {
        id: job_id(name),
        name: name.into(),
        space_id: space.id.clone(),
        prompt: "Brief me on my open PRs.".into(),
        schedule: Schedule {
            cron: "0 8 * * *".into(),
            timezone: "America/New_York".into(),
        },
        model: None,
        sources: vec![],
        output: None,
        sharing: Sharing::OneRunner,
        runner: None,
        enabled: true,
        extra: Default::default(),
    };
    host.save_job(recipe).unwrap().recipe
}

fn listed(host: &Host, space: &SpaceId) -> Vec<(String, bool)> {
    (host.list_issues(Some(space), None).unwrap().into_iter())
        .map(|listing| (listing.id.to_string(), listing.unread))
        .collect()
}

fn refusal(host: &mut Host, space: &SpaceId, target: Option<&str>, tools: &Tools) -> String {
    let error = host.move_storage(space, target, tools).unwrap_err();
    format!("{error:#}")
}

fn backups(home: &Path) -> Vec<String> {
    let found = fs::read_dir(home.join("backups")).into_iter().flatten();
    found
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn staging_folders(home: &Path) -> Vec<String> {
    (fs::read_dir(home).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("shared-staging-"))
        .collect()
}

fn failed_push() -> runtime::GitAnswer {
    runtime::GitAnswer {
        status: crate::publish::tests::exit(128),
        output: String::new(),
        error: "fatal: the remote end hung up unexpectedly".into(),
    }
}

/// How many commits GitHub holds, none for an empty repository.
fn commits(github: &GitHub) -> usize {
    git(&github.bare, &["rev-list", "--all"]).lines().count()
}

/// Reopens the store as a host would after a stop.
fn restart(home: &Path, host: Host) -> Host {
    let scan_path = host.scan_path.clone();
    drop(host);
    let mut host = Host::open(home).unwrap();
    host.scan_path = scan_path;
    host
}

/// Asserts a local Space is whole where it was.
fn is_local(host: &Host, space: &Space) {
    let now = host.space(&space.id).unwrap().space;
    assert_eq!(now.storage, space.storage);
    assert!(
        host.storage_folder(&now.storage.repository)
            .join(".git")
            .exists()
    );
    assert_eq!(host.tracker(&now).unwrap(), host.tracker(space).unwrap());
    let rows: i64 = (host.db)
        .query_row("SELECT count(*) FROM storage_repositories", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0, "no repository was recorded");
}

/// Asserts a Space is whole in `NAME`, with nothing left to rename.
fn is_on_github(home: &Path, host: &Host, id: &SpaceId) {
    let space = host.space(id).unwrap().space;
    assert_eq!(space.storage.repository, storage::github(NAME));
    let clone = host.storage_folder(&space.storage.repository);
    assert!(clone.join(".git").exists());
    assert!(
        clone
            .join(storage::space_file(&space.storage.folder))
            .exists()
    );
    let moves: i64 = (host.db)
        .query_row("SELECT count(*) FROM repo_moves", [], |r| r.get(0))
        .unwrap();
    assert_eq!(moves, 0);
    assert!(staging_folders(home).is_empty());
}

#[test]
fn a_local_space_moves_to_an_empty_private_repository_and_everything_carries_on() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let space = personal(&host);
    let tools = github.tools();
    let (one, two) = (
        file(&mut host, &space, "First", "").id,
        file(&mut host, &space, "Second", "").id,
    );
    host.mark_issue_read(&one).unwrap();
    let storage = space.storage.repository.key();
    let one_id = one.to_string();
    host.db
        .execute(
            "INSERT INTO issue_derived VALUES (?1,?2,'in_progress')",
            [&storage, &one_id],
        )
        .unwrap();
    host.db
        .execute(
            "INSERT INTO issue_links VALUES (?1,?2,'project','')",
            [&storage, &one_id],
        )
        .unwrap();
    host.db
        .execute(
            "INSERT INTO issue_notes VALUES (?1,?2,'k',?3)",
            params![
                storage,
                one_id,
                json!({"tracker": storage, "id": one_id, "kind": "assigned", "text": "x"})
                    .to_string()
            ],
        )
        .unwrap();
    let recipe = job(&mut host, &space, "Morning brief");
    let board = jobs::board_folder(&space.storage.folder, &recipe.id);
    let output = format!("{board}/brief/brief.md");
    host.write_storage_files(
        &space,
        &[(output.clone(), Some(&b"# Brief\n"[..]))],
        "Output",
    )
    .unwrap();
    let old = host.storage_folder(&space.storage.repository);
    let commits = git(&old, &["rev-list", "--count", "HEAD"]);
    let before = listed(&host, &space.id);

    let check = host
        .check_storage_move(
            &space.id,
            Some(&storage::stage(&host.home, NAME, &tools, &[]).unwrap()),
        )
        .unwrap();
    assert_eq!(check.spaces, [PERSONAL]);
    assert_eq!(check.issues, 2);
    assert_eq!(check.adopts, None);
    let moved = host.move_storage(&space.id, Some(NAME), &tools).unwrap();

    is_on_github(home.path(), &host, &space.id);
    assert_eq!(moved.space.storage.folder, space.storage.folder);
    assert_eq!(listed(&host, &space.id), before, "IDs and read state");
    assert_eq!(
        host.derived_state(NAME, &one).unwrap(),
        Some(IssueState::InProgress)
    );
    let linked: i64 = (host.db)
        .query_row(
            "SELECT count(*) FROM issue_links WHERE storage=?1",
            [NAME],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked, 1);
    let tracker: String = (host.db)
        .query_row(
            "SELECT json_extract(data,'$.tracker') FROM issue_notes",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tracker, NAME);
    let index: String = (host.db)
        .query_row(
            "SELECT DISTINCT json_extract(data,'$.tracker') FROM issue_index WHERE storage=?1",
            [NAME],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(index, NAME);
    for table in ISSUE_TABLES.iter().chain(&["trackers"]) {
        let left: i64 = (host.db)
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE storage=?1"),
                [&storage],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "{table}");
    }

    let status = host.storage_status(&space.id).unwrap();
    assert_eq!(status.issues, 2);
    assert!(status.readers.is_some());
    assert!(
        status.path.ends_with(&format!("github.com/{NAME}")),
        "{}",
        status.path
    );

    let shown = host.recipe(&recipe.key()).unwrap();
    let runner = shown.runner.as_ref().unwrap();
    assert_eq!(runner.machine, sync::machine_name());
    let pushed = github
        .show(&jobs::recipe_file(&space.storage.folder, &recipe.id))
        .unwrap();
    assert!(pushed.contains("runner"), "{pushed}");
    assert_eq!(github.show(&output).as_deref(), Some("# Brief\n"));
    assert!(github.show(&format!("issues/{two}/issue.md")).is_some());
    let log = github.log("%s");
    assert_eq!(log.len(), 4, "a first commit and one per rule set: {log:?}");

    let backup = backups(home.path());
    assert_eq!(backup.len(), 1);
    assert!(!old.exists());
    let kept = home.path().join("backups").join(&backup[0]);
    assert_eq!(git(&kept, &["rev-list", "--count", "HEAD"]), commits);

    // The first pass finds nothing to do, and a new issue takes the next number.
    let head = github.log("%H")[0].clone();
    sync(&mut host, &github);
    assert_eq!(github.log("%H")[0], head);
    let notes = host.workspace_notes(&[]).unwrap();
    assert!(
        matches!(notes.as_slice(), [WorkspaceNote::Issue(_)]),
        "only the news: {notes:?}"
    );
    assert_eq!(
        host.storage_cadences().unwrap(),
        [(repos::key(NAME), storage::TRACKER_CADENCE_MS)]
    );
    let new = file(&mut host, &space, "Third", "");
    assert!(new.id.is_temporary());
    sync(&mut host, &github);
    assert_eq!(listed(&host, &space.id).len(), 3);
    assert!(
        listed(&host, &space.id)
            .iter()
            .all(|(id, _)| !id.contains("new")),
        "numbered"
    );
}

#[test]
fn a_shared_space_moves_to_this_mac_and_github_stays_as_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    file(&mut host, &sage, "First", "");
    sync(&mut host, &github);
    let clone = host.storage_folder(&sage.storage.repository);
    let tools = github.tools();
    let head = github.log("%H");
    let before = listed(&host, &sage.id);

    let check = host.check_storage_move(&sage.id, None).unwrap();
    assert_eq!(check.leaves.as_deref(), Some(NAME));
    let moved = host.move_storage(&sage.id, None, &tools).unwrap();

    assert_eq!(github.log("%H"), head, "GitHub is unchanged");
    assert!(clone.join(".git").exists(), "the clone stays");
    assert!(
        host.storage_pass(&repos::key(NAME)).unwrap().is_none(),
        "and stops syncing"
    );
    assert!(host.storage_cadences().unwrap().is_empty());
    let folder = host.storage_folder(&moved.space.storage.repository);
    assert!(matches!(
        moved.space.storage.repository,
        StorageRepository::Local { .. }
    ));
    assert_eq!(git(&folder, &["remote"]), "");
    assert_eq!(git(&folder, &["status", "--porcelain"]), "");
    assert_eq!(listed(&host, &sage.id), before);
    assert_eq!(host.tracker(&moved.space).unwrap().prefix, "SAGE");
    assert!(backups(home.path()).is_empty());
    let rows: i64 = (host.db)
        .query_row("SELECT count(*) FROM storage_repositories", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
    let record: i64 = (host.db)
        .query_row(
            "SELECT count(*) FROM sync_record WHERE repository=?1",
            [repos::key(NAME)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(record, 0);

    // A later local issue takes the next number, and stays on this Mac.
    let next = file(&mut host, &moved.space, "Second", "");
    assert_eq!(next.id.to_string(), "SAGE-2");
    assert_eq!(github.log("%H"), head);
    let again = restart(home.path(), host);
    assert_eq!(listed(&again, &sage.id).len(), 2);
}

#[test]
fn a_shared_space_moves_to_another_github_repository_and_the_old_one_stays() {
    let (old_folder, new_folder) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let github = shared_storage(old_folder.path(), Some("SAGE"));
    let other = other_github(new_folder.path());
    let tools = both(&github, &other);
    let (home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    file(&mut host, &sage, "First", "");
    sync(&mut host, &github);
    let head = github.log("%H");
    let before = listed(&host, &sage.id);

    let moved = host.move_storage(&sage.id, Some(OTHER), &tools).unwrap();

    assert_eq!(moved.space.storage.repository, storage::github(OTHER));
    assert_eq!(github.log("%H"), head, "the old repository is unchanged");
    assert!(other.show("issues/SAGE-1/issue.md").is_some());
    assert!(other.show("spaces/sage/space.yaml").is_some());
    assert_eq!(listed(&host, &sage.id), before);
    assert!(host.storage_pass(&repos::key(NAME)).unwrap().is_none());
    assert!(host.storage_pass(&repos::key(OTHER)).unwrap().is_some());
    assert!(backups(home.path()).is_empty());
    assert!(repos::clone_folder(home.path(), NAME).join(".git").exists());
    let pass = host.storage_pass(&repos::key(OTHER)).unwrap().unwrap();
    let head = other.log("%H");
    host.finish_storage_pass(&repos::key(OTHER), storage::pass(pass, &tools))
        .unwrap();
    assert_eq!(other.log("%H"), head, "the first pass has nothing to push");
}

#[test]
fn two_spaces_in_one_repository_move_together() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let watch = sage("")
        .replace("sage-id", "watch-id")
        .replace("name: Sage", "name: Sage Watch");
    github.commit("spaces/sage-watch/space.yaml", watch);
    let (_home, mut host) = host();
    let staged = storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
    let ticked = ["sage", "sage-watch"].map(|folder| JoinSpace {
        folder: folder.into(),
        brain_path: None,
        clones: vec![],
    });
    host.join_spaces(staged, &ticked).unwrap();
    let sage = space_named(&host, "Sage");
    let check = host.check_storage_move(&sage.id, None).unwrap();
    assert_eq!(check.spaces, ["Sage", "Sage Watch"]);
    let moved = host.move_storage(&sage.id, None, &github.tools()).unwrap();
    let other = space_named(&host, "Sage Watch");
    assert_eq!(other.storage.repository, moved.space.storage.repository);
    assert_eq!(other.storage.folder, "sage-watch");
    let folder = host.storage_folder(&other.storage.repository);
    assert!(folder.join("spaces/sage-watch/space.yaml").exists());
    assert!(folder.join("spaces/sage/space.yaml").exists());
}

#[test]
fn a_public_repository_is_refused_and_nothing_is_written() {
    let folder = tempfile::tempdir().unwrap();
    let mut public = GitHub::new(folder.path(), false);
    public.repository["private"] = json!(false);
    let github = Arc::new(public);
    let (_home, mut host) = host();
    let space = personal(&host);
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(
        said.contains("Choose a private repository. Its Spaces and issues"),
        "{said}"
    );
    is_local(&host, &space);
}

#[test]
fn a_brain_is_refused_naming_its_space() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (_home, mut host) = host();
    let mut space = personal(&host);
    let (owner, name) = NAME.split_once('/').unwrap();
    space.brain = Some(SpaceBrain {
        repository: BrainRepository::GitHub {
            owner: owner.into(),
            name: name.into(),
            extra: Extra::new(),
        },
        folder: SpaceBrain::DEFAULT_FOLDER.into(),
        extra: Extra::new(),
    });
    host.update_space(space.clone()).unwrap();
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert_eq!(
        said,
        format!(
            "{NAME} is Personal’s brain. A repository is a brain or storage, not both, because a brain changes only through reviewed pull requests."
        )
    );
    is_local(&host, &space);
    assert_eq!(commits(&github), 0);
}

#[test]
fn a_prefix_another_storage_repository_uses_is_refused_not_counting_the_one_moved() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), None);
    let commits = {
        github.commit(TRACKER, "format: 1\nprefix: WOOD\n");
        github.log("%H")
    };
    let (_home, mut host) = host();
    host.create_space("Woodworking", "", "", Some("WOOD"))
        .unwrap();
    let space = personal(&host);
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(
        said.contains("Woodworking") && said.contains("WOOD"),
        "{said}"
    );
    is_local(&host, &space);
    assert_eq!(github.log("%H"), commits);
    // Woodworking itself may go, as its own prefix is not another's.
    let wood = space_named(&host, "Woodworking");
    host.move_storage(&wood.id, Some(NAME), &github.tools())
        .unwrap();
}

#[test]
fn a_target_with_a_tracker_is_refused_when_the_storage_has_issues() {
    let folder = tempfile::tempdir().unwrap();
    let github = Arc::new(GitHub::new(folder.path(), true));
    github.commit(TRACKER, "format: 1\nprefix: SAGE\n");
    let commits = github.log("%H");
    let (_home, mut host) = host();
    let space = personal(&host);
    file(&mut host, &space, "First", "");
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(said.contains("is already a tracker"), "{said}");
    is_local(&host, &space);
    assert_eq!(github.log("%H"), commits);
}

#[test]
fn storage_with_no_issues_takes_on_the_targets_tracker_and_prefix() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (_home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    file(&mut host, &sage, "Theirs", "");
    sync(&mut host, &github);
    let space = personal(&host);
    let old = space.storage.repository.key();
    let tracker = github.show(TRACKER);

    let staged = storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
    let check = host.check_storage_move(&space.id, Some(&staged)).unwrap();
    assert_eq!(check.adopts.as_deref(), Some("SAGE"));
    drop(staged);
    let moved = host
        .move_storage(&space.id, Some(NAME), &github.tools())
        .unwrap();

    assert_eq!(host.tracker(&moved.space).unwrap().prefix, "SAGE");
    let row: i64 = (host.db)
        .query_row(
            "SELECT count(*) FROM trackers WHERE storage=?1",
            [old],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(row, 0);
    assert_eq!(
        github.show(TRACKER),
        tracker,
        "its tracker is left as it is"
    );
    assert!(github.show("spaces/personal/space.yaml").is_some());
    assert_eq!(listed(&host, &space.id).len(), 1, "it shares Sage's issues");
    // The clone Sage already used took the files, and its next pass is clean.
    sync(&mut host, &github);
    let notes = host.workspace_notes(&[]).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_target_holding_a_space_in_the_same_folder_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), None);
    github.commit("spaces/personal/space.yaml", sage("Someone else's"));
    let (_home, mut host) = host();
    let space = personal(&host);
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(
        said.contains("already holds a Space in spaces/personal/"),
        "{said}"
    );
    is_local(&host, &space);
}

#[test]
fn storage_that_is_this_macs_settings_repository_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (_home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    let mut settings = host.settings();
    settings.settings_repository = Some(SettingsRepository {
        name: NAME.into(),
        push_url: "https://github.com/MHohlios/wiffletree-storage.git".into(),
        branch: "main".into(),
        readers: Readers::Invited,
    });
    host.save_settings(&settings).unwrap();
    let said = refusal(&mut host, &sage.id, None, &github.tools());
    assert!(
        said.contains("is also this Mac’s settings repository"),
        "{said}"
    );
    assert_eq!(space_named(&host, "Sage").storage, sage.storage);
}

#[test]
fn a_job_run_in_progress_refuses_the_move() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (_home, mut host) = host();
    let space = personal(&host);
    let recipe = job(&mut host, &space, "Morning brief");
    host.db
        .execute(
            "INSERT INTO job_runs VALUES ('run',?1,1,?2)",
            params![
                recipe.key().as_str(),
                json!({
                    "id": "run", "job": recipe.key(), "started_at": 1, "outcome": "running"
                })
                .to_string()
            ],
        )
        .unwrap();
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(said.contains("job Morning brief is running"), "{said}");
    is_local(&host, &space);
}

#[test]
fn new_issues_with_no_number_refuse_the_move_until_they_sync() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (_home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    file(&mut host, &sage, "First", "");
    let said = refusal(&mut host, &sage.id, None, &github.tools());
    assert!(said.contains("no number yet"), "{said}");
    sync(&mut host, &github);
    host.move_storage(&sage.id, None, &github.tools()).unwrap();
}

#[test]
fn moving_to_where_it_already_is_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (_home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    let said = refusal(&mut host, &sage.id, Some(NAME), &github.tools());
    assert!(said.contains("is already stored in"), "{said}");
    let space = personal(&host);
    let said = refusal(&mut host, &space.id, None, &github.tools());
    assert!(said.contains("already stored on this Mac"), "{said}");
}

#[test]
fn a_held_scan_moves_nothing_says_why_with_no_match_and_leaves_an_empty_repository_empty() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let space = personal(&host);
    let issue = file(&mut host, &space, "First", PHONE);
    let before = listed(&host, &space.id);
    let said = refusal(&mut host, &space.id, Some(NAME), &github.tools());
    assert!(
        said.contains(&format!("issues/{}/issue.md", issue.id)),
        "{said}"
    );
    assert!(said.contains("sensitive-info"), "{said}");
    assert!(!said.contains("555-0134"), "{said}");
    assert_eq!(commits(&github), 0, "nothing reached GitHub");
    is_local(&host, &space);
    assert_eq!(listed(&host, &space.id), before);
    assert!(staging_folders(home.path()).is_empty());
    assert!(
        host.storage_pass("anything").is_ok(),
        "the gate is open again"
    );
    assert!(!host.storage_moving);
}

#[test]
fn a_failed_push_leaves_the_space_where_it_was_and_it_can_move_again() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let space = personal(&host);
    file(&mut host, &space, "First", "");
    let tools = github.tools();
    let git_tool = tools.git.clone();
    let refusing = Tools {
        gh: tools.gh.clone(),
        git: Arc::new(move |at, args| match publish::subcommand(args) {
            "push" => Ok(failed_push()),
            _ => git_tool(at, args),
        }),
    };
    let said = refusal(&mut host, &space.id, Some(NAME), &refusing);
    assert!(said.contains("GitHub did not take the push"), "{said}");
    is_local(&host, &space);
    assert_eq!(commits(&github), 0);
    assert!(backups(home.path()).is_empty());
    host.move_storage(&space.id, Some(NAME), &github.tools())
        .unwrap();
    is_on_github(home.path(), &host, &space.id);
}

#[test]
fn a_push_that_landed_but_lost_its_answer_still_moves() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let space = personal(&host);
    let tools = github.tools();
    let git_tool = tools.git.clone();
    let losing = Tools {
        gh: tools.gh.clone(),
        git: Arc::new(move |at, args| {
            let answer = git_tool(at, args)?;
            Ok(match publish::subcommand(args) {
                "push" => failed_push(),
                _ => answer,
            })
        }),
    };
    host.move_storage(&space.id, Some(NAME), &losing).unwrap();
    is_on_github(home.path(), &host, &space.id);
}

/// A Space with an issue, and a move of it to `NAME` as far as `begin`.
fn begun(host: &mut Host, github: &Arc<GitHub>) -> (Space, Move) {
    let space = personal(host);
    file(host, &space, "First", "");
    let staged = storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
    let run = host.begin_storage_move(&space.id, Some(staged)).unwrap();
    (space, run)
}

#[test]
fn no_pass_runs_during_a_move() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (_home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    let run = host.begin_storage_move(&sage.id, None).unwrap();
    assert!(host.storage_pass(&repos::key(NAME)).unwrap().is_none());
    assert!(
        host.begin_storage_move(&sage.id, None).is_err(),
        "one at a time"
    );
    drop(run);
    host.end_storage_move();
    assert!(host.storage_pass(&repos::key(NAME)).unwrap().is_some());
}

#[test]
fn a_stop_before_the_push_leaves_the_space_where_it_was() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    copy(&run, &github.tools()).unwrap();
    host.note_push(&run).unwrap();
    std::mem::forget(run);
    let mut host = restart(home.path(), host);
    is_local(&host, &space);
    assert!(staging_folders(home.path()).is_empty());
    assert_eq!(commits(&github), 0);
    assert!(!host.storage_moving);
    host.move_storage(&space.id, Some(NAME), &github.tools())
        .unwrap();
    is_on_github(home.path(), &host, &space.id);
}

#[test]
fn a_stop_after_the_push_leaves_the_space_where_it_was_and_the_retry_finishes_it() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    std::mem::forget(run);
    let mut host = restart(home.path(), host);
    is_local(&host, &space);
    let pushed = github.log("%H");
    assert!(!pushed.is_empty());
    file(&mut host, &space, "Second", "");
    let moved = host.move_storage(&space.id, Some(NAME), &tools).unwrap();
    is_on_github(home.path(), &host, &space.id);
    assert_eq!(listed(&host, &moved.space.id).len(), 2);
    assert_eq!(
        github.log("%H").len(),
        pushed.len() + 1,
        "only what changed"
    );
    let left: i64 = (host.db)
        .query_row("SELECT count(*) FROM storage_moves", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_retry_after_a_stopped_adoption_leaves_the_targets_tracker_and_issues_alone() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let issue = "---\nid: SAGE-1\ntitle: Theirs\n---\n";
    github.commit("issues/SAGE-1/issue.md", issue);
    let (home, mut host) = host();
    let space = personal(&host);
    let tools = github.tools();
    let tracker = github.show(TRACKER);
    let staged = storage::stage(&host.home, NAME, &tools, &[]).unwrap();
    let run = host.begin_storage_move(&space.id, Some(staged)).unwrap();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    std::mem::forget(run);
    let mut host = restart(home.path(), host);

    let pushed = github.log("%H");
    let moved = host.move_storage(&space.id, Some(NAME), &tools).unwrap();

    assert_eq!(github.log("%H"), pushed, "nothing more to push");
    assert_eq!(github.show(TRACKER), tracker);
    assert_eq!(
        github.show("issues/SAGE-1/issue.md").as_deref(),
        Some(issue)
    );
    assert_eq!(host.tracker(&moved.space).unwrap().prefix, "SAGE");
    is_on_github(home.path(), &host, &space.id);
}

#[test]
fn a_stopped_adoption_still_refuses_storage_that_has_since_gained_issues() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (home, mut host) = host();
    let space = personal(&host);
    let tools = github.tools();
    let staged = storage::stage(&host.home, NAME, &tools, &[]).unwrap();
    let run = host.begin_storage_move(&space.id, Some(staged)).unwrap();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    std::mem::forget(run);
    let mut host = restart(home.path(), host);
    file(&mut host, &space, "Filed since", "");
    let pushed = github.log("%H");
    let said = refusal(&mut host, &space.id, Some(NAME), &tools);
    assert!(said.contains("is already a tracker"), "{said}");
    assert_eq!(github.log("%H"), pushed);
    is_local(&host, &space);
}

#[test]
fn a_teammates_change_to_what_a_stopped_move_pushed_puts_the_refusal_back() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    std::mem::forget(run);
    let mut host = restart(home.path(), host);
    let file = storage::space_file(&space.storage.folder);
    let edited = github.show(&file).unwrap() + "extra: a teammate's edit\n";
    github.commit(&file, edited);
    let said = refusal(&mut host, &space.id, Some(NAME), &tools);
    assert!(said.contains("already"), "{said}");
    is_local(&host, &space);
}

#[test]
fn a_stop_after_the_switch_is_finished_by_the_next_start() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    let old = host.storage_folder(&space.storage.repository);
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    let before = listed(&host, &space.id);
    let switched = host.switch_storage(run).unwrap();
    std::mem::forget(switched);
    let mut host = restart(home.path(), host);
    is_on_github(home.path(), &host, &space.id);
    assert_eq!(listed(&host, &space.id), before);
    assert!(!old.exists());
    assert_eq!(backups(home.path()).len(), 1);
    let head = github.log("%H")[0].clone();
    sync(&mut host, &github);
    assert_eq!(github.log("%H")[0], head);
}

#[test]
fn a_rename_that_fails_after_the_switch_says_the_space_moved_and_the_next_start_finishes_it() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    let switched = host.switch_storage(run).unwrap();
    // A file where the clone's folder must go stops the rename.
    let blocker = home.path().join(repos::FOLDER).join("github.com");
    fs::write(&blocker, "").unwrap();

    let error = host.finish_storage_move(switched).err().unwrap();

    let said = format!("{error:#}");
    assert!(
        said.starts_with(&format!(
            "{PERSONAL} moved to {NAME}. The next start finishes the rest: "
        )),
        "{said}"
    );
    assert!(!host.storage_moving);
    assert_eq!(
        host.space(&space.id).unwrap().space.storage.repository,
        storage::github(NAME)
    );
    fs::remove_file(blocker).unwrap();
    let host = restart(home.path(), host);
    is_on_github(home.path(), &host, &space.id);
}

#[test]
fn a_stop_after_the_switch_to_this_mac_is_finished_by_the_next_start() {
    let folder = tempfile::tempdir().unwrap();
    let github = shared_storage(folder.path(), Some("SAGE"));
    let (home, mut host) = host();
    join_sage(&mut host, &github).unwrap();
    let sage = space_named(&host, "Sage");
    let run = host.begin_storage_move(&sage.id, None).unwrap();
    copy(&run, &github.tools()).unwrap();
    let switched = host.switch_storage(run).unwrap();
    std::mem::forget(switched);
    let host = restart(home.path(), host);
    let moved = space_named(&host, "Sage");
    let folder = host.storage_folder(&moved.storage.repository);
    assert!(folder.join("spaces/sage/space.yaml").exists());
    assert!(staging_folders(home.path()).is_empty());
    assert_eq!(git(&folder, &["status", "--porcelain"]), "");
}

#[test]
fn a_file_changed_during_the_move_ends_it_and_the_retry_pushes_the_difference() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();
    file(&mut host, &space, "Filed during the move", "");
    let error = host.switch_storage(run).err().unwrap();
    assert_eq!(format!("{error:#}"), CHANGED);
    host.end_storage_move();
    is_local(&host, &space);
    assert_eq!(listed(&host, &space.id).len(), 2);
    host.move_storage(&space.id, Some(NAME), &tools).unwrap();
    is_on_github(home.path(), &host, &space.id);
    assert_eq!(listed(&host, &space.id).len(), 2);
}

#[test]
fn a_job_started_during_the_move_refuses_the_switch() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (_home, mut host) = host();
    let (space, run) = begun(&mut host, &github);
    let recipe = job(&mut host, &space, "Morning brief");
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.db
        .execute(
            "INSERT INTO job_runs VALUES ('run',?1,1,?2)",
            params![
                recipe.key().as_str(),
                json!({
                    "id": "run", "job": recipe.key(), "started_at": 1, "outcome": "running"
                })
                .to_string()
            ],
        )
        .unwrap();
    let error = host.switch_storage(run).err().unwrap();
    assert!(format!("{error:#}").contains("is running"), "{error:#}");
    host.end_storage_move();
    is_local(&host, &space);
}

/// A committed repository at `path`, on GitHub as `identity`.
fn clone_of(path: &Path, identity: &str) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    let commit = ["commit", "-q", "--allow-empty", "-m", "Fixture"];
    let named = ["-c", "user.name=F", "-c", "user.email=f@example.invalid"];
    git(path, &[&named[..], &commit[..]].concat());
    let url = format!("https://github.com/{identity}.git");
    git(path, &["remote", "add", "origin", &url]);
}

/// A root folder with one repository, which `space` follows.
fn followed_root(host: &mut Host, space: &SpaceId) -> (tempfile::TempDir, PathBuf) {
    let folder = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(folder.path()).unwrap();
    clone_of(&root.join("site"), "Vyttle-LLC/site");
    let as_text = |path: &Path| path.to_str().unwrap().to_owned();
    let site = as_text(&root.join("site"));
    host.import_repositories(None, &[site], &[as_text(&root)], &[])
        .unwrap();
    host.set_followed_roots(space, vec![as_text(&root)])
        .unwrap();
    (folder, root)
}

#[test]
fn a_space_keeps_following_its_roots_after_it_moves() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let space = personal(&host).id;
    let (_kept, root) = followed_root(&mut host, &space);
    let followed = host.space(&space).unwrap().local.followed_roots;

    host.move_storage(&space, Some(NAME), &github.tools())
        .unwrap();
    let mut host = restart(home.path(), host);

    assert_eq!(host.space(&space).unwrap().local.followed_roots, followed);
    clone_of(&root.join("pica-web"), "Vyttle-LLC/pica-web");
    let refresh = host.refresh_repositories().unwrap();
    assert_eq!(refresh.joined.len(), 1, "{:?}", refresh.not_joined);
    sync(&mut host, &github);
    let file = storage::space_file(&personal(&host).storage.folder);
    let pushed = github.show(&file).unwrap();
    assert!(pushed.contains("Vyttle-LLC/pica-web"), "{pushed}");
    assert!(!pushed.contains("followed_roots"), "{pushed}");
}

#[test]
fn a_root_check_joining_during_a_move_ends_it_and_the_repository_is_kept() {
    let folder = tempfile::tempdir().unwrap();
    let github = empty_github(folder.path());
    let (home, mut host) = host();
    let followed = personal(&host).id;
    let (_kept, root) = followed_root(&mut host, &followed);
    let (space, run) = begun(&mut host, &github);
    let tools = github.tools();
    copy(&run, &tools).unwrap();
    host.note_push(&run).unwrap();
    push(&run, &tools).unwrap();

    clone_of(&root.join("pica-web"), "Vyttle-LLC/pica-web");
    let refresh = host.refresh_repositories().unwrap();
    assert_eq!(refresh.joined.len(), 1, "{:?}", refresh.not_joined);
    let error = host.switch_storage(run).err().unwrap();
    assert_eq!(format!("{error:#}"), CHANGED);
    host.end_storage_move();

    is_local(&host, &space);
    let joined = "Vyttle-LLC/pica-web";
    let file = storage::space_file(&space.storage.folder);
    let source = host.storage_folder(&space.storage.repository);
    let committed = git(&source, &["show", &format!("HEAD:{file}")]);
    assert!(committed.contains(joined), "{committed}");
    assert!(!github.show(&file).unwrap().contains(joined));

    // Only a first find joins, so the move again is what carries it to GitHub.
    host.refresh_repositories().unwrap();
    host.move_storage(&space.id, Some(NAME), &tools).unwrap();
    is_on_github(home.path(), &host, &space.id);
    assert!(personal(&host).lists_named(joined));
    assert!(github.show(&file).unwrap().contains(joined));
}

#[test]
fn only_the_human_moves_storage() {
    let space_id = SpaceId("sage".into());
    let check = Command::CheckStorageMove {
        space_id: space_id.clone(),
        repository: None,
    };
    let moving = Command::MoveStorage {
        space_id,
        repository: Some(NAME.into()),
    };
    assert!(check.is_human_only() && check.is_read());
    assert!(moving.is_human_only() && !moving.is_read());
    assert_eq!(remote::access::row(&check), remote::access::Row::Read);
    assert_eq!(remote::access::row(&moving), remote::access::Row::Change);
    let tools = include_str!("../mcp.rs");
    assert!(!tools.contains("move_storage") && !tools.contains("MoveStorage"));
}
