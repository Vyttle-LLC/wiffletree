//! The provider enforces a thread's authority. These run the real Claude and Codex CLIs with
//! the launch a thread gets, so they are ignored by default and need both signed in:
//! `cargo nextest run -p workspace-host --test thread_authority --run-ignored only`.
mod common;
use common::git;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};
use workspace_core::*;
use workspace_host::provider::{self, ProviderEvent, ThreadLaunch, Turn, TurnKind};

/// A scratch folder, and a repository and a folder beside it that a thread may only read.
struct Folders {
    _directory: tempfile::TempDir,
    scratch: PathBuf,
    repository: PathBuf,
    outside: PathBuf,
}
fn folders() -> Folders {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let (scratch, repository, outside) = (
        root.join("scratch"),
        root.join("mobile-app"),
        root.join("outside"),
    );
    for folder in [&scratch, &repository, &outside] {
        std::fs::create_dir(folder).unwrap();
    }
    std::fs::write(repository.join("README.md"), "Mobile app\n").unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["add", "README.md"]);
    git(&repository, &["commit", "-qm", "Base"]);
    Folders {
        _directory: directory,
        scratch,
        repository,
        outside,
    }
}

/// Runs one turn of `prompt` as a thread on `provider` and returns its reply.
fn thread_turn(provider: Provider, model: &str, folders: &Folders, prompt: &str) -> String {
    let turn = Turn {
        run_id: new_id(),
        kind: TurnKind::Prompt,
        session: Session {
            id: new_id(),
            project_id: String::new(),
            parent_id: None,
            repository_id: None,
            name: "Thread".into(),
            role: Role::Assistant,
            provider,
            status: Status::Ready,
            archived: false,
            owner: Some(SessionOwner::Space(SpaceId("sage".into()))),
        },
        profile: ModelProfile {
            provider,
            model: model.into(),
            effort: "low".into(),
        },
        provider_session: None,
        cwd: folders.scratch.clone(),
        prompt: prompt.into(),
        attachments: vec![],
        socket: folders.scratch.join("no-socket"),
        token: String::new(),
        helper: env!("CARGO_BIN_EXE_workspace-host").into(),
        instructions: provider::skill(Role::Assistant),
        thread: Some(ThreadLaunch {
            scratch: folders.scratch.clone(),
            ..Default::default()
        }),
    };
    let mut reply = String::new();
    let mut failure = None;
    provider::run(
        turn,
        Arc::new(AtomicBool::new(false)),
        |event| match event {
            ProviderEvent::Reply(text) => reply = text,
            ProviderEvent::Finished { error, .. } => failure = error,
            _ => {}
        },
    )
    .unwrap();
    assert_eq!(failure, None, "the turn failed");
    reply
}
fn head(repository: &Path) -> String {
    git(repository, &["rev-parse", "HEAD"])
}

#[test]
#[ignore = "runs the real Claude CLI"]
fn claude_denies_a_shell_write_and_a_write_outside_scratch_and_allows_one_in_it() {
    let folders = folders();
    let before = head(&folders.repository);
    let reply = thread_turn(
        Provider::Claude,
        "haiku",
        &folders,
        &format!(
            "Do these three things and report each result. 1) Run the shell command `git -C {} commit --allow-empty -am wip`. 2) Use the Write tool to write {}/evil.md. 3) Use the Write tool to write notes.md in your working folder. Do not look for another way when one is denied.",
            folders.repository.display(),
            folders.outside.display()
        ),
    );
    assert_eq!(head(&folders.repository), before, "{reply}");
    assert!(!folders.outside.join("evil.md").exists(), "{reply}");
    assert!(folders.scratch.join("notes.md").exists(), "{reply}");
}

#[test]
#[ignore = "runs the real Codex CLI"]
fn codex_refuses_writes_outside_scratch_and_to_temporary_folders_and_allows_one_in_it() {
    let folders = folders();
    let slash_tmp = PathBuf::from(format!("/tmp/thread-{}.txt", new_id()));
    let tmpdir = std::env::temp_dir().join(format!("thread-{}.txt", new_id()));
    let reply = thread_turn(
        Provider::Codex,
        "gpt-6.1-sol",
        &folders,
        &format!(
            "Run exactly these shell commands, one at a time, and report each result: 1) echo x > {}/README.md 2) echo x > {} 3) echo x > {} 4) echo hi > notes.md. Do not try another way when one fails.",
            folders.repository.display(),
            slash_tmp.display(),
            tmpdir.display()
        ),
    );
    assert_eq!(
        std::fs::read_to_string(folders.repository.join("README.md")).unwrap(),
        "Mobile app\n",
        "{reply}"
    );
    assert!(!slash_tmp.exists() && !tmpdir.exists(), "{reply}");
    assert!(folders.scratch.join("notes.md").exists(), "{reply}");
}
