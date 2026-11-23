//! The guard on agents' `git` and `gh`: a folder first on every agent's `PATH` holding `git` and
//! `gh` as links to this binary. Started by either name, it runs the real program for what an
//! agent may do and refuses the rest with one line naming the Wiffletree tool to use instead.
//!
//! `git` runs everything but a push, which only a ticket's implementer may make, and only to its
//! own branch on `origin`. `gh` runs reads only. The host decides which branch, if any, an agent
//! may push and passes it in the environment, so branch protection and owners stay the host's.
//!
//! It is a guardrail, not a sandbox: a full path to the real program, a changed environment or a
//! call to GitHub with the human's token is not stopped.
use std::{
    ffi::OsString,
    fs, io,
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

/// The guard's folder in the data folder.
const FOLDER: &str = "guard";
const PROGRAMS: [&str; 2] = ["git", "gh"];
/// The branch the agent may push; absent, it may push nothing. No name holds `KEY`, `SECRET` or
/// `TOKEN`, which Codex keeps out of its shells.
const PUSH_BRANCH: &str = "WIFFLETREE_PUSH_BRANCH";
const REAL_GIT: &str = "WIFFLETREE_REAL_GIT";
const REAL_GH: &str = "WIFFLETREE_REAL_GH";
/// How many aliases deep a `git` call is followed.
const ALIAS_DEPTH: usize = 10;

/// The guard as the host installed it.
struct Installed {
    folder: PathBuf,
    git: Option<PathBuf>,
    gh: Option<PathBuf>,
}
static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// Links `git` and `gh` to `binary` in the data folder's guard folder, and finds the real ones
/// the host itself runs, on its own `PATH`.
pub fn install(data: &Path, binary: &Path) -> io::Result<()> {
    let folder = data.join(FOLDER);
    link(&folder, binary)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let _ = INSTALLED.set(Installed {
        git: find(&path, "git", &folder),
        gh: find(&path, "gh", &folder),
        folder,
    });
    Ok(())
}

/// Links each of `PROGRAMS` in `folder` to `binary`, replacing an older link in one step.
fn link(folder: &Path, binary: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    for name in PROGRAMS {
        let staged = folder.join(format!(".{name}-{}", std::process::id()));
        let _ = fs::remove_file(&staged);
        symlink(binary, &staged)?;
        fs::rename(&staged, folder.join(name))?;
    }
    Ok(())
}

/// The first `name` on `path` outside the guard's own folder.
fn find(path: &std::ffi::OsStr, name: &str, guard: &Path) -> Option<PathBuf> {
    let guard = fs::canonicalize(guard).ok();
    std::env::split_paths(path)
        .filter(|folder| fs::canonicalize(folder).ok() != guard)
        .map(|folder| folder.join(name))
        .find(|program| program.is_file())
}

/// The environment that puts the guard before an agent's `git` and `gh`, with the branch it may
/// push; none when the host installed no guard.
pub(crate) fn agent_env(push_branch: Option<&str>) -> Vec<(&'static str, OsString)> {
    let Some(installed) = INSTALLED.get() else {
        return vec![];
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    environment(installed, &path, push_branch)
}

fn environment(
    installed: &Installed,
    path: &std::ffi::OsStr,
    push_branch: Option<&str>,
) -> Vec<(&'static str, OsString)> {
    let folders = std::iter::once(installed.folder.clone()).chain(std::env::split_paths(path));
    let mut env = vec![(
        "PATH",
        std::env::join_paths(folders).unwrap_or_else(|_| path.to_owned()),
    )];
    env.extend(installed.git.as_ref().map(|git| (REAL_GIT, git.into())));
    env.extend(installed.gh.as_ref().map(|gh| (REAL_GH, gh.into())));
    env.extend(push_branch.map(|branch| (PUSH_BRANCH, branch.into())));
    env
}

/// Runs the guard when this binary was started as `git` or `gh`, and never returns then.
pub fn run_if_called() {
    let mut args = std::env::args_os();
    let called = args.next().map(PathBuf::from);
    let Some(name) = called.as_deref().and_then(Path::file_name) else {
        return;
    };
    let (real, program) = match name.to_str() {
        Some("git") => (REAL_GIT, Program::Git),
        Some("gh") => (REAL_GH, Program::Gh),
        _ => return,
    };
    let args: Vec<OsString> = args.collect();
    let Some(real) = std::env::var_os(real) else {
        eprintln!(
            "Wiffletree's guard cannot find {}: it is not installed.",
            program.name()
        );
        std::process::exit(127);
    };
    let text: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let verdict = match program {
        Program::Git => {
            let push_branch = std::env::var(PUSH_BRANCH).ok();
            git(&text, push_branch.as_deref(), |globals, name| {
                alias(&real, globals, name)
            })
        }
        Program::Gh => gh(&text),
    };
    match verdict {
        Verdict::Refuse(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
        Verdict::Run => exec(&real, args),
        Verdict::Push(at) => {
            let mut args = args;
            let pinned = ["--no-follow-tags", "--recurse-submodules=no"];
            args.splice(at + 1..at + 1, pinned.map(OsString::from));
            exec(&real, args)
        }
    }
}

fn exec(real: &std::ffi::OsStr, args: Vec<OsString>) -> ! {
    let error = Command::new(real).args(args).exec();
    eprintln!(
        "Wiffletree's guard could not run {}: {error}",
        real.to_string_lossy()
    );
    std::process::exit(127);
}

/// What `alias.<name>` is under these global options, read with the real `git`.
fn alias(real: &std::ffi::OsStr, globals: &[String], name: &str) -> Option<String> {
    let output = Command::new(real)
        .args(globals)
        .args(["config", "--get", &format!("alias.{name}")])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[derive(Clone, Copy)]
enum Program {
    Git,
    Gh,
}
impl Program {
    fn name(self) -> &'static str {
        match self {
            Program::Git => "git",
            Program::Gh => "gh",
        }
    }
}

/// What the guard does with a call.
#[derive(Debug, PartialEq)]
enum Verdict {
    Run,
    /// A push it allows, whose `push` is the argument at this index.
    Push(usize),
    Refuse(String),
}

/// Git's global options that take their value as the next argument.
const GIT_VALUED: [&str; 8] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--config-env",
    "--super-prefix",
    "--attr-source",
];
/// Git commands that are never an alias, since Git ignores an alias of a command it has; the
/// common ones, so most calls need no lookup.
const GIT_COMMANDS: [&str; 46] = [
    "add",
    "am",
    "apply",
    "bisect",
    "blame",
    "branch",
    "cat-file",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "describe",
    "diff",
    "fetch",
    "for-each-ref",
    "format-patch",
    "grep",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "merge",
    "merge-base",
    "mv",
    "pull",
    "range-diff",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "show",
    "show-ref",
    "stash",
    "status",
    "switch",
    "symbolic-ref",
    "tag",
    "worktree",
];
/// Commands that send commits to another repository without `push`.
const GIT_SENDERS: [&str; 2] = ["send-pack", "http-push"];
/// The options a push may carry.
const PUSH_OPTIONS: [&str; 15] = [
    "--force-with-lease",
    "--force-if-includes",
    "-u",
    "--set-upstream",
    "--no-verify",
    "-q",
    "--quiet",
    "-v",
    "--verbose",
    "-n",
    "--dry-run",
    "--porcelain",
    "--progress",
    "--no-follow-tags",
    "--atomic",
];
const NO_PUSH: &str = "Wiffletree refused `git push`: this agent may not push. publish_ticket pushes a ticket's branch and opens its pull request, the coordinator creates a branch with create_branch, and anything else goes to your coordinator.";

/// Judges a `git` call. `alias` reads `alias.<name>` under the call's global options.
fn git(
    args: &[String],
    push_branch: Option<&str>,
    alias: impl Fn(&[String], &str) -> Option<String>,
) -> Verdict {
    let mut args = args.to_vec();
    for _ in 0..ALIAS_DEPTH {
        let Some(at) = git_subcommand(&args) else {
            return Verdict::Run;
        };
        let name = args[at].as_str();
        if name == "push" {
            return match push_branch {
                Some(branch) => push(&args[at + 1..], branch, at),
                None => Verdict::Refuse(NO_PUSH.into()),
            };
        }
        if GIT_SENDERS.contains(&name) {
            return Verdict::Refuse(format!("Wiffletree refused `git {name}`: {NO_PUSH}"));
        }
        if GIT_COMMANDS.contains(&name) {
            return Verdict::Run;
        }
        // A shell alias runs its own shell, whose `git` is this guard again.
        match alias(&args[..at], name) {
            Some(expansion) if !expansion.starts_with('!') => {
                let words = expansion.split_whitespace().map(str::to_owned);
                args.splice(at..=at, words);
            }
            _ => return Verdict::Run,
        }
    }
    Verdict::Refuse("Wiffletree refused this `git` call: its aliases go too deep to check.".into())
}

/// The index of `git`'s subcommand, after its global options.
fn git_subcommand(args: &[String]) -> Option<usize> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if !arg.starts_with('-') {
            return Some(at);
        }
        at += if GIT_VALUED.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    None
}

/// Judges a push's arguments, after `push`, for an agent that may push `branch`.
fn push(args: &[String], branch: &str, at: usize) -> Verdict {
    let only = |why: &str| {
        Verdict::Refuse(format!(
            "Wiffletree refused this push: {why}You may push only your ticket's branch, as `git push origin HEAD:{branch}`, with --force-with-lease to replace it; publish_ticket opens the pull request."
        ))
    };
    let mut positionals = vec![];
    let mut options_end = false;
    for arg in args {
        if options_end || !arg.starts_with('-') || arg == "-" {
            positionals.push(arg.as_str());
        } else if arg == "--" {
            options_end = true;
        } else if matches!(arg.as_str(), "-f" | "--force") {
            return only("plain force is refused. ");
        } else if !PUSH_OPTIONS.contains(&arg.as_str()) && !arg.starts_with("--force-with-lease=") {
            return only(&format!("{arg} is not allowed. "));
        }
    }
    let Some((remote, refspecs)) = positionals.split_first() else {
        return only("");
    };
    if *remote != "origin" {
        return only("it pushes only to origin. ");
    }
    if refspecs.is_empty() {
        return only("name what to push. ");
    }
    for refspec in refspecs {
        if refspec.starts_with('+') {
            return only("plain force is refused. ");
        }
        let destination = match refspec.split_once(':') {
            Some(("", _)) => return only("deleting a branch is refused. "),
            Some((_, destination)) => destination,
            None => refspec,
        };
        if destination
            .strip_prefix("refs/heads/")
            .unwrap_or(destination)
            != branch
        {
            return only(&format!("{refspec} is not your branch. "));
        }
    }
    Verdict::Push(at)
}

/// `gh`'s reads: a command, and its subcommands, or `None` for any.
const GH_READS: [(&str, Option<&[&str]>); 13] = [
    ("pr", Some(&["view", "list", "diff", "checks", "status"])),
    ("issue", Some(&["view", "list", "status"])),
    ("run", Some(&["view", "list", "watch"])),
    ("repo", Some(&["view"])),
    ("release", Some(&["view", "list"])),
    ("workflow", Some(&["view", "list"])),
    ("label", Some(&["list"])),
    ("ruleset", Some(&["view", "list"])),
    ("auth", Some(&["status"])),
    ("search", None),
    ("help", None),
    ("--help", None),
    ("--version", None),
];

/// Judges a `gh` call.
fn gh(args: &[String]) -> Verdict {
    let words = without_repository(args);
    let (first, second) = (words.first().copied(), words.get(1).copied());
    let Some(first) = first else {
        return Verdict::Run;
    };
    if first == "api" {
        return api(&words[1..]);
    }
    let allowed = GH_READS.iter().any(|(command, subcommands)| {
        *command == first
            && subcommands
                .is_none_or(|subcommands| second.is_some_and(|s| subcommands.contains(&s)))
    });
    if allowed {
        return Verdict::Run;
    }
    let call = match second {
        Some(second) => format!("gh {first} {second}"),
        None => format!("gh {first}"),
    };
    Verdict::Refuse(match (first, second) {
        ("pr", Some("merge")) => {
            format!(
                "Wiffletree refused `{call}`: only the coordinator merges a pull request, with merge_ticket."
            )
        }
        ("pr", Some("create" | "edit")) => {
            format!(
                "Wiffletree refused `{call}`: publish_ticket opens and updates the pull request."
            )
        }
        _ => format!(
            "Wiffletree refused `{call}`: agents only read from GitHub here. Ask your coordinator, who asks the human for anything else."
        ),
    })
}

/// `gh`'s arguments without `-R`/`--repo` and `--hostname` and their values.
fn without_repository(args: &[String]) -> Vec<&str> {
    let mut words = vec![];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-R" | "--repo" | "--hostname" => {
                args.next();
            }
            a if a.starts_with("--repo=") || a.starts_with("--hostname=") => {}
            a if a.starts_with("-R") && a.len() > 2 => {}
            a => words.push(a),
        }
    }
    words
}

/// Judges `gh api`'s arguments: only a GET of a REST endpoint passes.
fn api(args: &[&str]) -> Verdict {
    let mut method = None;
    let mut body = false;
    let mut endpoint = None;
    let mut args = args.iter();
    while let Some(&arg) = args.next() {
        match arg {
            "-X" | "--method" => method = args.next().map(|m| m.to_uppercase()),
            "-f" | "-F" | "--field" | "--raw-field" | "--input" => {
                body = true;
                args.next();
            }
            a if a.starts_with("--method=") => method = Some(a["--method=".len()..].to_uppercase()),
            a if a.starts_with("-X") => method = Some(a[2..].to_uppercase()),
            a if ["--field=", "--raw-field=", "--input="]
                .iter()
                .any(|p| a.starts_with(p)) =>
            {
                body = true
            }
            a if (a.starts_with("-f") || a.starts_with("-F")) && a.len() > 2 => body = true,
            a if !a.starts_with('-') && endpoint.is_none() => endpoint = Some(a),
            _ => {}
        }
    }
    let method = method.unwrap_or_else(|| if body { "POST" } else { "GET" }.into());
    if endpoint == Some("graphql") {
        return Verdict::Refuse(
            "Wiffletree refused `gh api graphql`: agents only read from GitHub here, with gh api GET calls or gh's read commands.".into(),
        );
    }
    match method.as_str() {
        "GET" => Verdict::Run,
        _ => Verdict::Refuse(format!(
            "Wiffletree refused `gh api` with {method}: agents only read from GitHub here. Ask your coordinator, who asks the human for anything else."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRANCH: &str = "wiffletree/fix-x";

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }
    fn no_alias(_: &[String], _: &str) -> Option<String> {
        None
    }
    fn implementer(line: &str) -> Verdict {
        git(&words(line), Some(BRANCH), no_alias)
    }
    fn refused(verdict: Verdict) -> String {
        match verdict {
            Verdict::Refuse(why) => why,
            other => panic!("not refused: {other:?}"),
        }
    }

    #[test]
    fn an_implementer_pushes_its_own_branch() {
        for line in [
            "push origin HEAD:wiffletree/fix-x",
            "push origin HEAD:refs/heads/wiffletree/fix-x",
            "push origin wiffletree/fix-x",
            "push -u origin HEAD:wiffletree/fix-x",
            "push --force-with-lease origin HEAD:wiffletree/fix-x",
            "push --force-with-lease=wiffletree/fix-x:abc1234 --force-if-includes origin HEAD:wiffletree/fix-x",
        ] {
            assert_eq!(implementer(line), Verdict::Push(0), "{line}");
        }
    }

    #[test]
    fn global_options_before_push_are_skipped() {
        let line = "-C ../fix-x push --force-with-lease origin HEAD:refs/heads/wiffletree/fix-x";
        assert_eq!(implementer(line), Verdict::Push(2));
        let line = "-c push.default=current --no-pager push origin HEAD:wiffletree/fix-x";
        assert_eq!(implementer(line), Verdict::Push(3));
        let why = refused(implementer("-c push.default=current push origin HEAD:main"));
        assert!(
            why.contains("git push origin HEAD:wiffletree/fix-x"),
            "{why}"
        );
        refused(implementer("--git-dir .git --work-tree . push origin main"));
    }

    #[test]
    fn every_other_push_is_refused() {
        for line in [
            "push origin main",
            "push origin HEAD:main",
            "push --tags",
            "push origin --tags HEAD:wiffletree/fix-x",
            "push origin --follow-tags HEAD:wiffletree/fix-x",
            "push origin refs/tags/v1:refs/tags/v1",
            "push origin :wiffletree/fix-x",
            "push --delete origin wiffletree/fix-x",
            "push -d origin wiffletree/fix-x",
            "push --all origin",
            "push --mirror origin",
            "push --prune origin HEAD:wiffletree/fix-x",
            "push origin HEAD:wiffletree/fix-x HEAD:main",
            "push https://github.com/o/r HEAD:wiffletree/fix-x",
            "push upstream HEAD:wiffletree/fix-x",
            "push",
            "push origin",
            "push origin HEAD",
            "push --recurse-submodules=on-demand origin HEAD:wiffletree/fix-x",
        ] {
            refused(implementer(line));
        }
    }

    #[test]
    fn plain_force_names_the_lease() {
        for line in [
            "push -f origin HEAD:wiffletree/fix-x",
            "push --force origin HEAD:wiffletree/fix-x",
            "push origin +HEAD:wiffletree/fix-x",
        ] {
            let why = refused(implementer(line));
            assert!(
                why.contains("plain force") && why.contains("--force-with-lease"),
                "{why}"
            );
        }
    }

    #[test]
    fn without_a_branch_nothing_is_pushed() {
        for line in ["push origin HEAD:wiffletree/fix-x", "push origin feature/x"] {
            let why = refused(git(&words(line), None, no_alias));
            assert!(
                why.contains("publish_ticket") && why.contains("create_branch"),
                "{why}"
            );
        }
    }

    #[test]
    fn senders_are_refused() {
        refused(implementer("send-pack origin main"));
        refused(implementer("-C x http-push url main"));
    }

    #[test]
    fn an_alias_is_judged_as_what_it_expands_to() {
        let aliases = |globals: &[String], name: &str| match name {
            "p" => Some("push".to_owned()),
            "pp" => Some("p --force".to_owned()),
            "sh" => Some("!git push origin main".to_owned()),
            "co" if globals == ["-c", "alias.co=push"] => Some("push".to_owned()),
            _ => None,
        };
        let judge = |line: &str| git(&words(line), Some(BRANCH), aliases);
        refused(judge("p origin main"));
        assert_eq!(judge("p origin HEAD:wiffletree/fix-x"), Verdict::Push(0));
        refused(judge("pp origin HEAD:wiffletree/fix-x"));
        refused(judge("-c alias.co=push co origin main"));
        assert_eq!(judge("co origin main"), Verdict::Run);
        // A shell alias's own `git push` meets the guard again.
        assert_eq!(judge("sh"), Verdict::Run);
        let looping = |_: &[String], _: &str| Some("loop".to_owned());
        refused(git(&words("loop"), Some(BRANCH), looping));
    }

    #[test]
    fn reads_and_local_work_run() {
        let unasked =
            |_: &[String], name: &str| -> Option<String> { panic!("{name} needs no alias lookup") };
        for line in [
            "status",
            "-C dir log --oneline",
            "fetch origin",
            "commit -m x",
            "rebase origin/main",
            "--version",
            "",
        ] {
            assert_eq!(git(&words(line), None, unasked), Verdict::Run, "{line}");
        }
    }

    fn gh_line(line: &str) -> Verdict {
        gh(&words(line))
    }

    #[test]
    fn gh_reads_run() {
        for line in [
            "pr view 1 --json title",
            "-R o/r pr checks 1",
            "pr --repo o/r diff 1",
            "pr list --repo=o/r",
            "run view 5 --log",
            "search prs foo",
            "issue view 3",
            "repo view",
            "auth status",
            "--version",
            "",
            "api repos/o/r/pulls/1/comments",
            "api -X GET search/issues -f q=x",
            "api --method=get repos/o/r --input body.json -XGET",
            "api --hostname github.com repos/o/r",
        ] {
            assert_eq!(gh_line(line), Verdict::Run, "{line}");
        }
    }

    #[test]
    fn gh_writes_are_refused() {
        for line in [
            "pr close 1",
            "-R o/r pr close 1",
            "pr create",
            "pr comment 1 -b hi",
            "pr review 1 --approve",
            "pr edit 1",
            "pr ready 1",
            "pr reopen 1",
            "pr checkout 1",
            "issue create",
            "release create v1",
            "repo delete",
            "secret set X",
            "variable set X",
            "workflow run ci",
            "alias set x pr",
            "x",
            "pr",
            "api -X POST repos/o/r/issues",
            "api --method=PATCH repos/o/r",
            "api -XDELETE repos/o/r",
            "api repos/o/r/issues -f title=x",
            "api repos/o/r/issues -ftitle=x",
            "api repos/o/r/issues --input body.json",
            "api -X GET repos/o/r -X POST",
            "api graphql -f query=x",
            "api -X GET graphql",
        ] {
            refused(gh_line(line));
        }
    }

    #[test]
    fn gh_refusals_name_the_tool() {
        assert!(refused(gh_line("pr merge 1")).contains("merge_ticket"));
        assert!(refused(gh_line("-R o/r pr create")).contains("publish_ticket"));
        assert!(refused(gh_line("pr close 1")).contains("coordinator"));
    }

    #[test]
    fn the_environment_puts_the_guard_first() {
        let installed = Installed {
            folder: "/data/guard".into(),
            git: Some("/usr/bin/git".into()),
            gh: None,
        };
        let env = environment(
            &installed,
            "/opt/homebrew/bin:/usr/bin".as_ref(),
            Some(BRANCH),
        );
        assert_eq!(
            env,
            [
                ("PATH", "/data/guard:/opt/homebrew/bin:/usr/bin".into()),
                (REAL_GIT, "/usr/bin/git".into()),
                (PUSH_BRANCH, BRANCH.into()),
            ]
        );
        let env = environment(&installed, "/usr/bin".as_ref(), None);
        assert!(env.iter().all(|(name, _)| *name != PUSH_BRANCH));
    }

    #[test]
    fn install_links_both_names_and_skips_its_own_folder() {
        let data = tempfile::tempdir().unwrap();
        let binary = data.path().join("host");
        fs::write(&binary, "").unwrap();
        let folder = data.path().join(FOLDER);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("git"), "").unwrap();
        link(&folder, &binary).unwrap();
        for name in PROGRAMS {
            assert_eq!(fs::read_link(folder.join(name)).unwrap(), binary);
        }
        let other = data.path().join("bin");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("git"), "").unwrap();
        let path = std::env::join_paths([&folder, &other]).unwrap();
        assert_eq!(find(&path, "git", &folder), Some(other.join("git")));
    }
}
