//! How the host reaches a GitHub repository it syncs, as `gh` does: the URLs to try, gh's login
//! for Git over HTTPS, and the read-only check of who can read the repository. Shared by the
//! settings repository and, later, storage repositories, trackers and brains.
use crate::*;
use publish::{GitTool, Tools};

/// GitHub's owner and name, and the URLs to try reaching it by, in order: the URL named, or for
/// `owner/name` the URL of gh's Git `protocol` first and then the other's.
pub(crate) fn candidates(input: &str, protocol: &str) -> Result<(String, String, Vec<String>)> {
    let input = input.trim();
    let named = input.contains(':');
    let url = match named {
        true => input.to_owned(),
        false => format!("https://github.com/{input}.git"),
    };
    let (owner, name) = pr_watch::github_repository(&url)
        .context("Name a repository on github.com as owner/name or by its URL")?;
    let ssh = format!("git@github.com:{owner}/{name}.git");
    let urls = match (named, protocol) {
        (true, _) => vec![url],
        (false, "ssh") => vec![ssh, url],
        (false, _) => vec![url, ssh],
    };
    Ok((owner, name, urls))
}

/// The Git protocol gh uses for github.com: `ssh`, or `https`, gh's default.
pub(crate) fn gh_protocol(tools: &Tools, home: &Path) -> String {
    let args = [
        "config",
        "get",
        "git_protocol",
        "-h",
        repositories::GITHUB_HOST,
    ];
    match (tools.gh)(home, &args) {
        Ok(answer) if answer.status.success() => answer.output.trim().to_owned(),
        _ => "https".into(),
    }
}

/// Git options that give Git gh's GitHub login for one command over HTTPS, in place of any
/// credential helper Git has, so no token is stored or named on a command line.
pub(crate) fn gh_login(url: &str) -> &'static [&'static str] {
    match url.starts_with("https://github.com/") {
        true => &[
            "-c",
            "credential.https://github.com.helper=",
            "-c",
            "credential.https://github.com.helper=!gh auth git-credential",
        ],
        false => &[],
    }
}

/// Runs Git through `git`, refusing a failed command with Git's reason.
pub(crate) fn run(git: &GitTool, path: &Path, args: &[&str]) -> Result<String> {
    let answer = git(path, args)?;
    ensure!(
        answer.status.success(),
        "git {} failed{}",
        publish::subcommand(args),
        answer.reason()
    );
    Ok(answer.output)
}

/// `run` for a command that reaches the repository at `url`: over HTTPS with gh's login.
pub(crate) fn run_at(git: &GitTool, path: &Path, url: &str, args: &[&str]) -> Result<String> {
    run(git, path, &[gh_login(url), args].concat())
}

fn gh_json(tools: &Tools, home: &Path, endpoint: &str) -> Result<Value> {
    let args = ["api", "--hostname", repositories::GITHUB_HOST, endpoint];
    let answer = (tools.gh)(home, &args)?;
    ensure!(
        answer.status.success(),
        "GitHub could not be read: {}",
        answer.error
    );
    Ok(serde_json::from_str(&answer.output)?)
}

/// The branch's head at `url`, or `None` while the repository has no commit on it. Reading it
/// is how this machine's access is checked.
pub(crate) fn remote_head(
    path: &Path,
    url: &str,
    branch: &str,
    git: &GitTool,
) -> Result<Option<String>> {
    let reference = format!("refs/heads/{branch}");
    let heads = run_at(git, path, url, &["ls-remote", url, &reference])?;
    Ok(heads.split_whitespace().next().map(str::to_owned))
}

/// The first of `urls` this machine can read `branch` at, with the branch's head there, or
/// every URL tried and why it could not be read.
pub(crate) fn first_readable(
    path: &Path,
    urls: Vec<String>,
    branch: &str,
    git: &GitTool,
) -> Result<(String, Option<String>)> {
    let mut reasons = vec![];
    for url in urls {
        match remote_head(path, &url, branch, git) {
            Ok(head) => return Ok((url, head)),
            Err(error) => reasons.push(format!("{url}: {error:#}")),
        }
    }
    bail!(
        "This machine cannot read the repository. Tried {}. Add an SSH key to GitHub, sign gh in with gh auth login, or check that the repository exists and is yours to read.",
        reasons.join("; tried ")
    )
}

/// What GitHub says, read-only, about a private repository.
pub(crate) struct Private {
    /// `owner/name`, as GitHub names it.
    pub(crate) name: String,
    pub(crate) default_branch: String,
    pub(crate) organization: bool,
    pub(crate) readers: Readers,
}

/// Asks GitHub who can read `owner/name`, refusing a repository that is not private.
pub(crate) fn private_repository(
    tools: &Tools,
    home: &Path,
    owner: &str,
    name: &str,
) -> Result<Private> {
    let facts = gh_json(tools, home, &format!("repos/{owner}/{name}"))?;
    ensure!(
        facts["private"] == true,
        "Choose a private repository. Your skills and settings would be public."
    );
    let organization = facts["owner"]["type"] == "Organization";
    let readers = match organization {
        _ if facts["visibility"] == "internal" => Readers::EnterpriseMembers,
        false => Readers::Invited,
        // GitHub reports an organization's base permission only to some of its members.
        true => match gh_json(tools, home, &format!("orgs/{owner}"))
            .ok()
            .and_then(|org| {
                org["default_repository_permission"]
                    .as_str()
                    .map(str::to_owned)
            })
            .as_deref()
        {
            None => Readers::MaybeMembers,
            Some("none") => Readers::Invited,
            Some(_) => Readers::EveryMember,
        },
    };
    let field = |key: &str| facts[key].as_str().map(str::to_owned);
    Ok(Private {
        default_branch: field("default_branch").context("GitHub named no default branch")?,
        name: field("full_name").context("GitHub named no repository")?,
        organization,
        readers,
    })
}
