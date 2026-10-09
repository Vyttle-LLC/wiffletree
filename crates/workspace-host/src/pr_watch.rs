//! Follows open tickets' GitHub PRs without a model. Each pass reads every ticket branch of one
//! repository with one read-only `gh api graphql` query, off the actor thread, and `Watch` paces
//! the passes. Nothing here writes to GitHub, branches or worktrees.
use crate::*;
use std::collections::{HashMap, HashSet};

/// The interval while a repository has an open PR, and otherwise.
const ACTIVE_MS: i64 = 60_000;
pub(crate) const IDLE_MS: i64 = 300_000;
const MAX_BACKOFF_MS: i64 = 1_800_000;
/// Below this many API points, the next pass waits for GitHub's reset, leaving headroom for the
/// human's and the agents' own `gh` use.
const RATE_LIMIT_FLOOR: u64 = 200;
/// Branches per query; nested connections multiply GraphQL's cost.
const BRANCHES_PER_QUERY: usize = 50;

/// One repository's open tickets, gathered on the host thread and read off it.
pub(crate) struct Query {
    pub repository: PathBuf,
    pub base: String,
    pub tickets: Vec<TicketBranch>,
}
pub(crate) struct TicketBranch {
    pub ticket_id: String,
    pub branch: String,
    pub worktree: PathBuf,
}
pub(crate) enum Outcome {
    /// The base has no remote on github.com, so nothing was asked.
    NotOnGitHub,
    Read(Pass),
    Failed(String),
}
pub(crate) struct Pass {
    /// The tickets that have a PR.
    pub found: Vec<Found>,
    pub rate_limit: RateLimit,
}
pub(crate) struct Found {
    pub ticket_id: String,
    pub pull_request: PullRequest,
    /// The ticket worktree's HEAD, read only for a merged PR.
    pub worktree_head: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RateLimit {
    pub remaining: u64,
    pub reset_at: i64,
}

/// Reads the query's PRs through `gh`.
pub(crate) fn fetch(query: &Query) -> Outcome {
    fetch_with(query, runtime::gh_output)
}

fn fetch_with(query: &Query, gh: impl Fn(&Path, &[&str]) -> Result<String>) -> Outcome {
    match read(query, gh) {
        Ok(Some(pass)) => Outcome::Read(pass),
        Ok(None) => Outcome::NotOnGitHub,
        Err(e) => Outcome::Failed(format!("{e:#}")),
    }
}

fn read(query: &Query, gh: impl Fn(&Path, &[&str]) -> Result<String>) -> Result<Option<Pass>> {
    let Some((owner, name)) = github_remote(&query.repository, &query.base)? else {
        return Ok(None);
    };
    let mut found = vec![];
    let mut rate_limit = None;
    for tickets in query.tickets.chunks(BRANCHES_PER_QUERY) {
        // Raw-string fields only: `-F` would let gh coerce values or read files.
        let mut fields = vec![
            format!("query={}", graphql(tickets.len())),
            format!("owner={owner}"),
            format!("name={name}"),
        ];
        fields.extend(
            tickets
                .iter()
                .enumerate()
                .map(|(i, t)| format!("h{i}={}", t.branch)),
        );
        let mut args = vec!["api", "graphql"];
        for field in &fields {
            args.extend(["-f", field]);
        }
        let output = gh(&query.repository, &args)?;
        let (pull_requests, limit) = parse(&serde_json::from_str(&output)?, tickets)?;
        rate_limit = Some(limit);
        for (ticket, pull_request) in pull_requests {
            let worktree_head = (pull_request.state == PrState::Merged)
                .then(|| runtime::git_output(&ticket.worktree, &["rev-parse", "HEAD"]).ok())
                .flatten()
                .map(|head| head.trim().to_owned());
            found.push(Found {
                ticket_id: ticket.ticket_id.clone(),
                pull_request,
                worktree_head,
            });
        }
    }
    Ok(Some(Pass {
        found,
        rate_limit: rate_limit.context("No ticket branches to read")?,
    }))
}

/// The GitHub owner and name of the base's remote, when the base is a remote ref on github.com.
fn github_remote(repository: &Path, base: &str) -> Result<Option<(String, String)>> {
    let Some((remote, _)) = base.split_once('/') else {
        return Ok(None);
    };
    if !runtime::git_output(repository, &["remote"])?
        .lines()
        .any(|r| r == remote)
    {
        return Ok(None);
    }
    let url = runtime::git_output(repository, &["remote", "get-url", remote])?;
    Ok(github_repository(url.trim()))
}

/// Owner and name from an HTTPS or SSH `github.com` remote URL.
pub(crate) fn github_repository(remote_url: &str) -> Option<(String, String)> {
    let path = [
        "https://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| remote_url.strip_prefix(prefix))?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (valid(owner) && valid(name)).then(|| (owner.to_owned(), name.to_owned()))
}

const PULL_REQUEST_FIELDS: &str = "fragment pr on PullRequest { number url state isDraft \
    isCrossRepository headRefOid baseRefName mergeStateStatus mergeCommit { oid } \
    commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } \
    reviewThreads(first: 50) { nodes { isResolved comments(last: 1) { nodes { databaseId createdAt } } } } \
    comments(last: 1) { nodes { databaseId } } reviews(last: 1) { nodes { databaseId } } }";

/// A query for `branches` ticket branches, passed as the variables `$h0…`, with the 2 newest PRs
/// of each.
fn graphql(branches: usize) -> String {
    let variables: String = (0..branches).map(|i| format!(", $h{i}: String!")).collect();
    let connections: String = (0..branches)
        .map(|i| {
            format!(
                " t{i}: pullRequests(headRefName: $h{i}, first: 2, \
                 orderBy: {{field: CREATED_AT, direction: DESC}}) {{ nodes {{ ...pr }} }}"
            )
        })
        .collect();
    format!(
        "query($owner: String!, $name: String!{variables}) {{ \
         repository(owner: $owner, name: $name) {{{connections} }} \
         rateLimit {{ remaining resetAt }} }} {PULL_REQUEST_FIELDS}"
    )
}

/// Each ticket's PR from a query's answer, with the remaining rate limit. A ticket's PR is its
/// branch's open PR, or else its newest; PRs from other repositories, such as forks, are skipped.
pub(crate) fn parse<'a>(
    json: &Value,
    tickets: &'a [TicketBranch],
) -> Result<(Vec<(&'a TicketBranch, PullRequest)>, RateLimit)> {
    let data = &json["data"];
    let repository = &data["repository"];
    if !repository.is_object() {
        bail!(
            "GitHub returned no repository: {}",
            json["errors"][0]["message"]
                .as_str()
                .unwrap_or("no reason given")
        );
    }
    let mut found = vec![];
    for (i, ticket) in tickets.iter().enumerate() {
        let nodes = repository[format!("t{i}")]["nodes"]
            .as_array()
            .context("GitHub's answer is missing a branch")?;
        let own: Vec<&Value> = nodes
            .iter()
            .filter(|n| n["isCrossRepository"].as_bool() == Some(false))
            .collect();
        let chosen = own
            .iter()
            .find(|n| n["state"] == "OPEN")
            .or_else(|| own.first());
        if let Some(node) = chosen {
            found.push((ticket, pull_request(node)?));
        }
    }
    let limit = &data["rateLimit"];
    let reset_at = limit["resetAt"]
        .as_str()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .context("GitHub's answer is missing its rate limit")?;
    Ok((
        found,
        RateLimit {
            remaining: limit["remaining"]
                .as_u64()
                .context("GitHub's answer is missing its rate limit")?,
            reset_at: reset_at.timestamp_millis(),
        },
    ))
}

fn pull_request(node: &Value) -> Result<PullRequest> {
    let text = |field: &str| {
        node[field]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("A PR is missing {field}"))
    };
    let state = match node["state"].as_str() {
        Some("OPEN") => PrState::Open,
        Some("CLOSED") => PrState::Closed,
        Some("MERGED") => PrState::Merged,
        other => bail!("A PR has the unknown state {other:?}"),
    };
    let merge_state = match node["mergeStateStatus"].as_str() {
        Some("CLEAN") => MergeState::Clean,
        Some("BEHIND") => MergeState::Behind,
        Some("BLOCKED") => MergeState::Blocked,
        Some("DIRTY") => MergeState::Dirty,
        Some("UNSTABLE") => MergeState::Unstable,
        Some("HAS_HOOKS") => MergeState::HasHooks,
        _ => MergeState::Unknown,
    };
    let checks = match node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str()
    {
        Some("SUCCESS") => CheckState::Success,
        Some("FAILURE" | "ERROR") => CheckState::Failure,
        Some("PENDING" | "EXPECTED") => CheckState::Pending,
        _ => CheckState::None,
    };
    let threads = node["reviewThreads"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    // Each thread's last comment; the newest of them changes on any reply in any thread.
    let inline = threads
        .iter()
        .map(|t| &t["comments"]["nodes"][0])
        .filter_map(|c| Some((c["createdAt"].as_str()?, c["databaseId"].as_u64()?)))
        .max()
        .map(|(_, id)| id);
    let last_id = |kind: &str| node[kind]["nodes"][0]["databaseId"].as_u64();
    Ok(PullRequest {
        number: node["number"].as_u64().context("A PR is missing number")?,
        url: text("url")?,
        state,
        draft: node["isDraft"].as_bool().unwrap_or(false),
        base: text("baseRefName")?,
        head: text("headRefOid")?,
        merge_state,
        checks,
        unresolved_threads: threads.iter().filter(|t| t["isResolved"] == false).count() as u32,
        comments: CommentCursors {
            issue: last_id("comments"),
            review: last_id("reviews"),
            inline,
        },
        merge_commit: if state == PrState::Merged {
            node["mergeCommit"]["oid"].as_str().map(str::to_owned)
        } else {
            None
        },
    })
}

/// When each watched repository's next pass is due, whether one is running, and its failures in
/// a row.
#[derive(Default)]
pub(crate) struct Watch {
    repositories: HashMap<String, Pace>,
}
#[derive(Default)]
struct Pace {
    due_at: i64,
    running: bool,
    failures: u32,
}
impl Watch {
    /// Watches exactly these repositories; one not watched before is due at once.
    pub(crate) fn keep<'a>(&mut self, repositories: impl IntoIterator<Item = &'a str>) {
        let kept: HashSet<&str> = repositories.into_iter().collect();
        self.repositories.retain(|id, _| kept.contains(id.as_str()));
        for id in kept {
            self.repositories.entry(id.to_owned()).or_default();
        }
    }
    /// Marks the repositories due at `at` with no pass running as running, and returns them.
    pub(crate) fn start_due(&mut self, at: i64) -> Vec<String> {
        let mut due = vec![];
        for (id, pace) in &mut self.repositories {
            if !pace.running && pace.due_at <= at {
                pace.running = true;
                due.push(id.clone());
            }
        }
        due
    }
    /// Records a finished pass and returns when the next is due, or `None` for a repository no
    /// longer watched.
    pub(crate) fn finish(&mut self, repository: &str, at: i64, outcome: &Outcome) -> Option<i64> {
        let pace = self.repositories.get_mut(repository)?;
        pace.running = false;
        pace.due_at = match outcome {
            Outcome::Failed(_) => {
                pace.failures += 1;
                let doubling = 1i64 << (pace.failures - 1).min(10);
                at + (IDLE_MS * doubling).min(MAX_BACKOFF_MS)
            }
            Outcome::NotOnGitHub => {
                pace.failures = 0;
                at + IDLE_MS
            }
            Outcome::Read(pass) => {
                pace.failures = 0;
                let active = pass
                    .found
                    .iter()
                    .any(|f| f.pull_request.state == PrState::Open);
                let next = at + if active { ACTIVE_MS } else { IDLE_MS };
                if pass.rate_limit.remaining < RATE_LIMIT_FLOOR {
                    next.max(pass.rate_limit.reset_at)
                } else {
                    next
                }
            }
        };
        Some(pace.due_at)
    }
    /// The earliest due pass of a repository with none running.
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.repositories
            .values()
            .filter(|p| !p.running)
            .map(|p| p.due_at)
            .min()
    }
}

impl Host {
    /// Each repository with an open ticket whose coordinator is not archived, keyed by ID.
    pub(crate) fn pull_request_queries(&self) -> Result<Vec<(String, Query)>> {
        let archived: HashSet<String> = self
            .sessions()?
            .into_iter()
            .filter(|s| s.archived)
            .map(|s| s.id)
            .collect();
        let tickets = self.tickets()?;
        let mut queries = vec![];
        for repository in self.repositories()? {
            let watched: Vec<_> = tickets
                .iter()
                .filter(|t| {
                    t.is_open()
                        && t.repository_id == repository.id
                        && !archived.contains(&t.coordinator_id)
                })
                .map(|t| TicketBranch {
                    ticket_id: t.id.clone(),
                    branch: t.branch.clone(),
                    worktree: t.worktree.clone().into(),
                })
                .collect();
            if !watched.is_empty() {
                queries.push((
                    repository.id,
                    Query {
                        repository: repository.path.into(),
                        base: repository.base,
                        tickets: watched,
                    },
                ));
            }
        }
        Ok(queries)
    }
    /// Records one repository's pass: its check status, and each open ticket's PR when it
    /// changed. A failed pass keeps every recorded PR. Returns whether anything shown changed.
    pub(crate) fn record_pull_requests(
        &mut self,
        repository: &str,
        outcome: &Outcome,
        at: i64,
        next_at: i64,
    ) -> Result<bool> {
        let previous = self.pull_request_checks.get(repository);
        let check = PullRequestCheck {
            repository_id: repository.into(),
            checked_at: match outcome {
                Outcome::Failed(_) => previous.and_then(|c| c.checked_at),
                _ => Some(at),
            },
            error: match outcome {
                Outcome::Failed(error) => Some(error.clone()),
                _ => None,
            },
            watched: !matches!(outcome, Outcome::NotOnGitHub),
            next_at,
        };
        // The check and due times move every pass; only a new status is worth a signal.
        let mut changed =
            previous.is_none_or(|p| p.error != check.error || p.watched != check.watched);
        self.pull_request_checks.insert(repository.into(), check);
        if let Outcome::Read(pass) = outcome {
            for found in &pass.found {
                changed |= self.record_pull_request(found)?;
            }
        }
        Ok(changed)
    }
    fn record_pull_request(&mut self, found: &Found) -> Result<bool> {
        let mut ticket = self.ticket(&found.ticket_id)?;
        if !ticket.is_open() {
            return Ok(false);
        }
        let old = ticket.pull_request.clone();
        let mut new = found.pull_request.clone();
        // GitHub computes the merge state lazily, and merged PRs report it unknown.
        if new.merge_state == MergeState::Unknown
            && let Some(old) = old.as_ref().filter(|old| old.number == new.number)
        {
            new.merge_state = old.merge_state;
        }
        if old.as_ref() == Some(&new) {
            return Ok(false);
        }
        let detail = format!(
            "{} #{}: {}",
            ticket.id,
            new.number,
            PullRequest::changes(old.as_ref(), &new)
        );
        ticket.pull_request = Some(new);
        self.save_ticket(&ticket)?;
        let project = self.session(&ticket.coordinator_id)?.project_id;
        Self::event(&self.db, &project, None, "pull_request", &detail).map(|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "7c1c59eda70e41456e62530280eed9e930a140c8";

    /// A PR node as the query returns it, in PR #31's shape.
    fn node(number: u64, state: &str) -> Value {
        json!({
            "number": number,
            "url": format!("https://github.com/Vyttle-LLC/wiffletree/pull/{number}"),
            "state": state, "isDraft": false, "isCrossRepository": false,
            "headRefOid": HEAD, "baseRefName": "main", "mergeStateStatus": "UNKNOWN",
            "mergeCommit": {"oid": "be5f484628aa0ff990f6f65773ab731eeab93642"},
            "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]},
            "reviewThreads": {"nodes": []}, "comments": {"nodes": []}, "reviews": {"nodes": []}
        })
    }
    fn answer(branches: Vec<Vec<Value>>) -> Value {
        let mut repository = json!({});
        for (i, nodes) in branches.into_iter().enumerate() {
            repository[format!("t{i}")] = json!({ "nodes": nodes });
        }
        json!({"data": {"repository": repository,
            "rateLimit": {"remaining": 4996, "resetAt": "2026-10-09T20:49:16Z"}}})
    }
    fn branches(count: usize) -> Vec<TicketBranch> {
        (0..count)
            .map(|i| TicketBranch {
                ticket_id: format!("t{i}"),
                branch: format!("wiffletree/b{i}"),
                worktree: PathBuf::new(),
            })
            .collect()
    }
    /// The PR parsed for a single branch with these nodes.
    fn chosen(nodes: Vec<Value>) -> Option<PullRequest> {
        let tickets = branches(1);
        let (found, _) = parse(&answer(vec![nodes]), &tickets).unwrap();
        found.into_iter().next().map(|(_, pr)| pr)
    }

    #[test]
    fn pr_31_parses_as_merged_with_its_merge_commit_and_rate_limit() {
        let tickets = branches(2);
        let (found, limit) =
            parse(&answer(vec![vec![node(31, "MERGED")], vec![]]), &tickets).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0.ticket_id, "t0");
        let pr = &found[0].1;
        assert_eq!(
            (pr.number, pr.state, pr.merge_state, pr.checks),
            (
                31,
                PrState::Merged,
                MergeState::Unknown,
                CheckState::Success
            )
        );
        assert_eq!(pr.head, HEAD);
        assert_eq!(pr.base, "main");
        assert_eq!(
            pr.merge_commit.as_deref(),
            Some("be5f484628aa0ff990f6f65773ab731eeab93642")
        );
        assert_eq!(pr.comments, CommentCursors::default());
        assert_eq!(limit.remaining, 4996);
        assert_eq!(limit.reset_at, 1_791_578_956_000);
    }

    #[test]
    fn a_reply_in_an_existing_thread_moves_only_the_inline_cursor() {
        let thread = |resolved: bool, id: u64, at: &str| {
            json!({"isResolved": resolved,
                "comments": {"nodes": [{"databaseId": id, "createdAt": at}]}})
        };
        let mut before = node(142, "OPEN");
        before["comments"] = json!({"nodes": [{"databaseId": 900}]});
        before["reviews"] = json!({"nodes": [{"databaseId": 300}]});
        before["reviewThreads"] = json!({"nodes": [
            thread(true, 600, "2026-10-09T09:00:00Z"),
            thread(false, 400, "2026-10-09T10:00:00Z"),
        ]});
        let mut after = before.clone();
        // A newer reply inside the unresolved thread, with an id below the issue comment's.
        after["reviewThreads"]["nodes"][1] = thread(false, 450, "2026-10-09T11:00:00Z");
        let (before, after) = (chosen(vec![before]).unwrap(), chosen(vec![after]).unwrap());
        assert_eq!(
            before.comments,
            CommentCursors {
                issue: Some(900),
                review: Some(300),
                inline: Some(400)
            }
        );
        assert_eq!(before.unresolved_threads, 1);
        let mut expected = before.clone();
        expected.comments.inline = Some(450);
        assert_eq!(after, expected);
    }

    #[test]
    fn a_fork_pr_with_the_same_branch_name_is_not_the_tickets() {
        let mut fork = node(7, "OPEN");
        fork["isCrossRepository"] = json!(true);
        assert_eq!(chosen(vec![fork]), None);
        assert_eq!(chosen(vec![]), None);
    }

    #[test]
    fn an_open_pr_wins_over_a_newer_closed_one_and_otherwise_the_newest() {
        // Nodes arrive newest first.
        assert_eq!(
            chosen(vec![node(16, "CLOSED"), node(12, "OPEN")])
                .unwrap()
                .number,
            12
        );
        assert_eq!(
            chosen(vec![node(15, "OPEN"), node(12, "CLOSED")])
                .unwrap()
                .number,
            15
        );
        assert_eq!(
            chosen(vec![node(15, "CLOSED"), node(12, "MERGED")])
                .unwrap()
                .number,
            15
        );
    }

    #[test]
    fn a_null_rollup_and_unknown_enum_values_read_as_none_and_unknown() {
        let mut pr = node(1, "OPEN");
        pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = Value::Null;
        pr["mergeStateStatus"] = json!("QUEUED_SOMEHOW");
        let pr = chosen(vec![pr]).unwrap();
        assert_eq!(
            (pr.checks, pr.merge_state),
            (CheckState::None, MergeState::Unknown)
        );
        assert_eq!(pr.merge_commit, None);
        let mut odd = node(1, "OPEN");
        odd["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!("NEW");
        assert_eq!(chosen(vec![odd]).unwrap().checks, CheckState::None);
        for (state, checks) in [
            ("ERROR", CheckState::Failure),
            ("FAILURE", CheckState::Failure),
            ("EXPECTED", CheckState::Pending),
            ("PENDING", CheckState::Pending),
        ] {
            let mut pr = node(1, "OPEN");
            pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!(state);
            assert_eq!(chosen(vec![pr]).unwrap().checks, checks);
        }
    }

    #[test]
    fn an_answer_without_the_repository_fails_with_githubs_reason() {
        let Err(error) = parse(
            &json!({"data": {"repository": null},
                "errors": [{"message": "Could not resolve to a Repository"}]}),
            &branches(1),
        ) else {
            panic!("parsed");
        };
        assert_eq!(
            error.to_string(),
            "GitHub returned no repository: Could not resolve to a Repository"
        );
    }

    #[test]
    fn only_github_com_remotes_name_a_repository() {
        let named = Some(("Vyttle-LLC".to_owned(), "wiffletree".to_owned()));
        for url in [
            "https://github.com/Vyttle-LLC/wiffletree",
            "https://github.com/Vyttle-LLC/wiffletree.git",
            "https://github.com/Vyttle-LLC/wiffletree/",
            "git@github.com:Vyttle-LLC/wiffletree.git",
            "ssh://git@github.com/Vyttle-LLC/wiffletree.git",
        ] {
            assert_eq!(github_repository(url), named, "{url}");
        }
        for url in [
            "https://gitlab.com/Vyttle-LLC/wiffletree.git",
            "git@github.example.com:Vyttle-LLC/wiffletree.git",
            "/Users/me/origin.git",
            "https://github.com/Vyttle-LLC",
            "https://github.com/Vyttle-LLC/wiffle/tree",
            "https://github.com//wiffletree",
            "git@github.com:Vyttle-LLC/$(touch x).git",
        ] {
            assert_eq!(github_repository(url), None, "{url}");
        }
    }

    fn git(path: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(path)
            .args(["-c", "user.name=F", "-c", "user.email=f@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    fn repository(remote: Option<&str>) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        git(folder.path(), &["init", "-q", "-b", "main"]);
        git(
            folder.path(),
            &["commit", "-q", "--allow-empty", "-m", "start"],
        );
        if let Some(url) = remote {
            git(folder.path(), &["remote", "add", "origin", url]);
        }
        folder
    }
    fn query(repository: &Path, base: &str) -> Query {
        Query {
            repository: repository.into(),
            base: base.into(),
            tickets: vec![TicketBranch {
                ticket_id: "t0".into(),
                branch: "wiffletree/pr-watch".into(),
                worktree: repository.into(),
            }],
        }
    }

    #[test]
    fn a_repository_not_on_github_makes_no_gh_call() {
        let no_gh = |_: &Path, _: &[&str]| -> Result<String> { panic!("gh was called") };
        let local = repository(None);
        assert!(matches!(
            fetch_with(&query(local.path(), "main"), no_gh),
            Outcome::NotOnGitHub
        ));
        assert!(matches!(
            fetch_with(&query(local.path(), "origin/main"), no_gh),
            Outcome::NotOnGitHub
        ));
        let elsewhere = repository(Some("https://gitlab.com/o/r.git"));
        assert!(matches!(
            fetch_with(&query(elsewhere.path(), "origin/main"), no_gh),
            Outcome::NotOnGitHub
        ));
    }

    #[test]
    fn a_fake_gh_gets_raw_string_fields_and_its_answer_is_parsed() {
        let folder = repository(Some("git@github.com:Vyttle-LLC/wiffletree.git"));
        let worktree_head = git(folder.path(), &["rev-parse", "HEAD"]);
        let fake_gh = |path: &Path, args: &[&str]| -> Result<String> {
            assert_eq!(path, folder.path());
            assert_eq!(args[..3], ["api", "graphql", "-f"]);
            assert!(
                args[3].starts_with("query=query($owner: String!, $name: String!, $h0: String!)")
            );
            assert_eq!(
                args[4..],
                [
                    "-f",
                    "owner=Vyttle-LLC",
                    "-f",
                    "name=wiffletree",
                    "-f",
                    "h0=wiffletree/pr-watch"
                ]
            );
            Ok(answer(vec![vec![node(31, "MERGED")]]).to_string())
        };
        let Outcome::Read(pass) = fetch_with(&query(folder.path(), "origin/main"), fake_gh) else {
            panic!("not read");
        };
        assert_eq!(pass.found.len(), 1);
        assert_eq!(pass.found[0].pull_request.number, 31);
        assert_eq!(pass.found[0].worktree_head, Some(worktree_head));
        assert_eq!(pass.rate_limit.remaining, 4996);
        let failing =
            |_: &Path, _: &[&str]| -> Result<String> { bail!("gh failed: not logged in") };
        assert!(matches!(
            fetch_with(&query(folder.path(), "origin/main"), failing),
            Outcome::Failed(error) if error == "gh failed: not logged in"
        ));
    }

    #[test]
    fn a_large_repository_is_read_in_calls_of_fifty_branches() {
        let folder = repository(Some("https://github.com/o/r"));
        let mut many = query(folder.path(), "origin/main");
        many.tickets = branches(51);
        let calls = std::cell::RefCell::new(vec![]);
        let fake_gh = |_: &Path, args: &[&str]| -> Result<String> {
            let count = args.iter().filter(|a| a.starts_with('h')).count();
            calls.borrow_mut().push(count);
            Ok(answer(vec![vec![]; count]).to_string())
        };
        assert!(matches!(fetch_with(&many, fake_gh), Outcome::Read(_)));
        assert_eq!(*calls.borrow(), [50, 1]);
    }

    const MINUTE: i64 = 60_000;

    fn pass(open: bool, remaining: u64, reset_at: i64) -> Outcome {
        let mut pr = chosen(vec![node(1, "OPEN")]).unwrap();
        if !open {
            pr.state = PrState::Merged;
        }
        Outcome::Read(Pass {
            found: vec![Found {
                ticket_id: "t".into(),
                pull_request: pr,
                worktree_head: None,
            }],
            rate_limit: RateLimit {
                remaining,
                reset_at,
            },
        })
    }
    fn failed() -> Outcome {
        Outcome::Failed("gh failed".into())
    }

    #[test]
    fn a_new_repository_is_due_at_once_and_has_one_pass_at_a_time() {
        let mut watch = Watch::default();
        watch.keep(["web"]);
        assert_eq!(watch.next_due(), Some(0));
        assert_eq!(watch.start_due(1_000), ["web"]);
        assert!(watch.start_due(1_000 + 10 * MINUTE).is_empty());
        assert_eq!(watch.next_due(), None);
        // Keeping a running repository keeps its pass running.
        watch.keep(["web"]);
        assert!(watch.start_due(1_000 + 10 * MINUTE).is_empty());
    }

    #[test]
    fn an_open_pr_is_checked_every_minute_and_otherwise_every_five() {
        let mut watch = Watch::default();
        watch.keep(["active", "idle", "elsewhere"]);
        watch.start_due(0);
        assert_eq!(
            watch.finish("active", 0, &pass(true, 4000, 0)),
            Some(MINUTE)
        );
        assert_eq!(
            watch.finish("idle", 0, &pass(false, 4000, 0)),
            Some(5 * MINUTE)
        );
        assert_eq!(
            watch.finish("elsewhere", 0, &Outcome::NotOnGitHub),
            Some(5 * MINUTE)
        );
        assert_eq!(watch.next_due(), Some(MINUTE));
        assert_eq!(watch.start_due(MINUTE), ["active"]);
    }

    #[test]
    fn failures_back_off_to_thirty_minutes_and_a_success_resets() {
        let mut watch = Watch::default();
        watch.keep(["web"]);
        let mut at = 0;
        for wait in [5, 10, 20, 30, 30] {
            watch.start_due(at);
            let due = watch.finish("web", at, &failed()).unwrap();
            assert_eq!(due - at, wait * MINUTE);
            at = due;
        }
        watch.start_due(at);
        assert_eq!(
            watch.finish("web", at, &pass(true, 4000, 0)),
            Some(at + MINUTE)
        );
        watch.start_due(at + MINUTE);
        assert_eq!(
            watch.finish("web", at + MINUTE, &failed()),
            Some(at + 6 * MINUTE)
        );
    }

    #[test]
    fn a_nearly_spent_rate_limit_waits_for_the_reset() {
        let mut watch = Watch::default();
        watch.keep(["web"]);
        watch.start_due(0);
        assert_eq!(
            watch.finish("web", 0, &pass(true, 150, 12 * MINUTE)),
            Some(12 * MINUTE)
        );
        watch.start_due(12 * MINUTE);
        // A reset sooner than the interval does not hurry the pass.
        let at = 12 * MINUTE;
        assert_eq!(
            watch.finish("web", at, &pass(true, 150, at + 1)),
            Some(at + MINUTE)
        );
        watch.start_due(at + MINUTE);
        assert_eq!(
            watch.finish("web", at + MINUTE, &pass(true, 200, at + 30 * MINUTE)),
            Some(at + 2 * MINUTE)
        );
    }

    #[test]
    fn a_repository_without_open_tickets_leaves_the_watch() {
        let mut watch = Watch::default();
        watch.keep(["web", "docs"]);
        watch.start_due(0);
        watch.keep(["docs"]);
        // A pass that was running when its repository left reports into nothing.
        assert_eq!(watch.finish("web", 0, &failed()), None);
        assert_eq!(watch.finish("docs", 0, &failed()), Some(5 * MINUTE));
        watch.keep([]);
        assert_eq!(watch.next_due(), None);
    }

    /// A host with ticket "Toolbar" in a repository whose base has no remote.
    fn host_with_ticket() -> (tempfile::TempDir, Host, Ticket) {
        let home = tempfile::tempdir().unwrap();
        let folder = home.path().join("web");
        fs::create_dir_all(&folder).unwrap();
        git(&folder, &["init", "-q", "-b", "main"]);
        git(&folder, &["commit", "-q", "--allow-empty", "-m", "start"]);
        let mut host = Host::open(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        let project = host.create_project("Start").unwrap();
        let coordinator = host.sessions().unwrap().remove(0);
        let repository = host
            .attach_repository(&project.id, folder.to_str().unwrap(), "HEAD")
            .unwrap();
        let ticket = host
            .create_ticket(&coordinator.id, &repository.id, "Toolbar", "Do")
            .unwrap();
        (home, host, ticket)
    }
    fn open_pr(merge_state: MergeState) -> PullRequest {
        let mut pr = chosen(vec![node(142, "OPEN")]).unwrap();
        pr.merge_state = merge_state;
        pr
    }
    fn found(ticket: &Ticket, pull_request: PullRequest) -> Outcome {
        Outcome::Read(Pass {
            found: vec![Found {
                ticket_id: ticket.id.clone(),
                pull_request,
                worktree_head: None,
            }],
            rate_limit: RateLimit {
                remaining: 4000,
                reset_at: 0,
            },
        })
    }
    fn pull_request_events(host: &Host) -> i64 {
        host.db
            .query_row(
                "SELECT COUNT(*) FROM activity WHERE kind='pull_request'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_change_is_recorded_once() {
        let (_home, mut host, ticket) = host_with_ticket();
        let repository = ticket.repository_id.clone();
        let pass = found(&ticket, open_pr(MergeState::Clean));
        assert!(
            host.record_pull_requests(&repository, &pass, 1_000, 61_000)
                .unwrap()
        );
        assert_eq!(
            host.ticket(&ticket.id).unwrap().pull_request,
            Some(open_pr(MergeState::Clean))
        );
        assert_eq!(pull_request_events(&host), 1);
        assert!(
            !host
                .record_pull_requests(&repository, &pass, 61_000, 121_000)
                .unwrap()
        );
        assert_eq!(pull_request_events(&host), 1);
        // The ticket's state is never the watcher's to change.
        assert_eq!(host.ticket(&ticket.id).unwrap().state, ticket.state);
    }

    #[test]
    fn an_unknown_merge_state_keeps_the_recorded_one() {
        let (_home, mut host, ticket) = host_with_ticket();
        let repository = ticket.repository_id.clone();
        host.record_pull_requests(
            &repository,
            &found(&ticket, open_pr(MergeState::Behind)),
            0,
            0,
        )
        .unwrap();
        let unknown = found(&ticket, open_pr(MergeState::Unknown));
        assert!(
            !host
                .record_pull_requests(&repository, &unknown, 1, 1)
                .unwrap()
        );
        let recorded = host.ticket(&ticket.id).unwrap().pull_request.unwrap();
        assert_eq!(recorded.merge_state, MergeState::Behind);
        assert_eq!(pull_request_events(&host), 1);
    }

    #[test]
    fn a_failed_pass_keeps_every_pr_and_shows_why() {
        let (_home, mut host, ticket) = host_with_ticket();
        let repository = ticket.repository_id.clone();
        host.record_pull_requests(
            &repository,
            &found(&ticket, open_pr(MergeState::Clean)),
            1_000,
            61_000,
        )
        .unwrap();
        let failure = Outcome::Failed("gh failed: To get started, run: gh auth login".into());
        assert!(
            host.record_pull_requests(&repository, &failure, 61_000, 361_000)
                .unwrap()
        );
        assert_eq!(
            host.ticket(&ticket.id).unwrap().pull_request,
            Some(open_pr(MergeState::Clean))
        );
        let checks = host.snapshot().unwrap().pull_request_checks;
        assert_eq!(
            checks,
            [PullRequestCheck {
                repository_id: repository.clone(),
                checked_at: Some(1_000),
                error: Some("gh failed: To get started, run: gh auth login".into()),
                watched: true,
                next_at: 361_000,
            }]
        );
        // The same failure again is no news.
        assert!(
            !host
                .record_pull_requests(&repository, &failure, 361_000, 961_000)
                .unwrap()
        );
    }

    #[test]
    fn the_coordinator_reads_the_pr_in_workspace_context() {
        let (_home, mut host, ticket) = host_with_ticket();
        let mut pr = open_pr(MergeState::Behind);
        pr.unresolved_threads = 1;
        host.record_pull_requests(&ticket.repository_id, &found(&ticket, pr), 0, 0)
            .unwrap();
        let context = host.agent_context(&ticket.coordinator_id).unwrap();
        let recorded = &context["tickets"][0]["pull_request"];
        assert_eq!(recorded["number"], 142);
        assert_eq!(recorded["state"], "open");
        assert_eq!(recorded["head"], HEAD);
        assert_eq!(recorded["merge_state"], "behind");
        assert_eq!(recorded["checks"], "success");
        assert_eq!(recorded["unresolved_threads"], 1);
    }

    #[test]
    fn a_closed_ticket_is_not_queried_or_recorded() {
        let (_home, mut host, ticket) = host_with_ticket();
        assert_eq!(host.pull_request_queries().unwrap().len(), 1);
        host.close_ticket(&ticket.id).unwrap();
        assert!(host.pull_request_queries().unwrap().is_empty());
        let pass = found(&ticket, open_pr(MergeState::Clean));
        host.record_pull_requests(&ticket.repository_id, &pass, 0, 0)
            .unwrap();
        assert_eq!(host.ticket(&ticket.id).unwrap().pull_request, None);
    }

    #[test]
    fn three_tickets_in_one_repository_are_one_query() {
        let (_home, mut host, ticket) = host_with_ticket();
        for title in ["Menu", "Footer"] {
            host.create_ticket(&ticket.coordinator_id, &ticket.repository_id, title, "Do")
                .unwrap();
        }
        let queries = host.pull_request_queries().unwrap();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].1.tickets.len(), 3);
        // The fixture's base has no remote, so its row says PRs are not watched.
        let outcome = fetch(&queries[0].1);
        assert!(matches!(outcome, Outcome::NotOnGitHub));
        host.record_pull_requests(&queries[0].0, &outcome, 0, 0)
            .unwrap();
        assert!(!host.snapshot().unwrap().pull_request_checks[0].watched);
    }
}
