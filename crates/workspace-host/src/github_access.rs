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

pub(crate) fn gh_json(tools: &Tools, home: &Path, endpoint: &str) -> Result<Value> {
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
    /// GitHub lets this Mac's login push to it.
    pub(crate) push: bool,
}

/// Asks GitHub who can read `owner/name`, refusing a repository that is not private with
/// `public`, which says why it must be.
pub(crate) fn private_repository(
    tools: &Tools,
    home: &Path,
    owner: &str,
    name: &str,
    public: &str,
) -> Result<Private> {
    let facts = gh_json(tools, home, &format!("repos/{owner}/{name}"))?;
    ensure!(facts["private"] == true, "{public}");
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
        // A login GitHub gives no permissions for is left to the first push to refuse.
        push: facts["permissions"]["push"] != false,
    })
}

/// How many people GitHub lists as able to read the repository, when it lets this Mac count
/// them: an organization's members when every member reads it, or else its collaborators.
pub(crate) fn reader_count(
    tools: &Tools,
    home: &Path,
    name: &str,
    readers: Readers,
) -> Option<usize> {
    let owner = workspace_core::repository_owner(name);
    let endpoint = match readers {
        Readers::EveryMember => format!("orgs/{owner}/members?per_page=100"),
        Readers::Invited => format!("repos/{name}/collaborators?per_page=100"),
        Readers::EnterpriseMembers | Readers::MaybeMembers => return None,
    };
    Some(gh_json(tools, home, &endpoint).ok()?.as_array()?.len())
}

/// Reads the rulesets and branch protection on `branch` of the repository `name`, refusing a
/// branch whose rules require pull requests or status checks, or take no push, as Wiffletree
/// pushes straight to it. Answers whether they protect its history: block force-pushes and
/// deleting it.
pub(crate) fn direct_push(tools: &Tools, home: &Path, name: &str, branch: &str) -> Result<bool> {
    let cannot = |rule: &str| {
        anyhow::anyhow!(
            "{branch} {rule}, so Wiffletree can't write to it. Choose another repository, or turn that rule off on GitHub."
        )
    };
    // A plan without rulesets for a private repository answers 403, and then none can apply.
    let rules = match gh_json(
        tools,
        home,
        &format!("repos/{name}/rules/branches/{branch}"),
    ) {
        Ok(rules) => rules,
        Err(error)
            if ["HTTP 403", "HTTP 404"]
                .iter()
                .any(|code| format!("{error:#}").contains(code)) =>
        {
            Value::Null
        }
        Err(error) => return Err(error),
    };
    let types: Vec<&str> = (rules.as_array().into_iter().flatten())
        .filter_map(|rule| rule["type"].as_str())
        .collect();
    for (kind, rule) in [
        ("pull_request", "requires pull requests"),
        ("required_status_checks", "requires status checks"),
        ("update", "takes no push"),
    ] {
        if types.contains(&kind) {
            return Err(cannot(rule));
        }
    }
    let mut protected = types.contains(&"non_fast_forward") && types.contains(&"deletion");
    // A branch that does not exist yet, as in a new repository, has no protection.
    let Ok(summary) = gh_json(tools, home, &format!("repos/{name}/branches/{branch}")) else {
        return Ok(protected);
    };
    if summary["protected"] != true {
        return Ok(protected);
    }
    // The whole protection needs a repository admin; the summary says enough without it.
    match gh_json(
        tools,
        home,
        &format!("repos/{name}/branches/{branch}/protection"),
    ) {
        Ok(protection) => {
            if protection["required_pull_request_reviews"].is_object() {
                return Err(cannot("requires pull requests"));
            }
            let checks = &protection["required_status_checks"];
            let listed = |key: &str| checks[key].as_array().is_some_and(|l| !l.is_empty());
            if listed("contexts") || listed("checks") {
                return Err(cannot("requires status checks"));
            }
            if protection["lock_branch"]["enabled"] == true {
                return Err(cannot("takes no push"));
            }
            let allowed = |key: &str| protection[key]["enabled"] == true;
            protected |= !allowed("allow_force_pushes") && !allowed("allow_deletions");
        }
        Err(_) => {
            let checks = &summary["protection"]["required_status_checks"];
            if checks["enforcement_level"]
                .as_str()
                .is_some_and(|l| l != "off")
            {
                return Err(cannot("requires status checks"));
            }
            // Classic protection blocks force-pushes and deletion unless allowed.
            protected = true;
        }
    }
    Ok(protected)
}

/// Creates the repository `owner/name` on GitHub, private and empty.
pub(crate) fn create_private(tools: &Tools, home: &Path, name: &str) -> Result<()> {
    let answer = (tools.gh)(home, &["repo", "create", name, "--private"])?;
    ensure!(
        answer.status.success(),
        "GitHub did not create {name}: {}",
        answer.error.trim()
    );
    Ok(())
}

/// The ruleset Protect history adds: force-pushes and deleting the default branch are blocked,
/// and nothing else is required.
pub(crate) fn history_ruleset() -> Value {
    serde_json::json!({
        "name": "Protect history",
        "target": "branch",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
        "rules": [{"type": "non_fast_forward"}, {"type": "deletion"}],
    })
}

/// Protect history: adds `history_ruleset` to the repository `name`. GitHub's refusal, for
/// want of an admin or of a plan that allows rulesets on a private repository, is the error,
/// in GitHub's words.
pub(crate) fn protect_history(tools: &Tools, home: &Path, name: &str) -> Result<()> {
    let body = tempfile::NamedTempFile::new_in(home)?;
    fs::write(body.path(), history_ruleset().to_string())?;
    let input = body
        .path()
        .to_str()
        .context("The data folder's path is not UTF-8")?;
    let endpoint = format!("repos/{name}/rulesets");
    let args = [
        "api",
        "--hostname",
        repositories::GITHUB_HOST,
        "--method",
        "POST",
        &endpoint,
        "--input",
        input,
    ];
    let answer = (tools.gh)(home, &args)?;
    ensure!(
        answer.status.success(),
        "GitHub refused: {}",
        answer.error.trim()
    );
    Ok(())
}
