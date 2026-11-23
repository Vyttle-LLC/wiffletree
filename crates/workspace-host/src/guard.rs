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
        Verdict::Push(args) => exec(&real, args.into_iter().map(OsString::from).collect()),
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
    /// A push it allows, run with these arguments: every ref it names spelled out under
    /// `refs/heads/`, so none can resolve to a tag, and tags and submodules left out.
    Push(Vec<String>),
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
const NO_PUSH: &str = "Wiffletree refused `git push`: this agent may not push now, and once a ticket's pull request is open no agent pushes its branch. publish_ticket pushes a ticket's branch and opens or updates its pull request, the coordinator creates a branch with create_branch, and anything else goes to your coordinator.";

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
                Some(branch) => push(&args, at, branch),
                None => Verdict::Refuse(NO_PUSH.into()),
            };
        }
        if GIT_SENDERS.contains(&name) {
            return Verdict::Refuse(format!("Wiffletree refused `git {name}`: {NO_PUSH}"));
        }
        if GIT_COMMANDS.contains(&name) {
            return Verdict::Run;
        }
        // A shell alias runs its shell with Git's exec folder first on `PATH`, so its `git` is
        // the real one, which the guard could not judge.
        match alias(&args[..at], name) {
            Some(expansion) if expansion.starts_with('!') => {
                return Verdict::Refuse(format!(
                    "Wiffletree refused `git {name}`: it is a shell alias, which the guard cannot check. Run its commands yourself."
                ));
            }
            Some(expansion) => {
                let words = expansion.split_whitespace().map(str::to_owned);
                args.splice(at..=at, words);
            }
            None => return Verdict::Run,
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

/// Judges a push, whose `push` is `args[at]`, for an agent that may push `branch`, and spells
/// out the branch as `refs/heads/<branch>` wherever the push names it.
fn push(args: &[String], at: usize, branch: &str) -> Verdict {
    let only = |why: &str| {
        Verdict::Refuse(format!(
            "Wiffletree refused this push: {why}You may push only your ticket's branch, as `git push origin HEAD:{branch}`, with --force-with-lease to replace it; publish_ticket opens the pull request."
        ))
    };
    let names_branch = |name: &str| name.strip_prefix("refs/heads/").unwrap_or(name) == branch;
    let full = format!("refs/heads/{branch}");
    let mut sent = args[..=at].to_vec();
    sent.extend(["--no-follow-tags", "--recurse-submodules=no"].map(str::to_owned));
    let mut positionals = vec![];
    let mut options_end = false;
    for arg in &args[at + 1..] {
        if options_end || !arg.starts_with('-') || arg == "-" {
            positionals.push(arg.as_str());
        } else if arg == "--" {
            options_end = true;
        } else if matches!(arg.as_str(), "-f" | "--force") {
            return only("plain force is refused. ");
        } else if let Some(lease) = arg.strip_prefix("--force-with-lease=") {
            let (name, expected) = match lease.split_once(':') {
                Some((name, expected)) => (name, format!(":{expected}")),
                None => (lease, String::new()),
            };
            if !names_branch(name) {
                return only(&format!("{arg} names a ref that is not your branch. "));
            }
            sent.push(format!("--force-with-lease={full}{expected}"));
        } else if PUSH_OPTIONS.contains(&arg.as_str()) {
            sent.push(arg.clone());
        } else {
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
    sent.extend(["--".to_owned(), "origin".to_owned()]);
    for refspec in refspecs {
        if refspec.starts_with('+') {
            return only("plain force is refused. ");
        }
        let (source, destination) = match refspec.split_once(':') {
            Some(("", _)) => return only("deleting a branch is refused. "),
            Some(pair) => pair,
            None => (*refspec, *refspec),
        };
        if !names_branch(destination) {
            return only(&format!("{refspec} is not your branch. "));
        }
        sent.push(format!("{source}:{full}"));
    }
    Verdict::Push(sent)
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
    // Git's credential helper, as `gh auth setup-git` configures it, reads the sign-in.
    ("auth", Some(&["status", "git-credential"])),
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
    // `api` takes no `-R`, so its own arguments are read as given.
    if first == "api" {
        return match args.split_first() {
            Some((api_word, rest)) if api_word == "api" => api(rest),
            _ => Verdict::Refuse("Wiffletree refused this `gh api` call: name api first.".into()),
        };
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

/// `gh api`'s options that take a value, by their short and long names; its other options,
/// `API_SWITCHES`, take none. An option on neither list is refused, since its arity is unknown.
const API_VALUED: [(Option<char>, &str); 10] = [
    (Some('X'), "method"),
    (Some('f'), "raw-field"),
    (Some('F'), "field"),
    (Some('H'), "header"),
    (Some('q'), "jq"),
    (Some('t'), "template"),
    (Some('p'), "preview"),
    (None, "input"),
    (None, "hostname"),
    (None, "cache"),
];
const API_SWITCHES: [(Option<char>, &str); 6] = [
    (Some('i'), "include"),
    (Some('h'), "help"),
    (None, "paginate"),
    (None, "silent"),
    (None, "slurp"),
    (None, "verbose"),
];

/// `gh api`'s options, by long name with their values, and its first positional argument, read
/// as gh's flag parser reads them: a long option's value after `=` or as the next argument, and
/// short switches clustered, the last letter able to take the rest of the word, after an
/// optional `=`, or the next argument as its value. The error is an argument it cannot read.
fn api_arguments(args: &[String]) -> Result<ApiCall<'_>, &str> {
    let switch_named = |name: &str| API_SWITCHES.iter().any(|(_, long)| *long == name);
    let mut options = vec![];
    let mut endpoint = None;
    let mut options_end = false;
    let mut args = args.iter().map(String::as_str);
    while let Some(arg) = args.next() {
        if options_end || !arg.starts_with('-') || arg == "-" {
            endpoint = endpoint.or(Some(arg));
        } else if arg == "--" {
            options_end = true;
        } else if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            if switch_named(name) {
                continue;
            }
            let Some((_, name)) = API_VALUED.iter().find(|(_, long)| *long == name) else {
                return Err(arg);
            };
            options.push((*name, value.or_else(|| args.next()).ok_or(arg)?));
        } else {
            for (at, letter) in arg.char_indices().skip(1) {
                if API_SWITCHES.iter().any(|(short, _)| *short == Some(letter)) {
                    continue;
                }
                let Some((_, name)) = API_VALUED.iter().find(|(short, _)| *short == Some(letter))
                else {
                    return Err(arg);
                };
                let rest = &arg[at + letter.len_utf8()..];
                let rest = rest.strip_prefix('=').unwrap_or(rest);
                let value = match rest {
                    "" => args.next().ok_or(arg)?,
                    rest => rest,
                };
                options.push((*name, value));
                break;
            }
        }
    }
    Ok(ApiCall { options, endpoint })
}

/// A `gh api` call as gh reads it: its valued options by long name, and its endpoint.
struct ApiCall<'a> {
    options: Vec<(&'static str, &'a str)>,
    endpoint: Option<&'a str>,
}

/// Judges `gh api`'s arguments: only a GET of a REST endpoint passes.
fn api(args: &[String]) -> Verdict {
    let ApiCall { options, endpoint } = match api_arguments(args) {
        Ok(read) => read,
        Err(option) => {
            return Verdict::Refuse(format!(
                "Wiffletree refused this `gh api` call: it cannot read {option}, so it cannot tell the method."
            ));
        }
    };
    let body = (options.iter()).any(|(name, _)| matches!(*name, "raw-field" | "field" | "input"));
    let method = (options.iter().rev())
        .find(|(name, _)| *name == "method")
        .map(|(_, method)| method.to_uppercase());
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

    /// What a push runs, given the words before it and its options and refspecs as sent.
    fn sent(before: &str, options: &str, refspecs: &str) -> Verdict {
        let mut sent = words(before);
        sent.extend(words("push --no-follow-tags --recurse-submodules=no"));
        sent.extend(words(options));
        sent.extend(words("-- origin"));
        sent.extend(words(refspecs));
        Verdict::Push(sent)
    }
    const FULL: &str = "HEAD:refs/heads/wiffletree/fix-x";

    #[test]
    fn an_implementer_pushes_its_own_branch() {
        for (line, options, refspec) in [
            ("push origin HEAD:wiffletree/fix-x", "", FULL),
            ("push origin HEAD:refs/heads/wiffletree/fix-x", "", FULL),
            (
                "push origin wiffletree/fix-x",
                "",
                "wiffletree/fix-x:refs/heads/wiffletree/fix-x",
            ),
            ("push -u origin HEAD:wiffletree/fix-x", "-u", FULL),
            (
                "push --force-with-lease origin HEAD:wiffletree/fix-x",
                "--force-with-lease",
                FULL,
            ),
            (
                "push --force-with-lease=wiffletree/fix-x:abc1234 --force-if-includes origin HEAD:wiffletree/fix-x",
                "--force-with-lease=refs/heads/wiffletree/fix-x:abc1234 --force-if-includes",
                FULL,
            ),
        ] {
            assert_eq!(implementer(line), sent("", options, refspec), "{line}");
        }
    }

    #[test]
    fn a_push_never_reaches_a_tag_of_the_branchs_name() {
        let line = "push --force-with-lease=refs/tags/wiffletree/fix-x:55180e1 origin HEAD:wiffletree/fix-x";
        refused(implementer(line));
        let line = "push --force-with-lease=wiffletree/fix-x origin HEAD:wiffletree/fix-x";
        let options = "--force-with-lease=refs/heads/wiffletree/fix-x";
        assert_eq!(implementer(line), sent("", options, FULL));
    }

    #[test]
    fn global_options_before_push_are_skipped() {
        let line = "-C ../fix-x push --force-with-lease origin HEAD:refs/heads/wiffletree/fix-x";
        assert_eq!(
            implementer(line),
            sent("-C ../fix-x", "--force-with-lease", FULL)
        );
        let line = "-c push.default=current --no-pager push origin HEAD:wiffletree/fix-x";
        assert_eq!(
            implementer(line),
            sent("-c push.default=current --no-pager", "", FULL)
        );
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
        assert_eq!(judge("p origin HEAD:wiffletree/fix-x"), sent("", "", FULL));
        refused(judge("pp origin HEAD:wiffletree/fix-x"));
        refused(judge("-c alias.co=push co origin main"));
        assert_eq!(judge("co origin main"), Verdict::Run);
        // A shell alias's own `git` is the real one, so the alias is refused, whatever it runs.
        assert!(refused(judge("sh")).contains("shell alias"));
        assert!(refused(git(&words("sh"), None, aliases)).contains("shell alias"));
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
            "auth git-credential get",
            "--version",
            "",
            "api repos/o/r/pulls/1/comments",
            "api -X GET search/issues -f q=x",
            "api --method=get repos/o/r --input body.json -XGET",
            "api --hostname github.com repos/o/r",
            "api -iXGET repos/o/r",
            "api -X=GET repos/o/r -f q=x",
            "api --paginate --jq .[].id repos/o/r/pulls",
            "api repos/o/r --template -XPOST",
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
            // A template's value that looks like a method, and a method clustered after a
            // switch, are read as gh reads them.
            "api repos/o/r --template -XGET -f title=x",
            "api repos/o/r -iXPOST",
            "api repos/o/r -ifXGET",
            "api repos/o/r --unknown-flag -XGET",
            "api repos/o/r -z",
            "-R o/r api repos/o/r",
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
