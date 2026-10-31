//! An issue thread's `search_repositories` and `read_repository_file` (OpenSpec change
//! `assistant-spaces-jobs-boards`, task 6.3): read-only over its Space's repositories on this
//! Mac, at each one's base branch as last fetched, `origin/<base>`, or the local base branch
//! when it has no fetched one. They use `git grep` and `git show` on that commit, so no
//! worktree, branch or index is read or changed. The host prepares a read on its command
//! thread and the service runs it off that thread; answers are bounded in size.
use crate::*;

/// The most matching lines a search answers, over every repository.
const MAX_MATCHES: usize = 200;
/// The longest matching line kept, in characters.
const MAX_LINE_CHARS: usize = 300;
/// The most of a file `read_repository_file` answers, in bytes.
const MAX_FILE_BYTES: usize = 64 * 1024;
/// The longest search text.
const MAX_QUERY_CHARS: usize = 200;
/// The most matching lines one file gives a search.
const PER_FILE: usize = 20;

/// One repository a read may reach: its name, its folder and the commit it reads.
#[derive(Debug)]
struct Checkout {
    name: String,
    folder: PathBuf,
    commit: String,
}

/// A read prepared on the host's command thread, run with `run` off it.
#[derive(Debug)]
pub(crate) struct RepositoryRead {
    checkouts: Vec<Checkout>,
    request: Request,
}
#[derive(Debug)]
enum Request {
    Search { text: String },
    Read { path: String },
}

impl Host {
    /// What an issue thread's `search_repositories` or `read_repository_file` reads, over its
    /// Space's repositories on this Mac, or the one it names.
    pub(crate) fn repository_read(
        &self,
        session: &Session,
        name: &str,
        args: &Value,
    ) -> Result<RepositoryRead> {
        let thread = self.thread(&session.id)?;
        ensure!(
            thread.attached.is_some() && thread.job.is_none(),
            "{name} is an issue thread's tool"
        );
        let request = match name {
            "search_repositories" => {
                service::only_arguments(name, args, &["query", "repository"])?;
                let text = (args["query"].as_str())
                    .map(str::trim)
                    .filter(|q| !q.is_empty())
                    .context("Give query: the text to search for")?;
                ensure!(
                    text.chars().count() <= MAX_QUERY_CHARS,
                    "Keep query to {MAX_QUERY_CHARS} characters"
                );
                Request::Search { text: text.into() }
            }
            "read_repository_file" => {
                service::only_arguments(name, args, &["repository", "path"])?;
                let path = args["path"]
                    .as_str()
                    .context("Give path: the file to read")?;
                ensure!(
                    !path.is_empty()
                        && !path.starts_with('/')
                        && !path.split('/').any(|part| part == ".." || part.is_empty()),
                    "path is relative to the repository's root"
                );
                ensure!(
                    args["repository"].is_string(),
                    "Give repository: the one the file is in"
                );
                Request::Read { path: path.into() }
            }
            other => bail!("{other} is not a repository read"),
        };
        let listed = self.space_repositories(&thread.space_id)?.repository_ids;
        let named = args["repository"].as_str();
        let mut checkouts = vec![];
        for repository in self.repositories()? {
            if !listed.contains(&repository.id) {
                continue;
            }
            let full = (repository.github.as_ref()).map(|g| format!("{}/{}", g.owner, g.name));
            let matches = |named: &str| {
                repository.name.eq_ignore_ascii_case(named)
                    || full
                        .as_deref()
                        .is_some_and(|f| f.eq_ignore_ascii_case(named))
                    || (repository.github.as_ref())
                        .is_some_and(|g| g.name.eq_ignore_ascii_case(named))
            };
            if named.is_some_and(|named| !matches(named)) {
                continue;
            }
            let folder = PathBuf::from(&repository.path);
            if let Some(commit) = base_commit(&folder, &repository.base) {
                let name = full.unwrap_or_else(|| repository.name.clone());
                checkouts.push(Checkout {
                    name,
                    folder,
                    commit,
                });
            }
        }
        ensure!(
            !checkouts.is_empty(),
            match named {
                Some(named) =>
                    format!("{named} is not one of your Space's repositories on this Mac"),
                None => "Your Space has no repository on this Mac to read".into(),
            }
        );
        Ok(RepositoryRead { checkouts, request })
    }
}

impl RepositoryRead {
    /// Runs the read; slow work, so never on the host's command thread.
    pub(crate) fn run(self) -> Result<Value> {
        match self.request {
            Request::Search { text } => {
                let mut matches = vec![];
                let mut cut = false;
                for checkout in &self.checkouts {
                    let room = MAX_MATCHES - matches.len();
                    if room == 0 {
                        cut = true;
                        break;
                    }
                    let found = grep(checkout, &text, room + 1)?;
                    // A file with as many matches as one file may give may have had more.
                    let full = |path: &Value| {
                        found.iter().filter(|m| &m["path"] == path).count() >= PER_FILE
                    };
                    cut |= found.len() > room || found.iter().any(|m| full(&m["path"]));
                    matches.extend(found.into_iter().take(room));
                }
                Ok(json!({"matches": matches, "cut_short": cut}))
            }
            Request::Read { path } => {
                let checkout = &self.checkouts[0];
                let spec = format!("{}:{path}", checkout.commit);
                let shown =
                    runtime::git_bytes_answer(&checkout.folder, &["show", "--no-textconv", &spec]);
                let bytes = match shown {
                    Ok((status, bytes)) if status.success() => bytes,
                    _ => bail!(
                        "{path} is not a file of {} at its base branch",
                        checkout.name
                    ),
                };
                let kept = &bytes[..bytes.len().min(MAX_FILE_BYTES)];
                let text = String::from_utf8_lossy(kept);
                Ok(json!({
                    "repository": checkout.name,
                    "commit": checkout.commit,
                    "path": path,
                    "text": text,
                    "cut_short": bytes.len() > MAX_FILE_BYTES,
                }))
            }
        }
    }
}

/// The commit a read uses: `origin/<base>` as last fetched, else the local base branch; for a
/// base of `HEAD`, origin's default branch, never the branch checked out here.
fn base_commit(folder: &Path, base: &str) -> Option<String> {
    let references = match base {
        "HEAD" => vec!["refs/remotes/origin/HEAD".to_owned()],
        _ => vec![
            format!("refs/remotes/origin/{base}"),
            format!("refs/heads/{base}"),
        ],
    };
    references.iter().find_map(|reference| {
        let spec = format!("{reference}^{{commit}}");
        let answer = runtime::git_answer(folder, &["rev-parse", "--verify", "--quiet", &spec]);
        let answer = answer.ok().filter(|a| a.status.success())?;
        Some(answer.output.trim().to_owned())
    })
}

/// Up to `limit` lines of `checkout`'s commit that hold `text`, fixed and case-insensitive, at
/// most `PER_FILE` of them from one file.
fn grep(checkout: &Checkout, text: &str, limit: usize) -> Result<Vec<Value>> {
    let per_file = format!("--max-count={PER_FILE}");
    let args = [
        "grep",
        "-I",
        "-n",
        "-i",
        "-F",
        "--no-color",
        &per_file,
        "-e",
        text,
        &checkout.commit,
        "--",
    ];
    let (status, output) = runtime::git_bytes_answer(&checkout.folder, &args)?;
    // 1 is git grep's "nothing matched".
    ensure!(
        status.success() || status.code() == Some(1),
        "Searching {} failed",
        checkout.name
    );
    let prefix = format!("{}:", checkout.commit);
    Ok(String::from_utf8_lossy(&output)
        .lines()
        .take(limit)
        .filter_map(|line| {
            let line = line.strip_prefix(&prefix)?;
            let (path, rest) = line.split_once(':')?;
            let (number, text) = rest.split_once(':')?;
            let text: String = text.trim().chars().take(MAX_LINE_CHARS).collect();
            Some(json!({"repository": checkout.name, "path": path, "line": number.parse::<u64>().ok()?, "text": text}))
        })
        .collect())
}
