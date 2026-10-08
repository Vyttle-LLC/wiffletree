//! Worktrees left by tickets finished before close and accept removed them: listed first,
//! removed only when the human selects them, and never at the cost of unsaved work.
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use workspace_core::*;
use workspace_host::Host;

fn git(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

struct Fixture {
    _directory: tempfile::TempDir,
    host: Host,
    repo: PathBuf,
    stray: PathBuf,
    tickets: Vec<(String, Ticket)>,
}
impl Fixture {
    fn ticket(&self, name: &str) -> &Ticket {
        &self.tickets.iter().find(|(n, _)| n == name).unwrap().1
    }
    fn db(&self) -> Connection {
        Connection::open(self.host.home.join("workspace.sqlite3")).unwrap()
    }
    /// What listing must never change: stored rows, registered worktrees, branches and files.
    fn state(&self) -> Value {
        let db = self.db();
        let rows = |sql: &str| -> Vec<String> {
            db.prepare(sql)
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        json!({
            "tickets": rows("SELECT data FROM tickets ORDER BY rowid"),
            "sessions": rows("SELECT data FROM sessions ORDER BY rowid"),
            "pending": rows("SELECT ticket_id FROM pending_worktree_removals"),
            "activity": rows("SELECT kind||detail FROM activity ORDER BY sequence"),
            "worktrees": git(&self.repo, &["worktree", "list", "--porcelain"]),
            "branches": git(&self.repo, &["branch", "--list"]),
            "status": self.tickets.iter().map(|(_, t)| git(Path::new(&t.worktree), &["status", "--porcelain"])).collect::<Vec<_>>(),
        })
    }
}

/// A repository whose `main` is pushed, and one ticket per leftover class, finished the way a
/// release before worktree removal left them.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let origin = directory.path().join("origin.git");
    let repo = directory.path().join("web");
    std::fs::create_dir_all(&repo).unwrap();
    git(
        directory.path(),
        &["init", "-q", "--bare", origin.to_str().unwrap()],
    );
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Base"]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["fetch", "-q", "origin"]);
    let mut host = Host::open(directory.path().join("home")).unwrap();
    let workspaces = directory.path().join("workspaces");
    host.set_workspaces_dir(workspaces.to_str().unwrap())
        .unwrap();
    let project = host.create_project("Flatten").unwrap();
    let main = host.sessions().unwrap().remove(0);
    let repository = host
        .attach_repository(&project.id, repo.to_str().unwrap(), "HEAD")
        .unwrap();
    let names = [
        "clean",
        "changed",
        "unpushed",
        "dirty",
        "locked",
        "detached",
        "in-turn",
        "abandoned",
        "working",
        "pending",
    ];
    let mut tickets = vec![];
    for name in names {
        let ticket = host
            .create_ticket(&main.id, &repository.id, name, "Old work")
            .unwrap();
        tickets.push((name.to_owned(), ticket));
    }
    let worktree =
        |name: &str| PathBuf::from(&tickets.iter().find(|(n, _)| n == name).unwrap().1.worktree);
    git(
        &worktree("unpushed"),
        &["commit", "-q", "--allow-empty", "-m", "Local only"],
    );
    std::fs::write(worktree("dirty").join("notes.md"), "draft").unwrap();
    git(
        &repo,
        &["worktree", "lock", worktree("locked").to_str().unwrap()],
    );
    git(&worktree("detached"), &["checkout", "-q", "--detach"]);
    git(
        &worktree("detached"),
        &["commit", "-q", "--allow-empty", "-m", "Off branch"],
    );
    let in_turn = host
        .assign_ticket(
            &tickets[6].1.id,
            Role::Tester,
            Provider::Codex,
            "Test",
            None,
        )
        .unwrap();
    let abandoned = host
        .assign_ticket(
            &tickets[7].1.id,
            Role::Implementer,
            Provider::Codex,
            "Do",
            None,
        )
        .unwrap();
    host.set_archived(&abandoned.id, true).unwrap();
    host.assign_ticket(
        &tickets[8].1.id,
        Role::Implementer,
        Provider::Codex,
        "Do",
        None,
    )
    .unwrap();
    // A Wiffletree-made worktree that no ticket records.
    let stray = workspaces.join("tasks/flatten/web/stray");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wiffletree/stray",
            stray.to_str().unwrap(),
        ],
    );

    let db = Connection::open(host.home.join("workspace.sqlite3")).unwrap();
    let finished = [
        ("clean", "closed"),
        ("changed", "closed"),
        ("unpushed", "accepted"),
        ("dirty", "closed"),
        ("locked", "closed"),
        ("detached", "closed"),
        ("in-turn", "accepted"),
        ("pending", "accepted"),
    ];
    for (name, state) in finished {
        let id = &tickets.iter().find(|(n, _)| n == name).unwrap().1.id;
        db.execute(
            "UPDATE tickets SET data=json_set(data,'$.state',?2) WHERE id=?1",
            params![id, state],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO provider_runs(id,session_id,messages,started_at,detail) VALUES ('open-run',?1,'[]',1,'{}')",
        [&in_turn.id],
    )
    .unwrap();
    db.execute(
        "INSERT INTO pending_worktree_removals VALUES (?1,1)",
        [&tickets[9].1.id],
    )
    .unwrap();
    let tickets = tickets
        .into_iter()
        .map(|(name, t)| {
            let current = host.ticket(&t.id).unwrap();
            (name, current)
        })
        .collect();
    Fixture {
        _directory: directory,
        host,
        repo,
        stray,
        tickets,
    }
}

fn classes(listing: &[LeftoverWorktree], fixture: &Fixture) -> Vec<(String, LeftoverClass)> {
    listing
        .iter()
        .map(|l| {
            let name = match &l.ticket_id {
                Some(id) => fixture
                    .tickets
                    .iter()
                    .find(|(_, t)| &t.id == id)
                    .unwrap()
                    .0
                    .clone(),
                None => "stray".into(),
            };
            (name, l.class)
        })
        .collect()
}

#[test]
fn the_listing_classifies_each_leftover_and_changes_nothing() {
    let f = fixture();
    let before = f.state();

    let listing = f.host.leftover_worktrees().unwrap();

    assert_eq!(
        f.state(),
        before,
        "listing changes no row, file, worktree or branch"
    );
    let mut found = classes(&listing, &f);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected = vec![
        ("abandoned".to_owned(), LeftoverClass::Clean),
        ("changed".into(), LeftoverClass::Clean),
        ("clean".into(), LeftoverClass::Clean),
        ("detached".into(), LeftoverClass::Detached),
        ("dirty".into(), LeftoverClass::Dirty),
        ("in-turn".into(), LeftoverClass::InTurn),
        ("locked".into(), LeftoverClass::Locked),
        ("stray".into(), LeftoverClass::UntrackedByWiffletree),
        ("unpushed".into(), LeftoverClass::Unpushed),
    ];
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        found, expected,
        "open work and pending removals are not leftovers"
    );
    let clean = listing
        .iter()
        .find(|l| l.ticket_id.as_ref() == Some(&f.ticket("clean").id))
        .unwrap();
    assert_eq!(clean.project, "Flatten");
    assert_eq!(clean.repository, "web");
    assert_eq!(clean.title, "clean");
    assert_eq!(clean.state, "closed");
    assert_eq!(clean.branch, f.ticket("clean").branch);
    assert_eq!(clean.worktree, f.ticket("clean").worktree);
    assert_eq!(clean.head, git(&f.repo, &["rev-parse", "main"]).trim());
    let stray = listing.iter().find(|l| l.ticket_id.is_none()).unwrap();
    assert_eq!(stray.branch, "wiffletree/stray");
    assert_eq!(
        std::fs::canonicalize(&stray.worktree).unwrap(),
        std::fs::canonicalize(&f.stray).unwrap()
    );
    for leftover in &listing {
        assert_eq!(
            leftover.class.is_removable(),
            matches!(
                leftover.class,
                LeftoverClass::Clean | LeftoverClass::Unpushed
            )
        );
    }
}

#[test]
fn removal_takes_only_selected_worktrees_that_are_still_clean_or_unpushed() {
    let mut f = fixture();
    assert!(f.host.remove_leftover_worktrees(&[]).unwrap().is_empty());
    let listed = f.host.leftover_worktrees().unwrap();
    assert_eq!(listed.len(), 9);
    // A selected worktree gains an uncommitted change after the listing.
    std::fs::write(
        Path::new(&f.ticket("changed").worktree).join("late.txt"),
        "new",
    )
    .unwrap();
    let selected: Vec<String> = ["clean", "changed", "unpushed", "dirty", "in-turn"]
        .iter()
        .map(|name| f.ticket(name).id.clone())
        .collect();

    let outcomes = f.host.remove_leftover_worktrees(&selected).unwrap();

    let outcome = |name: &str| {
        outcomes
            .iter()
            .find(|o| o.ticket_id == f.ticket(name).id)
            .unwrap()
            .clone()
    };
    for name in ["clean", "unpushed"] {
        assert!(outcome(name).removed, "{name}");
        assert!(!Path::new(&f.ticket(name).worktree).exists());
    }
    for (name, reason) in [
        ("changed", "Uncommitted"),
        ("dirty", "Uncommitted"),
        ("in-turn", "turn"),
    ] {
        let kept = outcome(name);
        assert!(!kept.removed, "{name}");
        assert!(kept.reason.as_deref().unwrap().contains(reason), "{kept:?}");
        assert!(Path::new(&f.ticket(name).worktree).exists());
    }
    let branches = git(&f.repo, &["branch", "--list"]);
    for (_, ticket) in &f.tickets {
        assert!(branches.contains(&ticket.branch), "{} kept", ticket.branch);
    }
    assert!(
        git(&f.repo, &["log", "--oneline", &f.ticket("unpushed").branch]).contains("Local only"),
        "the unpushed commit survives on its branch"
    );
    assert_eq!(f.host.tickets().unwrap().len(), f.tickets.len());
    let activity = f
        .host
        .activity(&f.host.projects().unwrap()[0].id, None, 100)
        .unwrap();
    let count = |kind: &str| activity.iter().filter(|a| a.kind == kind).count();
    assert_eq!(count("worktree_removed"), 2);
    assert_eq!(count("worktree_kept"), 3);
}
