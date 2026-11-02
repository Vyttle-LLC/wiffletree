//! Scanning what is pulled (OpenSpec change `one-space-repository`, decision 5). A pass that moved
//! a GitHub repository's head leaves its commits to a job off the pass: each check keeps a mark
//! and scans the next commit after it against its first parent, one job at a time, so a quit
//! resumes and a long history never delays a pull. A finding raises one alert on this Mac for its
//! repository, path, check and rule. An alert closes when a later pull changes its file and the
//! rule no longer fires in the whole file at the head, or when the human marks it Not sensitive,
//! which writes nothing to the repository. A check that cannot run keeps its mark, with one note.
//! Nothing here keeps or shows what a check matched.
use crate::*;
use folder::Root;
use scan::{Finding, Scanner};
use std::collections::BTreeMap;
use workspace_core::{ScanAlert, ScanStopped};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS scan_marks (repository TEXT NOT NULL, check_name TEXT NOT NULL, commit_id TEXT NOT NULL, stopped TEXT, PRIMARY KEY(repository,check_name));
CREATE TABLE IF NOT EXISTS scan_alerts (repository TEXT NOT NULL, path TEXT NOT NULL, check_name TEXT NOT NULL, rule TEXT NOT NULL, line INTEGER, commit_id TEXT NOT NULL, pushed_by TEXT NOT NULL, web_editor INTEGER NOT NULL, blob TEXT NOT NULL, pulled TEXT NOT NULL, not_sensitive TEXT, PRIMARY KEY(repository,path,check_name,rule));
";

/// The tree of a commit with no parent is compared with this one, which Git always knows.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// How far into a file Git looks for a NUL to call it binary. The file's `.gitattributes` could
/// say otherwise, and no file in a repository may change a scan.
const BINARY_PROBE: usize = 8000;
/// The committer of a commit made on GitHub's site.
const GITHUB_COMMITTER: (&str, &str) = ("GitHub", "noreply@github.com");

/// Adds the tables of marks and alerts.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    Ok(())
}

/// A file a commit added or changed, with its blob.
struct Change {
    path: String,
    blob: String,
}

/// The plain files `commit` added or changed against `parent`, or against nothing.
fn changes(clone: &Path, parent: Option<&str>, commit: &str) -> Result<Vec<Change>> {
    let from = parent.unwrap_or(EMPTY_TREE);
    let args = ["diff-tree", "-r", "-z", "--no-renames", from, commit];
    let raw = runtime::git_output(clone, &args)?;
    let mut fields = raw.split('\0');
    let mut changed = vec![];
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let parts: Vec<&str> = meta.split(' ').collect();
        let [_, mode, _, blob, status] = parts[..] else {
            bail!("Git listed a change it did not describe");
        };
        if matches!(mode, "100644" | "100755") && status != "D" {
            changed.push(Change {
                path: path.to_owned(),
                blob: blob.to_owned(),
            });
        }
    }
    Ok(changed)
}

/// Whether Git would call `content` binary.
fn binary(content: &[u8]) -> bool {
    content[..content.len().min(BINARY_PROBE)].contains(&0)
}

/// A commit's first parent, `None` for a first commit.
fn first_parent(clone: &Path, commit: &str) -> Option<String> {
    let parent = format!("{commit}^1");
    let answer = runtime::git_answer(clone, &["rev-parse", "--verify", "--quiet", &parent]).ok()?;
    (answer.status.success()).then(|| answer.output.trim().to_owned())
}

/// The blob `path` holds at `commit`.
fn blob_at(clone: &Path, commit: &str, path: &str) -> Option<String> {
    let at = format!("{commit}:{path}");
    let answer = runtime::git_answer(clone, &["rev-parse", "--verify", "--quiet", &at]).ok()?;
    (answer.status.success()).then(|| answer.output.trim().to_owned())
}

/// Who a commit names, and whether GitHub's site made it.
fn pushed_by(clone: &Path, commit: &str) -> Result<(String, bool)> {
    let format = "--format=%cn%x00%ce%x00%an";
    let shown = runtime::git_output(clone, &["log", "-1", format, commit])?;
    let fields: Vec<&str> = shown.trim_end().split('\0').collect();
    let [committer, email, author] = fields[..] else {
        bail!("Git did not name who made {commit}");
    };
    Ok((author.to_owned(), (committer, email) == GITHUB_COMMITTER))
}

/// An alert a job looks at again, by its path in the repository.
struct Looking {
    path: String,
    check: String,
    rule: String,
    /// The head it was last looked at, or found, on.
    pulled: String,
}

/// One job of a repository's pull scan, off the host thread. It reads the clone's objects only,
/// so it takes no pass lock and no pass waits for it.
pub(crate) struct PullScan {
    key: String,
    clone: PathBuf,
    head: String,
    /// The folder as this Mac records it now.
    root: Root,
    scanner: Scanner,
    /// Each check and its mark.
    marks: Vec<(String, String)>,
    looking: Vec<Looking>,
}

/// A finding in a commit. Its path is relative to the folder, as a job may end on either side of
/// this Mac adopting a move.
struct Found {
    path: String,
    rule: String,
    line: Option<u64>,
    commit: String,
    pushed_by: String,
    web_editor: bool,
    blob: String,
}

/// What a job did with one check.
struct Step {
    check: String,
    /// Where its mark goes.
    to: Option<String>,
    /// Why it could not run.
    stopped: Option<String>,
    found: Vec<Found>,
}

/// What became of an alert a job looked at, by its path relative to the folder.
struct Looked {
    path: String,
    check: String,
    rule: String,
    outcome: Outcome,
}

enum Outcome {
    /// Its file is as it was, or the rule still fires, on this line.
    Open(Option<u64>),
    Closed,
}

/// What a job finished.
pub(crate) struct Scanned {
    key: String,
    head: String,
    steps: Vec<Step>,
    looked: Vec<Looked>,
}

impl PullScan {
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
    pub(crate) fn run(&self) -> Scanned {
        let mut steps: Vec<Step> = (self.marks.iter())
            .map(|(check, mark)| self.step(check, mark))
            .collect();
        let mut looked = vec![];
        if !self.looking.is_empty() && self.root_at(&self.head) == self.root {
            for alert in &self.looking {
                match self.look(alert) {
                    Ok(outcome) => looked.push(Looked {
                        path: self.relative(&self.root, &alert.path),
                        check: alert.check.clone(),
                        rule: alert.rule.clone(),
                        outcome,
                    }),
                    Err(reason) => {
                        let step = steps.iter_mut().find(|step| step.check == alert.check);
                        if let Some(step) = step {
                            step.stopped = Some(reason);
                        }
                    }
                }
            }
        }
        Scanned {
            key: self.key.clone(),
            head: self.head.clone(),
            steps,
            looked,
        }
    }
    fn step(&self, check: &str, mark: &str) -> Step {
        let mut found = vec![];
        let (to, stopped) = match self.advance(check, mark, &mut found) {
            Ok(to) => (to, None),
            Err(reason) => (None, Some(reason)),
        };
        Step {
            check: check.to_owned(),
            to,
            stopped,
            found,
        }
    }
    /// Scans the next commit on the head's first-parent line after `mark`, with any commits a
    /// merge brought, oldest first: where the mark goes, `None` when it is at the head. Walking
    /// that line keeps the mark an ancestor of what is left, so it always moves forward. A head
    /// behind the mark, such as a kept clone's before its first fetch, has nothing new yet.
    fn advance(
        &self,
        check: &str,
        mark: &str,
        found: &mut Vec<Found>,
    ) -> std::result::Result<Option<String>, String> {
        let failed = |error: anyhow::Error| format!("{error:#}");
        let clone = self.clone.as_path();
        if publish::is_ancestor(clone, &self.head, mark) {
            return Ok(None);
        }
        let start = self.start(mark);
        let range = format!("{start}..{}", self.head);
        let walk = ["rev-list", "--first-parent", "--reverse", &range];
        let line = runtime::git_output(clone, &walk).map_err(failed)?;
        let Some(next) = line.lines().next() else {
            return Ok((mark != self.head).then(|| self.head.clone()));
        };
        let commits = match first_parent(clone, next) {
            Some(parent) => {
                let brought = format!("{parent}..{next}");
                let list = ["rev-list", "--reverse", "--topo-order", &brought];
                runtime::git_output(clone, &list).map_err(failed)?
            }
            None => next.to_owned(),
        };
        for commit in commits.lines() {
            found.extend(self.scan_commit(check, commit)?);
        }
        Ok(Some(next.to_owned()))
    }
    /// Where a mark's check starts: the mark, or the merge base when a rewrite left it behind.
    fn start(&self, mark: &str) -> String {
        if publish::is_ancestor(&self.clone, mark, &self.head) {
            return mark.to_owned();
        }
        let base = runtime::git_output(&self.clone, &["merge-base", mark, &self.head]);
        base.map_or_else(|_| self.head.clone(), |base| base.trim().to_owned())
    }
    /// The folder `commit` holds, as a commit from before a move has its files at the root.
    fn root_at(&self, commit: &str) -> Root {
        (folder::find(&self.clone, commit).ok().flatten()).unwrap_or_else(|| self.root.clone())
    }
    fn relative(&self, root: &Root, path: &str) -> String {
        root.strip(path).unwrap_or(path).to_owned()
    }
    /// The findings of `check` in `commit`, against its first parent, over the text files it
    /// changed under the scanned paths. A move's commit carries files it does not change, and
    /// its stubs are outside the folder.
    fn scan_commit(&self, check: &str, commit: &str) -> std::result::Result<Vec<Found>, String> {
        let failed = |error: anyhow::Error| format!("{error:#}");
        let clone = self.clone.as_path();
        let parent = first_parent(clone, commit);
        let root = self.root_at(commit);
        let mut changed = changes(clone, parent.as_deref(), commit).map_err(failed)?;
        changed.retain(|change| root.scanned(&change.path));
        if changed.is_empty() {
            return Ok(vec![]);
        }
        if let Some(parent) = parent.as_deref().filter(|p| self.root_at(p) != root) {
            let before = folder::tree(clone, parent).map_err(failed)?;
            changed.retain(|change| !folder::carried(&before, &root, &change.path, &change.blob));
        }
        let wanted = |path: &str| changed.iter().any(|change| change.path == path);
        let contents = folder::plain_files(clone, commit, &wanted).map_err(failed)?;
        let files: Vec<(String, Vec<u8>)> = (contents.into_iter())
            .filter(|(_, content)| !binary(content))
            .collect();
        if files.is_empty() {
            return Ok(vec![]);
        }
        let findings = self
            .scanner
            .check(clone, parent.as_deref(), &files, check)?;
        let (who, web_editor) = pushed_by(clone, commit).map_err(failed)?;
        let mut found: Vec<Found> = vec![];
        for (path, rule, line) in findings {
            let path_relative = self.relative(&root, &path);
            let repeated = |f: &Found| f.path == path_relative && f.rule == rule;
            if found.iter().any(repeated) {
                continue;
            }
            let blob = (changed.iter().find(|change| change.path == path))
                .map(|change| change.blob.clone())
                .unwrap_or_default();
            found.push(Found {
                path: path_relative,
                rule,
                line,
                commit: commit.to_owned(),
                pushed_by: who.clone(),
                web_editor,
                blob,
            });
        }
        Ok(found)
    }
    /// Whether an alert stays open at the head: a file that a pull since it was last looked at
    /// did not change does, and otherwise the file is scanned whole.
    fn look(&self, alert: &Looking) -> std::result::Result<Outcome, String> {
        let clone = self.clone.as_path();
        let then = (blob_at(clone, &alert.pulled, &alert.path)).or_else(|| {
            let old = self.root.moved_from(&alert.path)?;
            blob_at(clone, &alert.pulled, old)
        });
        let now = blob_at(clone, &self.head, &alert.path);
        if now.is_some() && now == then {
            return Ok(Outcome::Open(None));
        }
        let wanted = |path: &str| path == alert.path;
        let contents = folder::plain_files(clone, &self.head, &wanted);
        let contents = contents.map_err(|error| format!("{error:#}"))?;
        let Some((path, content)) = contents.into_iter().next() else {
            return Ok(Outcome::Closed);
        };
        if binary(&content) {
            return Ok(Outcome::Closed);
        }
        let findings = (self.scanner).check(clone, None, &[(path, content)], &alert.check)?;
        let fires = |finding: &&Finding| finding.1 == alert.rule;
        Ok(match findings.iter().find(fires) {
            Some(finding) => Outcome::Open(finding.2),
            None => Outcome::Closed,
        })
    }
}

impl Host {
    /// Starts the repository `key`'s marks at `head`, which is what makes a joining Mac, and every
    /// repository at upgrade, scan only what comes next.
    pub(crate) fn start_scan_marks(&self, key: &str, head: &str) -> Result<()> {
        for (check, _) in scan::CHECKS {
            self.db.execute(
                "INSERT OR IGNORE INTO scan_marks(repository,check_name,commit_id) VALUES (?1,?2,?3)",
                params![key, check, head],
            )?;
        }
        Ok(())
    }
    /// Starts the marks of a repository this Mac joins with no Space there yet at `head`, the
    /// head it joined at, as it scanned nothing while it was away: a clone kept from an earlier
    /// Space may have last synced long before.
    pub(crate) fn restart_scan_marks(&self, key: &str, head: Option<&str>) -> Result<()> {
        let stored = (self.spaces()?.iter())
            .any(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key));
        if !stored {
            self.db
                .execute("DELETE FROM scan_marks WHERE repository=?1", [key])?;
            if let Some(head) = head {
                self.start_scan_marks(key, head)?;
            }
        }
        Ok(())
    }
    /// The next job of the repository `key`'s pull scan, `None` with nothing left to do.
    pub(crate) fn pull_scan(&self, key: &str) -> Result<Option<PullScan>> {
        let Some(head) = self.brain_commit(key)? else {
            return Ok(None);
        };
        let spaces = self.spaces()?;
        let Some(record) = (spaces.iter())
            .find(|r| Self::storage_key(&r.space.storage.repository).as_deref() == Some(key))
        else {
            return Ok(None);
        };
        let marks: Vec<(String, String)> = (self.db)
            .prepare("SELECT check_name,commit_id FROM scan_marks WHERE repository=?1")?
            .query_map([key], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let at_head = |check: &str| marks.iter().any(|(c, mark)| c == check && *mark == head);
        let looking: Vec<Looking> = (self.db.prepare(
            "SELECT path,check_name,rule,pulled FROM scan_alerts WHERE repository=?1 AND not_sensitive IS NULL AND pulled<>?2",
        )?)
        .query_map(params![key, head], |r| {
            Ok(Looking {
                path: r.get(0)?,
                check: r.get(1)?,
                rule: r.get(2)?,
                pulled: r.get(3)?,
            })
        })?
        .filter(|alert| alert.as_ref().map_or(true, |a| at_head(&a.check)))
        .collect::<rusqlite::Result<_>>()?;
        if looking.is_empty() && marks.iter().all(|(_, mark)| *mark == head) {
            return Ok(None);
        }
        let root = Root::new(&record.space.storage.root);
        let scanner = Scanner::bundled(&self.home, self.scan_path.clone())?;
        Ok(Some(PullScan {
            key: key.to_owned(),
            clone: repos::clone_folder(&self.home, &self.stored_repository(key)?.name),
            head,
            scanner: scanner.in_folder(root.clone(), self.scan_domains(key)),
            root,
            marks,
            looking,
        }))
    }
    /// Takes what a job found. `(more, changed)`: whether a mark or an alert moved, so the next
    /// job follows at once, and whether what the human sees did.
    pub(crate) fn finish_pull_scan(&mut self, scanned: Scanned) -> Result<(bool, bool)> {
        let Scanned {
            key,
            head,
            steps,
            looked,
        } = scanned;
        let root = self.root_of_key(&key)?;
        let more = !looked.is_empty() || steps.iter().any(|step| step.to.is_some());
        let mut changed = false;
        let tx = self.db.unchecked_transaction()?;
        for step in &steps {
            changed |= tx.execute(
                "UPDATE scan_marks SET stopped=?3 WHERE repository=?1 AND check_name=?2 AND stopped IS NOT ?3",
                params![key, step.check, step.stopped],
            )? > 0;
            if let Some(to) = &step.to {
                tx.execute(
                    "UPDATE scan_marks SET commit_id=?3 WHERE repository=?1 AND check_name=?2",
                    params![key, step.check, to],
                )?;
            }
            for found in &step.found {
                let path = root.join(&found.path);
                // Marked Not sensitive for this very content: never again here.
                let marked: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM scan_alerts WHERE repository=?1 AND path=?2 AND check_name=?3 AND rule=?4 AND not_sensitive IS NOT NULL AND blob=?5)",
                    params![key, path, step.check, found.rule, found.blob],
                    |r| r.get(0),
                )?;
                if marked {
                    continue;
                }
                tx.execute(
                    "INSERT OR REPLACE INTO scan_alerts(repository,path,check_name,rule,line,commit_id,pushed_by,web_editor,blob,pulled) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![key, path, step.check, found.rule, found.line, found.commit, found.pushed_by, found.web_editor, found.blob, head],
                )?;
                changed = true;
            }
        }
        for alert in &looked {
            let path = root.join(&alert.path);
            let row = params![key, path, alert.check, alert.rule];
            match alert.outcome {
                Outcome::Open(line) => {
                    tx.execute(
                        "UPDATE scan_alerts SET pulled=?5,line=COALESCE(?6,line) WHERE repository=?1 AND path=?2 AND check_name=?3 AND rule=?4",
                        params![key, path, alert.check, alert.rule, head, line],
                    )?;
                }
                Outcome::Closed => {
                    changed |= tx.execute(
                        "DELETE FROM scan_alerts WHERE repository=?1 AND path=?2 AND check_name=?3 AND rule=?4 AND not_sensitive IS NULL",
                        row,
                    )? > 0;
                }
            }
        }
        tx.commit()?;
        Ok((more, changed))
    }
    /// Not sensitive: closes the alert on this Mac, keeping its row so the same path, rule and
    /// content never alert here again. Nothing reaches the repository.
    pub fn mark_alert_not_sensitive(
        &mut self,
        repository: &str,
        path: &str,
        check: &str,
        rule: &str,
        reason: &str,
    ) -> Result<()> {
        let reason = memory::one_line(reason);
        ensure!(
            !reason.is_empty() && reason.chars().count() <= brain_sync::MAX_REASON_CHARS,
            "Give a reason of at most {} characters",
            brain_sync::MAX_REASON_CHARS
        );
        let marked = self.db.execute(
            "UPDATE scan_alerts SET not_sensitive=?5 WHERE repository=?1 AND path=?2 AND check_name=?3 AND rule=?4 AND not_sensitive IS NULL",
            params![repos::key(repository), path, check, rule, reason],
        )?;
        ensure!(marked == 1, "That alert is not open on this Mac");
        Ok(())
    }
    /// This Mac's open alerts, each with the projects of the Spaces using its folder.
    pub(crate) fn scan_alerts(&self) -> Result<Vec<ScanAlert>> {
        let repositories = self.scanned_repositories()?;
        let alerts: Vec<ScanAlert> = (self.db.prepare(
            "SELECT repository,path,check_name,rule,line,commit_id,pushed_by,web_editor FROM scan_alerts WHERE not_sensitive IS NULL ORDER BY repository,path,check_name,rule",
        )?)
        .query_map([], |r| {
            Ok(ScanAlert {
                repository: r.get(0)?,
                path: r.get(1)?,
                check: r.get(2)?,
                rule: r.get(3)?,
                line: r.get(4)?,
                commit: r.get(5)?,
                pushed_by: r.get(6)?,
                web_editor: r.get(7)?,
                projects: vec![],
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
        Ok((alerts.into_iter())
            .filter_map(|alert| {
                let (name, projects) = repositories.get(&alert.repository)?;
                Some(ScanAlert {
                    repository: name.clone(),
                    projects: projects.clone(),
                    ..alert
                })
            })
            .collect())
    }
    /// The GitHub repositories of this Mac's Spaces, by key: each one's name and its projects.
    fn scanned_repositories(&self) -> Result<BTreeMap<String, (String, Vec<String>)>> {
        let projects = self.projects()?;
        let mut repositories: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
        for record in self.spaces()? {
            let repository = &record.space.storage.repository;
            let Some(key) = Self::storage_key(repository) else {
                continue;
            };
            let (_, listed) = repositories
                .entry(key)
                .or_insert_with(|| (repository.key(), vec![]));
            let ids = projects.iter().filter(|p| p.space == record.space.id);
            listed.extend(ids.map(|project| project.id.clone()));
        }
        Ok(repositories)
    }
    /// The alert cards, then one note per check that cannot run and why, naming its repositories.
    pub(crate) fn alert_notes(&self) -> Result<Vec<WorkspaceNote>> {
        let mut notes: Vec<WorkspaceNote> = (self.scan_alerts()?.into_iter())
            .map(WorkspaceNote::Alert)
            .collect();
        let repositories = self.scanned_repositories()?;
        let mut stopped: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        let rows: Vec<(String, String, String)> = (self.db.prepare(
            "SELECT check_name,stopped,repository FROM scan_marks WHERE stopped IS NOT NULL ORDER BY repository",
        )?)
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
        for (check, reason, key) in rows {
            if let Some((name, _)) = repositories.get(&key) {
                stopped
                    .entry((check, reason))
                    .or_default()
                    .push(name.clone());
            }
        }
        notes.extend(stopped.into_iter().map(|((check, reason), repositories)| {
            let looked_for = match check == workspace_core::SENSITIVE_INFO {
                true => "sensitive information",
                false => "secrets",
            };
            let text = format!(
                "What is pulled into {} is not being scanned for {looked_for}: {check} could not run: {reason}. It catches up by itself when it can run.",
                repositories.join(", "),
            );
            WorkspaceNote::ScanStopped(ScanStopped {
                check,
                repositories,
                text,
            })
        }));
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tests::{host, issue, join_sage, press_move, sage, shared_storage, sync};
    use crate::sync::tests::{GitHub, NAME};
    use crate::test_support::{git, scan_tools};
    use std::sync::Arc;
    use workspace_core::{SENSITIVE_INFO, ScanAlert};

    /// Invented values only.
    const PHONE: &str = "Call the parent at (415) 555-0134 tonight.\n";
    const OTHER_PHONE: &str = "Or at (415) 555-0199.\n";
    const CLEAN: &str = "Nothing to see here.\n";
    const NOTE: &str = "second-brain/notes/Retries.md";
    const SECRET_SCAN: &str = "secret-scan";

    /// One Mac that joined Sage on the fake GitHub, with bundled checks that run.
    struct Mac {
        home: PathBuf,
        _home: tempfile::TempDir,
        host: Host,
        bin: tempfile::TempDir,
    }
    impl Mac {
        fn join(github: &Arc<GitHub>) -> Self {
            Self::join_with(github, scan_tools(true))
        }
        fn join_with(github: &Arc<GitHub>, bin: tempfile::TempDir) -> Self {
            let (home, mut host) = host();
            host.record_gh_login(Some("mhohlios".into()));
            host.scan_path = Some(bin.path().into());
            join_sage(&mut host, github).unwrap();
            let mut mac = Self {
                home: home.path().to_owned(),
                _home: home,
                host,
                bin,
            };
            mac.pass(github);
            mac
        }
        fn pass(&mut self, github: &Arc<GitHub>) {
            sync(&mut self.host, github);
        }
        /// One pull scan job, as the service runs them; `false` with nothing done.
        fn step(&mut self) -> bool {
            let key = repos::key(NAME);
            let Some(job) = self.host.pull_scan(&key).unwrap() else {
                return false;
            };
            self.host.finish_pull_scan(job.run()).unwrap().0
        }
        fn scan(&mut self) {
            while self.step() {}
        }
        fn pull(&mut self, github: &Arc<GitHub>) {
            self.pass(github);
            self.scan();
        }
        fn alerts(&self) -> Vec<ScanAlert> {
            self.host.scan_alerts().unwrap()
        }
        fn stopped(&self) -> Vec<ScanStopped> {
            let notes = self.host.alert_notes().unwrap().into_iter();
            notes
                .filter_map(|note| match note {
                    WorkspaceNote::ScanStopped(stopped) => Some(stopped),
                    _ => None,
                })
                .collect()
        }
        fn mark(&self, check: &str) -> String {
            self.host
                .db
                .query_row(
                    "SELECT commit_id FROM scan_marks WHERE repository=?1 AND check_name=?2",
                    params![repos::key(NAME), check],
                    |r| r.get(0),
                )
                .unwrap()
        }
        fn not_sensitive(&mut self, alert: &ScanAlert, reason: &str) -> Result<()> {
            self.host
                .mark_alert_not_sensitive(NAME, &alert.path, &alert.check, &alert.rule, reason)
        }
        /// Quits and starts again, on the same store.
        fn restart(&mut self) {
            let elsewhere = tempfile::tempdir().unwrap();
            let another = Host::open_set_up(elsewhere.path()).unwrap();
            drop(std::mem::replace(&mut self.host, another));
            self.host = Host::open_set_up(&self.home).unwrap();
            self.host.scan_path = Some(self.bin.path().into());
        }
    }

    /// A repository on the fake GitHub with Sage in `wiffletree/`.
    fn moved_repository(folder: &Path) -> Arc<GitHub> {
        let github = Arc::new(GitHub::new(folder, true));
        github.commit("wiffletree/spaces/sage/space.yaml", sage("Sage"));
        github.commit(
            "wiffletree/wiffletree-issues.yaml",
            "format: 1\nprefix: SAGE\n",
        );
        github
    }

    fn in_folder(path: &str) -> String {
        format!("wiffletree/{path}")
    }

    fn head(github: &GitHub) -> String {
        git(&github.bare, &["rev-parse", "main"]).trim().to_owned()
    }

    fn fixture() -> (tempfile::TempDir, Arc<GitHub>) {
        let folder = tempfile::tempdir().unwrap();
        let github = moved_repository(folder.path());
        (folder, github)
    }

    #[test]
    fn an_edit_on_githubs_site_raises_one_card_naming_the_rule_and_never_the_text() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let path = in_folder("issues/SAGE-1/issue.md");
        let text = format!("{}{PHONE}", String::from_utf8(issue("SAGE-1")).unwrap());
        let line = text.lines().count() as u64;
        let work = tempfile::tempdir().unwrap();
        git(
            work.path(),
            &["clone", "-q", github.bare.to_str().unwrap(), "."],
        );
        let file = work.path().join(&path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        git(work.path(), &["add", "--all"]);
        let identity = [
            "-c",
            "user.name=GitHub",
            "-c",
            "user.email=noreply@github.com",
        ];
        let commit = [
            "commit",
            "-q",
            "--author=Jo Lin <jo@example.invalid>",
            "-m",
            "Web",
        ];
        git(work.path(), &[&identity[..], &commit[..]].concat());
        git(work.path(), &["push", "-q", "origin", "HEAD:main"]);
        mac.pull(&github);
        let alerts = mac.alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        let alert = &alerts[0];
        assert_eq!(
            (alert.path.as_str(), alert.rule.as_str()),
            (path.as_str(), "phone")
        );
        assert_eq!(alert.check, SENSITIVE_INFO);
        assert_eq!(alert.line, Some(line));
        assert!(alert.web_editor);
        let card = alert.found();
        assert!(card.contains("pushed from GitHub's web editor"), "{card}");
        assert!(!card.contains("555") && !card.contains("415"), "{card}");
        let shown = serde_json::to_string(&mac.host.alert_notes().unwrap()).unwrap();
        assert!(!shown.contains("555"), "{shown}");
        mac.pull(&github);
        assert_eq!(mac.alerts().len(), 1, "a second pass raises nothing");
    }

    #[test]
    fn a_commit_from_a_teammate_names_its_author() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        let alert = mac.alerts().remove(0);
        assert!(!alert.web_editor);
        assert_eq!(alert.pushed(), format!("pushed by {}", alert.pushed_by));
    }

    #[test]
    fn text_added_and_removed_between_two_pulls_still_alerts_until_a_later_pull_changes_the_file() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        github.commit(&in_folder(NOTE), PHONE);
        github.commit(&in_folder(NOTE), CLEAN);
        mac.pull(&github);
        assert_eq!(mac.alerts().len(), 1, "it reached GitHub");
        github.commit("README.md", "Unrelated.\n");
        mac.pull(&github);
        assert_eq!(mac.alerts().len(), 1, "that pull left the file alone");
        github.commit(&in_folder(NOTE), "Changed again.\n");
        mac.pull(&github);
        assert_eq!(mac.alerts(), []);
    }

    #[test]
    fn a_file_outside_the_folder_or_hidden_or_binary_raises_nothing() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        github.commit("README.md", PHONE);
        github.commit("wiffletree/other/notes.md", PHONE);
        github.commit(&in_folder("spaces/sage/.hidden.md"), PHONE);
        github.commit(&in_folder("spaces/sage/board.bin"), format!("\0{PHONE}"));
        mac.pull(&github);
        assert_eq!(mac.alerts(), []);
        assert_eq!(mac.stopped(), []);
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
    }

    /// Config files a teammate pushes, under `root`, then a phone number at `path`.
    fn silenced_by_nothing(github: &Arc<GitHub>, mac: &mut Mac, path: &str) {
        let allows_all = "[allowlist]\npaths = ['''.*''']\n";
        github.commit(".gitleaks.toml", allows_all);
        github.commit(".gitleaksignore", "everything\n");
        github.commit(".gitattributes", "* binary\n");
        github.commit(
            "brain-gate-allowlist.toml",
            "[[path]]\nglob = \"*\"\nreason = \"all\"\n",
        );
        github.commit(
            "wiffletree-brain.json",
            r#"{"gate": {"allowlist": "brain-gate-allowlist.toml"}}"#,
        );
        github.commit(path, PHONE);
        mac.pull(github);
        let alerts = mac.alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].path, path);
    }

    #[test]
    fn a_pushed_allow_list_silences_nothing_in_a_moved_repository() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        silenced_by_nothing(&github, &mut mac, &in_folder(NOTE));
    }

    #[test]
    fn a_pushed_allow_list_silences_nothing_in_a_repository_with_an_empty_root() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let mut mac = Mac::join(&github);
        silenced_by_nothing(&github, &mut mac, NOTE);
    }

    #[test]
    fn three_commits_raise_one_card_with_the_latest_line() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let mut text = String::new();
        for _ in 0..3 {
            text.push_str(CLEAN);
            text.push_str(PHONE);
            github.commit(&in_folder(NOTE), &text);
        }
        mac.pull(&github);
        let alerts = mac.alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].line, Some(6));
    }

    #[test]
    fn each_mac_raises_its_own_and_removal_closes_each_at_its_own_pull() {
        let (_folder, github) = fixture();
        let (mut mine, mut theirs) = (Mac::join(&github), Mac::join(&github));
        github.commit(&in_folder(NOTE), PHONE);
        mine.pull(&github);
        assert_eq!(mine.alerts().len(), 1);
        assert_eq!(theirs.alerts(), [], "that Mac has not pulled");
        theirs.pull(&github);
        assert_eq!(theirs.alerts().len(), 1);
        github.commit(&in_folder(NOTE), CLEAN);
        mine.pull(&github);
        assert_eq!(mine.alerts(), []);
        assert_eq!(theirs.alerts().len(), 1, "closed at its own pull");
        theirs.pull(&github);
        assert_eq!(theirs.alerts(), []);
    }

    #[test]
    fn a_file_deleted_on_github_closes_its_alert() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        let work = tempfile::tempdir().unwrap();
        git(
            work.path(),
            &["clone", "-q", github.bare.to_str().unwrap(), "."],
        );
        git(work.path(), &["rm", "-q", &in_folder(NOTE)]);
        git(work.path(), &["commit", "-q", "-m", "Removed"]);
        git(work.path(), &["push", "-q", "origin", "HEAD:main"]);
        mac.pull(&github);
        assert_eq!(mac.alerts(), []);
    }

    #[test]
    fn not_sensitive_closes_here_only_writes_nothing_and_a_new_finding_raises_anew() {
        let (_folder, github) = fixture();
        let (mut mine, mut theirs) = (Mac::join(&github), Mac::join(&github));
        let path = in_folder(NOTE);
        github.commit(&path, PHONE);
        mine.pull(&github);
        theirs.pull(&github);
        let alert = mine.alerts().remove(0);
        assert!(
            mine.not_sensitive(&alert, "  ").is_err(),
            "a reason is needed"
        );
        let commits = github.log("%H").len();
        mine.not_sensitive(&alert, "Our shop's public number")
            .unwrap();
        assert!(mine.not_sensitive(&alert, "Again").is_err());
        assert_eq!(mine.alerts(), []);
        assert_eq!(theirs.alerts().len(), 1, "a teammate keeps their own card");
        mine.pull(&github);
        assert_eq!(github.log("%H").len(), commits, "nothing reached GitHub");
        // An unrelated edit of the file adds no line that fires.
        github.commit(&path, format!("{CLEAN}{PHONE}"));
        mine.pull(&github);
        assert_eq!(mine.alerts(), []);
        // The very file again, after it changed and came back.
        github.commit(&path, format!("{CLEAN}{OTHER_PHONE}{PHONE}"));
        mine.pull(&github);
        let raised = mine.alerts();
        assert_eq!(raised.len(), 1, "a new finding raises anew");
        assert_eq!(raised[0].line, Some(2));
    }

    #[test]
    fn marked_content_seen_again_after_a_rewrite_raises_nothing() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let before = head(&github);
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        let alert = mac.alerts().remove(0);
        mac.not_sensitive(&alert, "A fixture").unwrap();
        // The same file in a rewritten commit.
        git(&github.bare, &["update-ref", "refs/heads/main", &before]);
        github.commit("README.md", "Another start.\n");
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        assert_eq!(mac.alerts(), []);
    }

    #[test]
    fn an_open_alert_follows_its_file_through_the_move_on_every_mac() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        let (mut mover, mut adopter, mut late) =
            (Mac::join(&github), Mac::join(&github), Mac::join(&github));
        github.commit(NOTE, PHONE);
        mover.pull(&github);
        adopter.pull(&github);
        press_move(&mut mover.host, &github);
        let moved = in_folder(NOTE);
        let at = |mac: &Mac| {
            mac.alerts()
                .iter()
                .map(|a| a.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(at(&mover), std::slice::from_ref(&moved));
        adopter.pull(&github);
        assert_eq!(
            at(&adopter),
            std::slice::from_ref(&moved),
            "adopted with the record"
        );
        // A Mac that pulls the finding and the move together.
        late.pull(&github);
        late.pull(&github);
        assert_eq!(at(&late), std::slice::from_ref(&moved));
        mover.pull(&github);
        adopter.pull(&github);
        for mac in [&mover, &adopter, &late] {
            assert_eq!(
                at(mac),
                std::slice::from_ref(&moved),
                "the move's commit is no change"
            );
        }
        github.commit("README.md", "Unrelated.\n");
        mover.pull(&github);
        assert_eq!(
            at(&mover),
            std::slice::from_ref(&moved),
            "the rule still fires"
        );
        github.commit(&moved, CLEAN);
        for mac in [&mut mover, &mut adopter, &mut late] {
            mac.pull(&github);
            assert_eq!(mac.alerts(), [], "closed once the text is gone from main");
        }
    }

    #[test]
    fn the_moves_commit_raises_nothing_though_it_adds_every_file() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("SAGE"));
        github.commit(NOTE, PHONE);
        let mut mover = Mac::join(&github);
        let mut teammate = Mac::join(&github);
        press_move(&mut mover.host, &github);
        mover.pull(&github);
        teammate.pull(&github);
        teammate.pull(&github);
        assert_eq!(mover.mark(SENSITIVE_INFO), head(&github));
        assert_eq!(teammate.mark(SENSITIVE_INFO), head(&github));
        assert_eq!(mover.alerts(), []);
        assert_eq!(teammate.alerts(), []);
    }

    #[test]
    fn a_quit_midway_resumes_from_its_mark_and_a_joining_mac_scans_no_old_history() {
        let (_folder, github) = fixture();
        github.commit(&in_folder(NOTE), PHONE);
        let mut mac = Mac::join(&github);
        assert_eq!(
            mac.mark(SENSITIVE_INFO),
            head(&github),
            "marks start at the head"
        );
        mac.scan();
        assert_eq!(mac.alerts(), [], "no old file alerts");
        let first = in_folder("second-brain/notes/First.md");
        github.commit(&first, PHONE);
        let scanned = head(&github);
        github.commit(&in_folder("second-brain/notes/Second.md"), PHONE);
        mac.pass(&github);
        assert!(mac.step());
        assert_eq!(mac.mark(SENSITIVE_INFO), scanned, "one commit at a time");
        assert_eq!(mac.alerts().len(), 1);
        mac.restart();
        mac.scan();
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
        assert_eq!(mac.alerts().len(), 2, "each commit was scanned once");
    }

    /// A Mac removes Sage, a teammate pushes a phone number, and the Mac comes back with `back`.
    fn back_after_a_removed_space(back: fn(&mut Host, &Arc<GitHub>)) {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let sage = crate::storage::tests::space_named(&mac.host, "Sage");
        mac.host.remove_space(&sage.id).unwrap();
        github.commit(&in_folder(NOTE), PHONE);
        back(&mut mac.host, &github);
        mac.pull(&github);
        assert_eq!(mac.alerts(), [], "what it missed is old history");
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
        github.commit(&in_folder("second-brain/notes/New.md"), PHONE);
        mac.pull(&github);
        assert_eq!(mac.alerts().len(), 1, "what comes next is scanned");
    }

    #[test]
    fn a_mac_that_rejoins_scans_nothing_pushed_while_it_was_away() {
        back_after_a_removed_space(|host, github| {
            join_sage(host, github).unwrap();
        });
    }

    #[test]
    fn a_new_space_in_a_kept_clone_scans_nothing_pushed_while_it_was_away() {
        back_after_a_removed_space(|host, github| {
            let staged = crate::storage::stage(&host.home, NAME, &github.tools(), &[]).unwrap();
            let new = crate::storage::tests::form("Sage Watch", None);
            host.create_stored_space(&new, staged).unwrap();
        });
    }

    #[test]
    fn a_kept_clones_first_scan_waits_for_its_fetch_and_scans_no_old_history() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let sage = crate::storage::tests::space_named(&mac.host, "Sage");
        mac.host.remove_space(&sage.id).unwrap();
        github.commit(&in_folder(NOTE), PHONE);
        join_sage(&mut mac.host, &github).unwrap();
        mac.scan();
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github), "the mark waits");
        mac.pull(&github);
        assert_eq!(mac.alerts(), [], "what it missed is old history");
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
    }

    #[test]
    fn a_rewrite_before_a_kept_clones_first_fetch_scans_what_follows() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let base = head(&github);
        let sage = crate::storage::tests::space_named(&mac.host, "Sage");
        mac.host.remove_space(&sage.id).unwrap();
        github.commit(&in_folder(NOTE), PHONE);
        join_sage(&mut mac.host, &github).unwrap();
        git(&github.bare, &["update-ref", "refs/heads/main", &base]);
        let rewritten = in_folder("second-brain/notes/Rewritten.md");
        github.commit(&rewritten, PHONE);
        mac.pull(&github);
        let alerts = mac.alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(
            alerts[0].path, rewritten,
            "the dropped commit is not scanned"
        );
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
    }

    #[test]
    fn a_rewrite_restarts_at_the_merge_base() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let base = head(&github);
        github.commit(&in_folder("second-brain/notes/Dropped.md"), CLEAN);
        mac.pull(&github);
        git(&github.bare, &["update-ref", "refs/heads/main", &base]);
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
        assert_eq!(mac.alerts().len(), 1, "the rewritten commit is scanned");
    }

    #[test]
    fn a_commit_a_merge_brought_is_scanned_and_the_mark_stays_on_the_main_line() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let work = tempfile::tempdir().unwrap();
        git(
            work.path(),
            &["clone", "-q", github.bare.to_str().unwrap(), "."],
        );
        git(work.path(), &["checkout", "-q", "-b", "side"]);
        let side = work.path().join("wiffletree/second-brain/notes");
        fs::create_dir_all(&side).unwrap();
        fs::write(side.join("Side.md"), PHONE).unwrap();
        git(work.path(), &["add", "--all"]);
        git(work.path(), &["commit", "-q", "-m", "Side"]);
        fs::write(side.join("Side.md"), CLEAN).unwrap();
        git(work.path(), &["add", "--all"]);
        git(work.path(), &["commit", "-q", "-m", "Removed on the side"]);
        git(work.path(), &["checkout", "-q", "main"]);
        fs::write(work.path().join("README.md"), "Main moved.\n").unwrap();
        git(work.path(), &["add", "--all"]);
        git(work.path(), &["commit", "-q", "-m", "Main"]);
        git(
            work.path(),
            &["merge", "-q", "--no-ff", "-m", "Merge", "side"],
        );
        git(work.path(), &["push", "-q", "origin", "main"]);
        mac.pull(&github);
        assert_eq!(mac.alerts().len(), 1, "added and removed on a side branch");
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
    }

    #[test]
    fn nine_hundred_commits_do_not_delay_a_pass_and_scan_behind_it() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join(&github);
        let start = head(&github);
        let mut stream = String::new();
        for at in 0..900 {
            let parent = match at {
                0 => format!("from {start}\n"),
                _ => String::new(),
            };
            stream.push_str(&format!(
                "commit refs/heads/main\ncommitter Jo <jo@example.invalid> {} +0000\ndata 1\nx\n{parent}M 100644 inline README.md\ndata 4\n{at:03}\n\n",
                1_700_000_000 + at,
            ));
        }
        let imported =
            runtime::git_bytes(&github.bare, &["fast-import", "--quiet"], stream.as_bytes());
        imported.unwrap();
        mac.pass(&github);
        assert_eq!(mac.mark(SENSITIVE_INFO), start, "the pass scanned nothing");
        assert!(mac.host.storage_pass(&repos::key(NAME)).unwrap().is_some());
        for _ in 0..3 {
            assert!(mac.step());
        }
        let range = format!("{}..main", mac.mark(SENSITIVE_INFO));
        let left = git(&github.bare, &["rev-list", "--count", &range]);
        assert_eq!(left.trim(), "897");
    }

    /// A gitleaks that finds its rule on line 1 of the file at `path`, in a scan that holds it.
    fn finding_gitleaks(bin: &Path, path: &str) {
        let report =
            format!(r#"[{{"File":"{path}","StartLine":1,"StartColumn":1,"RuleID":"github-pat"}}]"#);
        let script = format!(
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --report-path ]; then\n    if [ -f {path} ]; then echo '{report}' > \"$2\"; else echo '[]' > \"$2\"; fi\n  fi\n  shift\ndone\n"
        );
        let gitleaks = bin.join("gitleaks");
        fs::write(&gitleaks, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gitleaks, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn without_gitleaks_the_pull_completes_sensitive_info_runs_and_one_note_says_so() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join_with(&github, scan_tools(false));
        let before = mac.mark(SECRET_SCAN);
        let path = in_folder(NOTE);
        github.commit(&path, PHONE);
        github.commit(&in_folder("second-brain/notes/Two.md"), PHONE);
        mac.pull(&github);
        assert_eq!(
            mac.host.brain_commit(&repos::key(NAME)).unwrap().unwrap(),
            head(&github)
        );
        assert_eq!(mac.mark(SENSITIVE_INFO), head(&github));
        assert_eq!(mac.mark(SECRET_SCAN), before, "nothing is skipped");
        assert_eq!(mac.alerts().len(), 2, "sensitive-info goes on");
        let stopped = mac.stopped();
        assert_eq!(stopped.len(), 1, "{stopped:?}");
        assert_eq!(stopped[0].check, SECRET_SCAN);
        assert!(stopped[0].text.contains("gitleaks"), "{}", stopped[0].text);
        assert!(stopped[0].text.contains("not being scanned for secrets"));
        // Then it arrives, with no new commit to pull.
        finding_gitleaks(mac.bin.path(), &path);
        mac.pass(&github);
        mac.scan();
        assert_eq!(mac.stopped(), [], "the note closes");
        assert_eq!(mac.mark(SECRET_SCAN), head(&github), "it caught up");
        let secrets: Vec<ScanAlert> = (mac.alerts().into_iter())
            .filter(|alert| alert.check == SECRET_SCAN)
            .collect();
        assert_eq!(secrets.len(), 1, "{secrets:?}");
        assert_eq!(secrets[0].rule, "github-pat");
    }

    #[test]
    fn a_store_from_before_this_release_starts_its_marks_at_the_head_it_recorded() {
        let (_folder, github) = fixture();
        github.commit(&in_folder("second-brain/notes/Old.md"), PHONE);
        let mut mac = Mac::join(&github);
        let db = &mac.host.db;
        db.execute("DELETE FROM scan_marks", []).unwrap();
        github.commit(&in_folder(NOTE), PHONE);
        mac.pull(&github);
        let alerts = mac.alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].path, in_folder(NOTE));
    }

    #[test]
    fn a_note_about_a_repository_no_space_uses_shows_nothing() {
        let (_folder, github) = fixture();
        let mut mac = Mac::join_with(&github, scan_tools(false));
        github.commit(&in_folder(NOTE), CLEAN);
        mac.pull(&github);
        assert_eq!(mac.stopped().len(), 1);
        let forgotten = "UPDATE scan_marks SET repository='a/gone'";
        mac.host.db.execute(forgotten, []).unwrap();
        assert_eq!(mac.stopped(), []);
    }

    #[test]
    fn an_alert_is_listed_in_each_project_of_the_spaces_using_the_folder() {
        let mut s = crate::brain_sync::tests::stored();
        s.github.commit(&in_folder(NOTE), PHONE);
        s.sync();
        let key = repos::key(NAME);
        while let Some(job) = s.f.host.pull_scan(&key).unwrap() {
            if !s.f.host.finish_pull_scan(job.run()).unwrap().0 {
                break;
            }
        }
        let project = s.f.coordinator.project_id.clone();
        let notes = s.f.host.workspace_notes(&[]).unwrap();
        let alert = (notes.iter())
            .find(|note| matches!(note, WorkspaceNote::Alert(_)))
            .unwrap();
        assert!(alert.is_for(&project) && alert.needs_you());
        assert!(!alert.is_for("another project"));
        assert_eq!(s.f.host.brains_page().unwrap().alerts.len(), 1);
    }
}
