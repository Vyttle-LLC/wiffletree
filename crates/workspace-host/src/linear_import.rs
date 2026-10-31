//! Import from Linear (OpenSpec change `shared-issues`, task 6.1), once and read-only. The
//! human's personal API key waits in the Keychain while an import runs and is removed when it
//! ends. The import reads one team through Linear's GraphQL API, shows what it would write, and
//! then writes the issues no earlier import wrote through the tracker's own writes, so the scans
//! before a push and the hold on outside edits apply. Each commit holds at most 100 issues: a
//! local tracker commits each batch, and a GitHub tracker's pass pushes one before the next is
//! written. Relations follow once every issue has its ID. Nothing here logs the key or any issue
//! text, and nothing but queries is ever sent to Linear.
use crate::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use tracker::{Store, Write, comment_md, issue_file, issue_folder, issue_md};

const ENDPOINT: &str = "https://api.linear.app/graphql";
/// The most issues one commit adds or changes.
const BATCH: usize = 100;
/// The front matter field holding an issue's Linear identifier, such as `linear: CEL-12`.
const LINEAR_FIELD: &str = "linear";
/// How many passes may end with a batch still unpushed before the import stops.
const PASS_TRIES: usize = 3;
/// How long the key waits for the human's next step before the import starts; then it ends.
const KEY_WAIT_MS: i64 = 30 * 60 * 1000;

const TEAMS: &str = "query Teams($after: String) { teams(first: 250, after: $after) { nodes { id key name } pageInfo { hasNextPage endCursor } } }";
const TEAM: &str = "query Team($team: String!) { viewer { id } team(id: $team) { id key name states(first: 250) { nodes { id name type } } } }";
const ISSUES_PAGE: &str = "query Issues($team: String!, $after: String) { team(id: $team) { issues(first: 25, after: $after) { nodes { id identifier number title description priority createdAt state { id } creator { id name email } assignee { id name email } labels(first: 50) { nodes { name } pageInfo { hasNextPage endCursor } } relations(first: 50) { nodes { type relatedIssue { identifier } } pageInfo { hasNextPage endCursor } } comments(first: 50) { nodes { body createdAt user { id name email } } pageInfo { hasNextPage endCursor } } } pageInfo { hasNextPage endCursor } } } }";
const LABELS_PAGE: &str = "query Labels($issue: String!, $after: String) { issue(id: $issue) { labels(first: 100, after: $after) { nodes { name } pageInfo { hasNextPage endCursor } } } }";
const RELATIONS_PAGE: &str = "query Relations($issue: String!, $after: String) { issue(id: $issue) { relations(first: 100, after: $after) { nodes { type relatedIssue { identifier } } pageInfo { hasNextPage endCursor } } } }";
const COMMENTS_PAGE: &str = "query Comments($issue: String!, $after: String) { issue(id: $issue) { comments(first: 100, after: $after) { nodes { body createdAt user { id name email } } pageInfo { hasNextPage endCursor } } } }";

/// Where the key waits while an import runs.
pub(crate) trait KeyStore: Send + Sync {
    fn keep(&self, key: &LinearKey) -> Result<()>;
    fn read(&self) -> Result<LinearKey>;
    fn remove(&self) -> Result<()>;
}

/// Linear's GraphQL endpoint: posts one request body and answers with Linear's JSON.
pub(crate) trait Api: Send + Sync {
    fn post(&self, key: &LinearKey, body: &Value) -> Result<Value>;
}

#[derive(Clone)]
pub(crate) struct Tools {
    pub(crate) keys: Arc<dyn KeyStore>,
    pub(crate) api: Arc<dyn Api>,
}
impl Tools {
    /// The data folder's login Keychain item, and Linear itself.
    pub(crate) fn real(data: &Path) -> Self {
        Self {
            keys: Arc::new(Keychain(data.to_owned())),
            api: Arc::new(Curl),
        }
    }
}

struct Keychain(PathBuf);
impl KeyStore for Keychain {
    fn keep(&self, key: &LinearKey) -> Result<()> {
        keychain::keep_linear_key(&self.0, &key.0)
    }
    fn read(&self) -> Result<LinearKey> {
        keychain::linear_key(&self.0).map(LinearKey)
    }
    fn remove(&self) -> Result<()> {
        keychain::remove_linear_key(&self.0)
    }
}

struct Curl;
impl Api for Curl {
    fn post(&self, key: &LinearKey, body: &Value) -> Result<Value> {
        use std::io::Write as _;
        use std::process::Stdio;
        let mut curl = std::process::Command::new("/usr/bin/curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                "60",
                "--config",
                "-",
            ])
            .args([
                "--header",
                "Content-Type: application/json",
                "--data-binary",
            ])
            .arg(body.to_string())
            .args(["--write-out", "\n%{http_code}", ENDPOINT])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Run curl")?;
        // The key reaches curl on its standard input, never its command line or environment.
        let config = format!("header = \"Authorization: {}\"\n", key.0);
        (curl.stdin.take().context("curl takes no input")?).write_all(config.as_bytes())?;
        let output = curl.wait_with_output()?;
        ensure!(
            output.status.success(),
            "Linear could not be reached: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let text = String::from_utf8_lossy(&output.stdout);
        let (answer, status) = text.rsplit_once('\n').unwrap_or((&text, ""));
        serde_json::from_str(answer)
            .map_err(|_| anyhow::anyhow!("Linear answered with HTTP {}", status.trim()))
    }
}

/// A query to Linear. Anything else is refused before it is sent, so nothing in Linear changes.
fn read(api: &dyn Api, key: &LinearKey, document: &str, variables: Value) -> Result<Value> {
    ensure!(
        document.starts_with("query "),
        "Wiffletree only reads Linear"
    );
    let answer = api.post(key, &json!({"query": document, "variables": variables}))?;
    if let Some(error) = answer["errors"]
        .as_array()
        .and_then(|errors| errors.first())
    {
        let reason = error["message"].as_str().unwrap_or("no reason given");
        bail!("Linear refused the read: {reason}");
    }
    ensure!(answer["data"].is_object(), "Linear's answer has no data");
    Ok(answer["data"].clone())
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<T>,
    #[serde(default, rename = "pageInfo")]
    page: Option<Page>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    has_next_page: bool,
    end_cursor: Option<String>,
}
impl Page {
    fn next(page: Option<Page>) -> Option<String> {
        page.filter(|page| page.has_next_page)
            .and_then(|page| page.end_cursor)
    }
}
#[derive(Clone, Deserialize)]
struct Person {
    id: String,
    name: String,
    #[serde(default)]
    email: Option<String>,
}
#[derive(Deserialize)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
struct State {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
struct Name {
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Relation {
    #[serde(rename = "type")]
    kind: String,
    related_issue: Identifier,
}
#[derive(Deserialize)]
struct Identifier {
    identifier: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinearComment {
    body: String,
    created_at: String,
    #[serde(default)]
    user: Option<Person>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinearIssue {
    id: String,
    identifier: String,
    number: u64,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: f64,
    created_at: String,
    state: Id,
    #[serde(default)]
    creator: Option<Person>,
    #[serde(default)]
    assignee: Option<Person>,
    labels: Nodes<Name>,
    relations: Nodes<Relation>,
    comments: Nodes<LinearComment>,
}

/// One Linear team as an import reads it, every comment included.
pub(crate) struct Team {
    team: LinearTeam,
    /// The Linear user the key belongs to: the human.
    viewer: String,
    states: Vec<State>,
    issues: Vec<LinearIssue>,
}

/// The teams the key reads, a page at a time.
pub(crate) fn teams(api: &dyn Api, key: &LinearKey) -> Result<Vec<LinearTeam>> {
    let (mut teams, mut after) = (vec![], None);
    loop {
        let data = read(api, key, TEAMS, json!({ "after": after }))?;
        let page: Nodes<LinearTeam> = serde_json::from_value(data["teams"].clone())?;
        teams.extend(page.nodes);
        after = Page::next(page.page);
        if after.is_none() {
            return Ok(teams);
        }
    }
}

/// The rest of one of an issue's lists, `field`, after its first page.
fn rest<T: DeserializeOwned>(
    api: &dyn Api,
    key: &LinearKey,
    (document, field): (&str, &str),
    issue: &str,
    list: &mut Nodes<T>,
) -> Result<()> {
    while let Some(after) = Page::next(list.page.take()) {
        let variables = json!({ "issue": issue, "after": after });
        let data = read(api, key, document, variables)?;
        let more: Nodes<T> = serde_json::from_value(data["issue"][field].clone())?;
        list.nodes.extend(more.nodes);
        list.page = more.page;
    }
    Ok(())
}

/// Reads the team with its states and every issue with all its labels, relations and
/// comments, a page at a time.
pub(crate) fn team(api: &dyn Api, key: &LinearKey, team_id: &str) -> Result<Team> {
    let data = read(api, key, TEAM, json!({ "team": team_id }))?;
    let viewer = (data["viewer"]["id"].as_str()).context("Linear did not say whose key it is")?;
    let states: Nodes<State> = serde_json::from_value(data["team"]["states"].clone())?;
    let team: LinearTeam = serde_json::from_value(data["team"].clone())?;
    let (mut issues, mut after) = (vec![], None);
    loop {
        let variables = json!({ "team": team_id, "after": after });
        let data = read(api, key, ISSUES_PAGE, variables)?;
        let page: Nodes<LinearIssue> = serde_json::from_value(data["team"]["issues"].clone())?;
        issues.extend(page.nodes);
        after = Page::next(page.page);
        if after.is_none() {
            break;
        }
    }
    for issue in &mut issues {
        let id = &issue.id;
        rest(api, key, (LABELS_PAGE, "labels"), id, &mut issue.labels)?;
        rest(
            api,
            key,
            (RELATIONS_PAGE, "relations"),
            id,
            &mut issue.relations,
        )?;
        rest(
            api,
            key,
            (COMMENTS_PAGE, "comments"),
            id,
            &mut issue.comments,
        )?;
    }
    Ok(Team {
        team,
        viewer: viewer.to_owned(),
        states: states.nodes,
        issues,
    })
}

/// The seven states a Linear workflow state's type maps to; a started state named for review
/// is In review.
fn state_of(state: &State) -> IssueState {
    match state.kind.as_str() {
        "backlog" => IssueState::Backlog,
        "unstarted" => IssueState::Todo,
        "started" if state.name.to_lowercase().contains("review") => IssueState::InReview,
        "started" => IssueState::InProgress,
        "completed" => IssueState::Done,
        "canceled" => IssueState::Canceled,
        _ => IssueState::Triage,
    }
}

fn priority_of(priority: f64) -> Priority {
    match priority.round() as i64 {
        1 => Priority::Urgent,
        2 => Priority::High,
        3 => Priority::Medium,
        4 => Priority::Low,
        _ => Priority::None,
    }
}

/// Linear's time, such as `2026-10-29T14:02:11.123Z`, as the tracker writes times.
fn time_of(at: &str) -> Result<String> {
    let at = chrono::DateTime::parse_from_rfc3339(at)?;
    Ok((at.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ")).to_string())
}

/// A title as the tracker takes one: one line of at most 500 bytes.
fn title_of(title: &str) -> String {
    let mut title = title.replace(['\r', '\n'], " ").trim().to_owned();
    if title.is_empty() {
        return "Untitled".into();
    }
    while title.len() > 500 {
        title.pop();
    }
    title
}

/// Each issue an earlier import wrote, by its Linear identifier.
fn imported(store: &Store) -> BTreeMap<String, IssueId> {
    (store.ids().into_iter())
        .filter_map(|id| {
            let issue = store.issue(&id).ok()?;
            let identifier = issue.extra.get(LINEAR_FIELD)?.as_str()?.to_owned();
            Some((identifier, id))
        })
        .collect()
}

/// The numbers the tracker's issues have, or have had in any commit here, so none is reused.
fn used_numbers(store: &Store) -> Result<BTreeSet<u64>> {
    let args = [
        "log",
        "--all",
        "--format=",
        "--name-only",
        "--",
        tracker::ISSUES,
    ];
    let committed = runtime::git_output(&store.folder, &args)?;
    let ids = (committed.lines().filter_map(tracker::issue_of)).chain(store.ids());
    Ok(ids
        .filter_map(|id| match id {
            IssueId::Numbered { prefix, number } if prefix == store.prefix => Some(number),
            _ => None,
        })
        .collect())
}

/// What a step of an import leaves to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Write the next batch.
    Next,
    /// A pass of the Space's tracker must push the batch written before the next is.
    Push(SpaceId),
    /// The import ended.
    Done,
}

/// Relations one issue gains.
#[derive(Clone, Default)]
struct Related {
    blocks: Vec<String>,
    blocked_by: Vec<String>,
    related: Vec<String>,
    duplicate_of: Option<String>,
}

struct Run {
    space_id: SpaceId,
    /// The issues still to write, those keeping their Linear number first.
    queue: VecDeque<LinearIssue>,
    /// Each Linear relation of the team's issues: from, kind and to, by identifier.
    relations: Vec<(String, String, String)>,
    /// The relations still to write, once every issue is written.
    related: Option<VecDeque<(IssueId, Related)>>,
    /// By Linear user ID, the GitHub login the human confirmed.
    logins: BTreeMap<String, String>,
    /// The human's login, the author of what no confirmed login wrote.
    importer: String,
    states: BTreeMap<String, IssueState>,
    team_key: String,
    /// While a batch waits for a pass: how many passes ended without pushing it.
    waiting: Option<usize>,
}
impl Run {
    fn login(&self, person: &Option<Person>) -> Option<String> {
        (person.as_ref()).and_then(|person| self.logins.get(&person.id).cloned())
    }
    /// The author of what `person` wrote in Linear, and the first line naming them when no
    /// login was confirmed for them.
    fn author(&self, person: &Option<Person>, wrote: &str) -> (String, Option<String>) {
        match (self.login(person), person) {
            (Some(login), _) => (login, None),
            (None, Some(person)) => {
                let line = format!("{wrote} in Linear by {}.", person.name);
                (self.importer.clone(), Some(line))
            }
            (None, None) => (self.importer.clone(), None),
        }
    }
}

/// The import on this Mac, one at a time, and how the last one went.
pub(crate) struct Linear {
    pub(crate) tools: Tools,
    /// The Space whose import began, and the team once read.
    began: Option<(SpaceId, Option<Team>)>,
    /// When the human last took a step before the import started.
    stepped_at: i64,
    run: Option<Run>,
    pub(crate) status: Option<LinearImportStatus>,
}
impl Linear {
    pub(crate) fn new(tools: Tools) -> Self {
        Self {
            tools,
            began: None,
            stepped_at: 0,
            run: None,
            status: None,
        }
    }
}

/// `text` with `line` before it, as its own paragraph.
fn with_first_line(line: Option<String>, text: &str) -> String {
    match line {
        Some(line) if text.trim().is_empty() => line,
        Some(line) => format!("{line}\n\n{text}"),
        None => text.to_owned(),
    }
}

impl Host {
    /// Begins an import into the Space's tracker with the human's key, which waits in the
    /// Keychain until the import ends.
    pub(crate) fn begin_linear_import(
        &mut self,
        space_id: &SpaceId,
        key: &LinearKey,
    ) -> Result<()> {
        ensure!(
            self.linear.run.is_none(),
            "An import from Linear is running"
        );
        self.issue_author()?;
        self.tracker(&self.space(space_id)?.space)?;
        ensure!(
            !key.0.is_empty() && (key.0.chars()).all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "A Linear personal API key is letters, digits and underscores, such as lin_api_…"
        );
        self.linear.tools.keys.keep(key)?;
        self.linear.began = Some((space_id.clone(), None));
        self.linear.stepped_at = now();
        self.linear.status = None;
        Ok(())
    }
    /// Passes on a read of Linear for the import begun, ending the import when the read failed
    /// so the key does not outlast it.
    pub(crate) fn linear_read<T>(&mut self, read: Result<T>) -> Result<T> {
        if read.is_err() && self.linear.run.is_none() {
            self.end_linear_import(None);
        }
        read
    }
    /// Ends an import that has not started once its key has waited `KEY_WAIT_MS` at `at` for
    /// the human's next step, as when the app quit mid-way; while it waits, when it will end.
    pub(crate) fn expire_linear_key(&mut self, at: i64) -> Option<i64> {
        if self.linear.began.is_none() || self.linear.run.is_some() {
            return None;
        }
        let ends = self.linear.stepped_at + KEY_WAIT_MS;
        if at < ends {
            return Some(ends);
        }
        self.end_linear_import(None);
        None
    }
    /// Refuses a step of an import that did not begin in the Space.
    pub(crate) fn linear_began(&self, space_id: &SpaceId) -> Result<()> {
        ensure!(
            (self.linear.began.as_ref()).is_some_and(|(began, _)| began == space_id),
            "Paste the Linear key again to import"
        );
        Ok(())
    }
    /// What importing the team read into the Space's tracker would write; nothing is written.
    pub(crate) fn linear_plan(
        &mut self,
        space_id: &SpaceId,
        team: Team,
    ) -> Result<LinearImportPlan> {
        self.linear_began(space_id)?;
        let space = self.space(space_id)?.space;
        let store = self.store(&space, &self.tracker(&space)?.prefix);
        let before = imported(&store);
        // Relations among issues imported before that an import cut short never wrote.
        let imported_ids: BTreeSet<&IssueId> = before.values().collect();
        let relations_to_add = (relations(&team_relations(&team.issues), &before).into_iter())
            .filter(|(id, _)| imported_ids.contains(id))
            .filter(|(id, adds)| {
                (store.issue(id)).is_ok_and(|mut issue| relate(&mut issue, adds.clone()))
            })
            .count();
        let used = used_numbers(&store)?;
        let new: Vec<&LinearIssue> = (team.issues.iter())
            .filter(|issue| !before.contains_key(&issue.identifier))
            .collect();
        let keeps =
            |issue: &LinearIssue| store.prefix == team.team.key && !used.contains(&issue.number);
        let mut states: Vec<LinearStateMapping> = (team.states.iter())
            .map(|state| LinearStateMapping {
                name: state.name.clone(),
                state: state_of(state),
                issues: new
                    .iter()
                    .filter(|issue| issue.state.id == state.id)
                    .count(),
            })
            .filter(|mapping| mapping.issues > 0)
            .collect();
        states.sort_by(|a, b| (a.state, &a.name).cmp(&(b.state, &b.name)));
        let mut people: BTreeMap<String, Person> = BTreeMap::new();
        for issue in &new {
            let comments = issue.comments.nodes.iter().map(|c| &c.user);
            let named = [&issue.creator, &issue.assignee]
                .into_iter()
                .chain(comments);
            for person in named.flatten() {
                (people.entry(person.id.clone())).or_insert_with(|| person.clone());
            }
        }
        let known = self.issue_people(space_id)?;
        let mut users: Vec<LinearUser> = (people.into_values())
            .map(|person| LinearUser {
                login: match person.id == team.viewer {
                    true => self.gh_login.clone(),
                    false => suggested_login(&person, &known),
                },
                id: person.id,
                name: person.name,
                email: person.email,
            })
            .collect();
        users.sort_by_key(|user| user.name.to_lowercase());
        let plan = LinearImportPlan {
            space_id: space_id.clone(),
            team: team.team.clone(),
            issues: new.len(),
            imported_before: team.issues.len() - new.len(),
            comments: new.iter().map(|issue| issue.comments.nodes.len()).sum(),
            keeping_numbers: new.iter().filter(|issue| keeps(issue)).count(),
            relations_to_add,
            states,
            users,
        };
        self.linear.began = Some((space_id.clone(), Some(team)));
        self.linear.stepped_at = now();
        Ok(plan)
    }
    /// Starts writing the previewed team's new issues, naming each Linear user by the login the
    /// human confirmed for them in `logins`.
    pub(crate) fn start_linear_import(
        &mut self,
        space_id: &SpaceId,
        logins: BTreeMap<String, String>,
    ) -> Result<()> {
        let importer = self.issue_author()?;
        ensure!(
            self.linear.run.is_none(),
            "An import from Linear is running"
        );
        for login in logins.values() {
            ensure!(
                issue_inbox::is_login(login),
                "{login} is not a GitHub login"
            );
        }
        let space = self.space(space_id)?.space;
        let store = self.store(&space, &self.tracker(&space)?.prefix);
        if let StorageRepository::GitHub { .. } = space.storage.repository {
            ensure!(
                self.unpushed_issue_files(&space.storage.repository)? == 0,
                "This tracker has changes waiting to be pushed. Sync them, then import."
            );
        }
        let team = match self.linear.began.take() {
            Some((began, Some(team))) if &began == space_id => team,
            began => {
                self.linear.began = began;
                bail!("Preview the Linear team before importing it");
            }
        };
        let before = imported(&store);
        let used = used_numbers(&store)?;
        // Every relation of the team, so a run cut short before its relations are written
        // has them written by the next.
        let relations = team_relations(&team.issues);
        let (mut kept, mut renumbered): (Vec<LinearIssue>, Vec<LinearIssue>) = (team.issues)
            .into_iter()
            .filter(|issue| !before.contains_key(&issue.identifier))
            .partition(|issue| store.prefix == team.team.key && !used.contains(&issue.number));
        kept.sort_by_key(|issue| issue.number);
        renumbered.sort_by_key(|issue| issue.number);
        let queue: VecDeque<LinearIssue> = kept.into_iter().chain(renumbered).collect();
        self.linear.status = Some(LinearImportStatus {
            space_id: space_id.clone(),
            team: team.team.key.clone(),
            written: 0,
            total: queue.len(),
            finished: false,
            error: None,
        });
        self.linear.run = Some(Run {
            space_id: space_id.clone(),
            queue,
            relations,
            related: None,
            logins,
            importer,
            states: (team.states.iter())
                .map(|state| (state.id.clone(), state_of(state)))
                .collect(),
            team_key: team.team.key,
            waiting: None,
        });
        Ok(())
    }
    /// Whether an import has a batch to write now.
    pub(crate) fn linear_ready(&self) -> bool {
        (self.linear.run.as_ref()).is_some_and(|run| run.waiting.is_none())
    }
    /// Writes the import's next batch, or ends it once nothing is left or a write fails.
    pub(crate) fn linear_step(&mut self) -> Step {
        let Some(mut run) = self.linear.run.take() else {
            return Step::Done;
        };
        match self.write_linear_batch(&mut run) {
            Ok(false) => {
                self.end_linear_import(None);
                Step::Done
            }
            Err(error) => {
                self.end_linear_import(Some(format!("{error:#}")));
                Step::Done
            }
            Ok(true) => {
                let space_id = run.space_id.clone();
                let storage = self.space(&space_id).map(|r| r.space.storage.repository);
                let step = match storage {
                    Ok(StorageRepository::GitHub { .. }) => {
                        run.waiting = Some(0);
                        Step::Push(space_id)
                    }
                    _ => Step::Next,
                };
                self.linear.run = Some(run);
                step
            }
        }
    }
    /// After a pass of the storage repository `key`: the import goes on once its batch was
    /// pushed, asks for another pass while it was not, and stops when the pass failed or
    /// passes keep leaving it unpushed.
    pub(crate) fn linear_pass_ended(&mut self, key: &str) -> Option<Step> {
        let run = self
            .linear
            .run
            .as_mut()
            .filter(|run| run.waiting.is_some())?;
        let space_id = run.space_id.clone();
        let storage = self.space(&space_id).ok()?.space.storage.repository;
        let ours = repos::key(&storage.key());
        if ours != key {
            return None;
        }
        let unpushed = self.unpushed_issue_files(&storage);
        let (_, error) = self.storage_sync_state(&ours).unwrap_or_default();
        let run = self.linear.run.as_mut()?;
        let tries = run.waiting.unwrap_or_default() + 1;
        let stopped = match (unpushed, error) {
            (Ok(0), _) => {
                run.waiting = None;
                return Some(Step::Next);
            }
            (Err(error), _) => format!("{error:#}"),
            (Ok(_), Some(error)) => format!("GitHub was not updated: {error}"),
            (Ok(_), None) if tries >= PASS_TRIES => "The last batch could not be pushed".into(),
            (Ok(_), None) => {
                run.waiting = Some(tries);
                return Some(Step::Push(space_id));
            }
        };
        self.end_linear_import(Some(stopped));
        Some(Step::Done)
    }
    /// Ends the import, a running one after the batch it wrote, removes the key from the
    /// Keychain and closes the import's status.
    pub(crate) fn cancel_linear_import(&mut self) -> Result<()> {
        self.linear.run = None;
        self.linear.began = None;
        self.linear.status = None;
        self.linear.tools.keys.remove()
    }
    /// Runs the whole import on this thread, with `tools` for each pass of a GitHub tracker.
    pub(crate) fn import_from_linear_now(
        &mut self,
        space_id: &SpaceId,
        logins: BTreeMap<String, String>,
        tools: &publish::Tools,
    ) -> Result<()> {
        self.start_linear_import(space_id, logins)?;
        let mut step = Step::Next;
        loop {
            step = match step {
                Step::Next => self.linear_step(),
                Step::Push(space_id) => {
                    // A failed pass is recorded, and read back as the import's reason to stop.
                    let _ = self.sync_issues(Some(&space_id), tools);
                    let key = repos::key(&self.space(&space_id)?.space.storage.repository.key());
                    self.linear_pass_ended(&key).unwrap_or(Step::Done)
                }
                Step::Done => return Ok(()),
            };
        }
    }
    fn end_linear_import(&mut self, error: Option<String>) {
        self.linear.run = None;
        self.linear.began = None;
        if let Some(status) = &mut self.linear.status {
            status.finished = true;
            status.error = error;
        }
        if let Err(error) = self.linear.tools.keys.remove() {
            eprintln!("Linear import: the key was not removed from the Keychain: {error:#}");
        }
    }
    /// The tracker's issue files this Mac wrote that no pass has pushed, leaving out those a
    /// scan holds or sync paused, which wait for the human.
    fn unpushed_issue_files(&self, storage: &StorageRepository) -> Result<usize> {
        Ok(self.db.query_row(
            "SELECT count(*) FROM sync_record r WHERE repository=?1 AND path LIKE 'issues/%' AND written IS NOT synced AND NOT EXISTS (SELECT 1 FROM sync_held h WHERE h.repository=r.repository AND h.path=r.path) AND NOT EXISTS (SELECT 1 FROM sync_paused p WHERE p.repository=r.repository AND p.path=r.path)",
            [repos::key(&storage.key())],
            |r| r.get(0),
        )?)
    }
    /// Writes the next batch of issues or, once all are written, of relations; `false` when
    /// nothing was left.
    fn write_linear_batch(&mut self, run: &mut Run) -> Result<bool> {
        let space = self.space(&run.space_id)?.space;
        let store = self.store(&space, &self.tracker(&space)?.prefix);
        if !run.queue.is_empty() {
            let mut used = used_numbers(&store)?;
            let mut writes = vec![];
            for _ in 0..BATCH.min(run.queue.len()) {
                let issue = run.queue.pop_front().context("A queued issue")?;
                let same_prefix = store.prefix == run.team_key;
                let id = match &space.storage.repository {
                    // Numbered by a pass, as a new issue is: Linear's number when GitHub's
                    // history never had it, and none while a scan holds it.
                    StorageRepository::GitHub { .. } => {
                        let id = self.temporary_id(&space.storage.repository, &store.prefix)?;
                        if same_prefix {
                            self.db.execute(
                                "INSERT INTO issue_preferred(storage,temporary,number) VALUES (?1,?2,?3)",
                                params![space.storage.repository.key(), id.to_string(), issue.number],
                            )?;
                        }
                        id
                    }
                    StorageRepository::Local { .. } => {
                        let number = match same_prefix && !used.contains(&issue.number) {
                            true => issue.number,
                            false => used.last().copied().unwrap_or(0) + 1,
                        };
                        used.insert(number);
                        IssueId::Numbered {
                            prefix: store.prefix.clone(),
                            number,
                        }
                    }
                };
                writes.push(issue_write(run, &issue, id)?);
            }
            let count = writes.len();
            let message = format!("Imported {count} issues from Linear");
            self.write_issues(&space, writes, &message)?;
            if let Some(status) = &mut self.linear.status {
                status.written += count;
            }
            return Ok(true);
        }
        let related = run
            .related
            .get_or_insert_with(|| relations(&run.relations, &imported(&store)));
        let mut writes = vec![];
        while writes.len() < BATCH
            && let Some((id, adds)) = related.pop_front()
        {
            // One a teammate broke meanwhile keeps its file as it is.
            let Ok(mut issue) = store.issue(&id) else {
                continue;
            };
            if relate(&mut issue, adds) {
                let files = vec![(issue_file(&id), issue_md(&issue)?)];
                let did = "related".into();
                writes.push(Write { id, files, did });
            }
        }
        if writes.is_empty() {
            return Ok(false);
        }
        let message = format!("Related {} issues imported from Linear", writes.len());
        self.write_issues(&space, writes, &message)?;
        Ok(true)
    }
}

/// Each Linear relation of `issues`: from, kind and to, by identifier.
fn team_relations(issues: &[LinearIssue]) -> Vec<(String, String, String)> {
    (issues.iter())
        .flat_map(|issue| {
            (issue.relations.nodes.iter()).map(|relation| {
                let to = relation.related_issue.identifier.clone();
                (issue.identifier.clone(), relation.kind.clone(), to)
            })
        })
        .collect()
}

/// Adds the relations `adds` names that `issue` lacks; `true` when it gained any.
fn relate(issue: &mut Issue, adds: Related) -> bool {
    let before = issue.clone();
    let add = |list: &mut Vec<String>, ids: Vec<String>| {
        for id in ids {
            if !list.contains(&id) {
                list.push(id);
            }
        }
    };
    add(&mut issue.blocks, adds.blocks);
    add(&mut issue.blocked_by, adds.blocked_by);
    add(&mut issue.related, adds.related);
    issue.duplicate_of = issue.duplicate_of.take().or(adds.duplicate_of);
    *issue != before
}

/// The login of a Linear user as the tracker's people have it: by login, by profile name, or by
/// the start of their email.
fn suggested_login(person: &Person, known: &[IssuePerson]) -> Option<String> {
    let local = (person.email.as_deref()).and_then(|email| email.split('@').next());
    let names = [Some(person.name.as_str()), local];
    (known.iter())
        .find(|known| {
            names.iter().flatten().any(|name| {
                known.login.eq_ignore_ascii_case(name)
                    || (known.name.as_deref()).is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
        })
        .map(|known| known.login.clone())
}

/// The relations to write, by issue, among the issues imported now or before: a Linear
/// `blocks` on both ends, a duplicate on the duplicate, and anything else as related both ways.
fn relations(
    relations: &[(String, String, String)],
    imported: &BTreeMap<String, IssueId>,
) -> VecDeque<(IssueId, Related)> {
    let mut related: BTreeMap<IssueId, Related> = BTreeMap::new();
    for (from, kind, to) in relations {
        let (Some(a), Some(b)) = (imported.get(from), imported.get(to)) else {
            continue;
        };
        let (a_text, b_text) = (a.to_string(), b.to_string());
        match kind.as_str() {
            "blocks" => {
                related.entry(a.clone()).or_default().blocks.push(b_text);
                related
                    .entry(b.clone())
                    .or_default()
                    .blocked_by
                    .push(a_text);
            }
            "duplicate" => {
                (related.entry(a.clone()).or_default().duplicate_of).get_or_insert(b_text);
            }
            _ => {
                related.entry(a.clone()).or_default().related.push(b_text);
                related.entry(b.clone()).or_default().related.push(a_text);
            }
        }
    }
    related.into_iter().collect()
}

/// The files of one imported issue: `issue.md` with its Linear identifier, and its comments.
fn issue_write(run: &Run, linear: &LinearIssue, id: IssueId) -> Result<Write> {
    let (author, line) = run.author(&linear.creator, "Filed");
    let description = linear.description.as_deref().unwrap_or_default().trim_end();
    let mut extra = Extra::new();
    extra.insert(LINEAR_FIELD.into(), json!(linear.identifier));
    let issue = Issue {
        id: id.clone(),
        title: title_of(&linear.title),
        state: (run.states.get(&linear.state.id).copied()).unwrap_or_default(),
        priority: priority_of(linear.priority),
        assignee: run.login(&linear.assignee),
        labels: linear.labels.nodes.iter().map(|l| l.name.clone()).collect(),
        repositories: vec![],
        blocks: vec![],
        blocked_by: vec![],
        related: vec![],
        duplicate_of: None,
        author,
        created: time_of(&linear.created_at)?,
        description: with_first_line(line, description),
        extra,
    };
    let mut files = vec![(issue_file(&id), issue_md(&issue)?)];
    let mut names = BTreeSet::new();
    let mut comments: Vec<&LinearComment> = linear.comments.nodes.iter().collect();
    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    for comment in comments {
        let (author, line) = run.author(&comment.user, "Written");
        let created = time_of(&comment.created_at)?;
        let stamp = format!("{}-{author}", created.replace(':', "-"));
        let name = (1..)
            .map(|n| match n {
                1 => format!("{stamp}.md"),
                n => format!("{stamp}-{n}.md"),
            })
            .find(|name| !names.contains(name))
            .expect("a free name");
        names.insert(name.clone());
        let content = comment_md(&Comment {
            file: name.clone(),
            author,
            created,
            edited: None,
            assistant_for: None,
            body: with_first_line(line, comment.body.trim_end()),
        })?;
        files.push((format!("{}/comments/{name}", issue_folder(&id)), content));
    }
    let did = "imported".into();
    Ok(Write { id, files, did })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::storage::tests::{host, join_sage, shared_storage, space_named};
    use crate::test_support::git;
    use std::sync::Mutex;

    /// The key as the Keychain would keep it.
    #[derive(Default)]
    pub(crate) struct Kept(Mutex<Option<LinearKey>>);
    impl KeyStore for Kept {
        fn keep(&self, key: &LinearKey) -> Result<()> {
            *self.0.lock().unwrap() = Some(key.clone());
            Ok(())
        }
        fn read(&self) -> Result<LinearKey> {
            (self.0.lock().unwrap().clone()).context("No key is kept")
        }
        fn remove(&self) -> Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    impl Kept {
        pub(crate) fn kept(&self) -> Option<LinearKey> {
            self.0.lock().unwrap().clone()
        }
    }

    pub(crate) type Answer = Box<dyn Fn(&str, &Value) -> Value + Send + Sync>;

    /// Linear as recorded: each request answered by its operation's name and variables. Every
    /// body sent is kept.
    pub(crate) struct Recorded {
        answer: Answer,
        sent: Mutex<Vec<Value>>,
    }
    impl Api for Recorded {
        fn post(&self, key: &LinearKey, body: &Value) -> Result<Value> {
            assert_eq!(key.0, KEY);
            self.sent.lock().unwrap().push(body.clone());
            let document = body["query"].as_str().unwrap();
            let operation = document.split([' ', '(', '{']).nth(1).unwrap();
            Ok((self.answer)(operation, &body["variables"]))
        }
    }
    impl Recorded {
        /// Each document sent.
        fn documents(&self) -> Vec<String> {
            (self.sent.lock().unwrap().iter())
                .map(|body| body["query"].as_str().unwrap().to_owned())
                .collect()
        }
    }

    const KEY: &str = "lin_api_recorded";

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/linear/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Team CEL as recorded: three issues on two pages, the last with a second page of
    /// comments, and `more` issues appended to the last page.
    fn celadon(more: Vec<Value>) -> Answer {
        Box::new(move |operation, variables| match operation {
            "Teams" => teams_page(variables),
            "Team" => fixture("team.json"),
            "Issues" if variables["after"].is_null() => fixture("issues-1.json"),
            "Issues" => {
                assert_eq!(variables["after"], "cursor-1");
                let mut page = fixture("issues-2.json");
                let nodes = page["data"]["team"]["issues"]["nodes"]
                    .as_array_mut()
                    .unwrap();
                nodes.extend(more.iter().cloned());
                page
            }
            "Comments" => {
                assert_eq!(
                    variables,
                    &json!({"issue":"issue-3","after":"comment-cursor-1"})
                );
                fixture("comments.json")
            }
            "Labels" => {
                assert_eq!(
                    variables,
                    &json!({"issue":"issue-1","after":"label-cursor-1"})
                );
                fixture("labels.json")
            }
            "Relations" => {
                let after = json!({"issue":"issue-1","after":"relation-cursor-1"});
                assert_eq!(variables, &after);
                fixture("relations.json")
            }
            other => panic!("{other} was not recorded"),
        })
    }

    /// A Linear issue of team CEL in Todo, with nothing else.
    fn plain(number: u64) -> Value {
        json!({
            "id": format!("issue-{number}"),
            "identifier": format!("CEL-{number}"),
            "number": number,
            "title": format!("Issue {number}"),
            "description": null,
            "priority": 0,
            "createdAt": "2026-09-01T10:00:00.000Z",
            "state": {"id": "state-todo"},
            "creator": null,
            "assignee": null,
            "labels": {"nodes": []},
            "relations": {"nodes": []},
            "comments": {"nodes": [], "pageInfo": {"hasNextPage": false, "endCursor": null}},
        })
    }

    /// Team CEL with `count` plain issues, 25 a page as the import asks.
    pub(crate) fn many(count: u64) -> Answer {
        listed((1..=count).map(plain).collect())
    }

    /// Team CEL with `issues`, 25 a page as the import asks.
    fn listed(issues: Vec<Value>) -> Answer {
        Box::new(move |operation, variables| match operation {
            "Team" => fixture("team.json"),
            "Teams" => teams_page(variables),
            "Issues" => {
                let from: usize =
                    (variables["after"].as_str()).map_or(0, |after| after.parse().unwrap());
                let to = (from + 25).min(issues.len());
                let nodes = &issues[from..to];
                let more = to < issues.len();
                let page = json!({"hasNextPage": more, "endCursor": to.to_string()});
                json!({"data":{"team":{"issues":{"nodes": nodes, "pageInfo": page}}}})
            }
            other => panic!("{other} was not recorded"),
        })
    }

    /// The key's teams, CEL on the first page and OPS on the second.
    fn teams_page(variables: &Value) -> Value {
        match variables["after"].as_str() {
            None => fixture("teams.json"),
            Some(after) => {
                assert_eq!(after, "team-cursor-1");
                fixture("teams-2.json")
            }
        }
    }

    /// Gives the host a kept key and Linear as `answer` records it.
    pub(crate) fn recorded(host: &mut Host, answer: Answer) -> (Arc<Kept>, Arc<Recorded>) {
        let keys = Arc::new(Kept::default());
        let api = Arc::new(Recorded {
            answer,
            sent: Mutex::default(),
        });
        host.linear.tools = Tools {
            keys: keys.clone(),
            api: api.clone(),
        };
        (keys, api)
    }

    pub(crate) fn begin(host: &mut Host, space: &Space) -> Result<Value> {
        let key = LinearKey(KEY.into());
        host.execute(Command::LinearTeams {
            space_id: space.id.clone(),
            key,
        })
    }

    pub(crate) fn preview(host: &mut Host, space: &Space) -> Result<LinearImportPlan> {
        let team_id = "team-cel".into();
        let space_id = space.id.clone();
        let plan = host.execute(Command::PreviewLinearImport { space_id, team_id })?;
        Ok(serde_json::from_value(plan)?)
    }

    /// Begins, previews and runs an import of team CEL, with `tools` for a GitHub tracker's
    /// passes.
    fn import(
        host: &mut Host,
        space: &Space,
        logins: &[(&str, &str)],
        tools: &publish::Tools,
    ) -> LinearImportStatus {
        begin(host, space).unwrap();
        preview(host, space).unwrap();
        let logins = (logins.iter())
            .map(|(id, login)| (id.to_string(), login.to_string()))
            .collect();
        host.import_from_linear_now(&space.id, logins, tools)
            .unwrap();
        host.linear.status.clone().unwrap()
    }

    /// Personal on this Mac, a local tracker, with the human signed in to `gh` as `jlin`.
    fn personal() -> (tempfile::TempDir, Host, Space, String) {
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        let personal = host.spaces().unwrap().remove(0).space;
        let prefix = host.tracker(&personal).unwrap().prefix;
        (home, host, personal, prefix)
    }

    fn issue(host: &Host, id: &str) -> IssueRecord {
        host.read_issue(&IssueId::parse(id).unwrap()).unwrap()
    }

    #[test]
    fn importing_team_cel_into_an_empty_cel_tracker_pushes_three_commits_keeping_numbers() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("CEL"));
        let (_home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let (keys, api) = recorded(&mut host, many(240));
        let before = github.log("%s").len();
        let status = import(&mut host, &sage, &[], &github.tools());
        assert_eq!((status.written, status.total), (240, 240));
        assert!(status.finished && status.error.is_none(), "{status:?}");
        let subjects = github.log("%s");
        assert_eq!(subjects.len() - before, 3, "{subjects:?}");
        for subject in &subjects[..3] {
            let issues = subject.matches(" filed").count();
            assert!((1..=100).contains(&issues), "{subject}");
        }
        for number in [1, 100, 101, 240] {
            let there = github
                .show(&format!("issues/CEL-{number}/issue.md"))
                .unwrap();
            assert!(
                there.contains(&format!("\nlinear: CEL-{number}\n")),
                "{there}"
            );
        }
        assert!(
            keys.kept().is_none(),
            "the key is removed once the import ends"
        );
        let documents = api.documents();
        assert!(
            documents.iter().all(|d| d.starts_with("query ")),
            "reads only"
        );
        assert_eq!(
            documents.len(),
            3 + 10,
            "two pages of teams, the team and ten pages"
        );
    }

    #[test]
    fn the_preview_shows_counts_states_and_people_and_writes_nothing() {
        let (_home, mut host, personal, _) = personal();
        let (keys, _) = recorded(&mut host, celadon(vec![]));
        let teams: Vec<LinearTeam> =
            serde_json::from_value(begin(&mut host, &personal).unwrap()).unwrap();
        let keys_of: Vec<&str> = teams.iter().map(|team| team.key.as_str()).collect();
        assert_eq!(keys_of, ["CEL", "OPS"]);
        assert_eq!(
            keys.kept(),
            Some(LinearKey(KEY.into())),
            "kept while it runs"
        );
        let folder = host.storage_folder(&personal.storage.repository);
        let commits = git(&folder, &["rev-list", "--count", "HEAD"]);
        let plan = preview(&mut host, &personal).unwrap();
        assert_eq!((plan.issues, plan.imported_before), (3, 0));
        assert_eq!((plan.comments, plan.keeping_numbers), (4, 0));
        let states: Vec<(&str, IssueState, usize)> = (plan.states.iter())
            .map(|s| (s.name.as_str(), s.state, s.issues))
            .collect();
        assert_eq!(
            states,
            [
                ("Todo", IssueState::Todo, 1),
                ("In Review", IssueState::InReview, 1),
                ("Canceled", IssueState::Canceled, 1)
            ]
        );
        let users: Vec<(&str, Option<&str>)> = (plan.users.iter())
            .map(|u| (u.name.as_str(), u.login.as_deref()))
            .collect();
        assert_eq!(
            users,
            [
                ("Jamie Lin", Some("jlin")),
                ("Priya Shah", None),
                ("Sam Ortiz", None)
            ],
            "the key's owner is the human"
        );
        assert!(
            host.list_issues(Some(&personal.id), None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(git(&folder, &["rev-list", "--count", "HEAD"]), commits);
    }

    #[test]
    fn issues_comments_and_relations_are_written_with_confirmed_logins_and_renumbered() {
        let (_home, mut host, personal, prefix) = personal();
        recorded(&mut host, celadon(vec![]));
        let logins = [("user-jamie", "jlin"), ("user-priya", "pshah")];
        let status = import(&mut host, &personal, &logins, &publish::Tools::default());
        assert!(status.finished && status.error.is_none(), "{status:?}");
        // Linear's numbers keep their order after the tracker's, from the next free one.
        let id = |n: u64| format!("{prefix}-{n}");
        let first = issue(&host, &id(1)).issue.unwrap();
        assert_eq!(first.extra[LINEAR_FIELD], "CEL-1");
        assert_eq!(
            (first.state, first.priority),
            (IssueState::InReview, Priority::Urgent)
        );
        assert_eq!(
            (&*first.author, first.assignee.as_deref()),
            ("pshah", Some("jlin"))
        );
        assert_eq!(
            first.labels,
            ["bug", "webhooks", "p1"],
            "both pages of labels"
        );
        assert_eq!(first.created, "2026-09-01T10:00:00Z");
        assert_eq!(
            (first.blocks.clone(), first.related.clone()),
            (vec![id(2)], vec![id(3)])
        );
        let comments = issue(&host, &id(1)).comments;
        let written: Vec<(&str, &str)> = (comments.iter())
            .map(|c| (c.author.as_str(), c.body.as_str()))
            .collect();
        assert_eq!(
            written,
            [
                (
                    "jlin",
                    "Written in Linear by Sam Ortiz.\n\nSeen on two watches."
                ),
                ("pshah", "Fixed upstream?")
            ],
            "an unmapped author is named on the first line"
        );
        let second = issue(&host, &id(2)).issue.unwrap();
        assert_eq!(&*second.author, "jlin");
        assert_eq!(second.description, "Filed in Linear by Sam Ortiz.");
        assert_eq!(
            (second.blocked_by.clone(), second.duplicate_of),
            (vec![id(1)], Some(id(3)))
        );
        let third = issue(&host, &id(3));
        let fields = third.issue.as_ref().unwrap();
        assert_eq!(fields.state, IssueState::Canceled);
        assert_eq!(
            fields.related,
            [id(1)],
            "from CEL-1's second page; OPS-9 is not here"
        );
        assert_eq!(third.comments.len(), 2, "both pages of comments");
        let folder = host.storage_folder(&personal.storage.repository);
        let subjects = git(&folder, &["log", "--format=%s", "-2"]);
        assert_eq!(
            subjects,
            "Related 3 issues imported from Linear\nImported 3 issues from Linear"
        );
    }

    #[test]
    fn running_it_again_adds_only_the_issues_not_imported_before() {
        let (_home, mut host, personal, prefix) = personal();
        recorded(&mut host, celadon(vec![]));
        import(&mut host, &personal, &[], &publish::Tools::default());
        recorded(&mut host, celadon(vec![plain(4)]));
        begin(&mut host, &personal).unwrap();
        let plan = preview(&mut host, &personal).unwrap();
        assert_eq!((plan.issues, plan.imported_before), (1, 3));
        let logins = BTreeMap::new();
        let tools = publish::Tools::default();
        host.import_from_linear_now(&personal.id, logins, &tools)
            .unwrap();
        let listed = host.list_issues(Some(&personal.id), None).unwrap();
        assert_eq!(listed.len(), 4);
        let fourth = issue(&host, &format!("{prefix}-4")).issue.unwrap();
        assert_eq!(fourth.extra[LINEAR_FIELD], "CEL-4");
    }

    /// Sage on this Mac, its tracker CEL on `github`, with the human signed in as `jlin`.
    fn cel_tracker(github: &Arc<sync::tests::GitHub>) -> (tempfile::TempDir, Host, Space) {
        let (home, mut host) = host();
        host.record_gh_login(Some("jlin".into()));
        join_sage(&mut host, github).unwrap();
        let sage = space_named(&host, "Sage");
        (home, host, sage)
    }

    #[test]
    fn a_linear_number_a_teammate_took_meanwhile_goes_to_the_next_free_one() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("CEL"));
        let (_home, mut host, sage) = cel_tracker(&github);
        recorded(&mut host, many(3));
        begin(&mut host, &sage).unwrap();
        preview(&mut host, &sage).unwrap();
        // A teammate files CEL-1 after this Mac last fetched.
        let theirs = issue_md(&tracker::tests::blurry("CEL-1")).unwrap();
        github.commit("issues/CEL-1/issue.md", &theirs);
        let tools = github.tools();
        (host.import_from_linear_now(&sage.id, BTreeMap::new(), &tools)).unwrap();
        let status = host.linear.status.clone().unwrap();
        assert!(status.error.is_none(), "{status:?}");
        let teammates = github.show("issues/CEL-1/issue.md").unwrap();
        assert_eq!(
            teammates.as_bytes(),
            theirs,
            "left as the teammate wrote it"
        );
        for (number, linear) in [(2, "CEL-2"), (3, "CEL-3"), (4, "CEL-1")] {
            let there = github
                .show(&format!("issues/CEL-{number}/issue.md"))
                .unwrap();
            assert!(there.contains(&format!("\nlinear: {linear}\n")), "{there}");
        }
    }

    #[test]
    fn an_imported_issue_a_scan_holds_stays_here_whole_and_unnumbered() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("CEL"));
        let (_home, mut host, sage) = cel_tracker(&github);
        let mut held = plain(2);
        held["description"] = json!("Call the parent at (415) 555-0134 tonight.");
        held["comments"]["nodes"] =
            json!([{"body": "Seen.", "createdAt": "2026-09-02T09:00:00.000Z", "user": null}]);
        recorded(&mut host, listed(vec![plain(1), held, plain(3)]));
        let status = import(&mut host, &sage, &[], &github.tools());
        assert!(status.finished && status.error.is_none(), "{status:?}");
        let there = git(&github.bare, &["ls-tree", "-r", "--name-only", "main"]);
        assert!(
            !there.contains("CEL-2/"),
            "nothing of it is pushed: {there}"
        );
        for number in [1, 3] {
            assert!(
                there.contains(&format!("issues/CEL-{number}/issue.md")),
                "{there}"
            );
        }
        let here = host.list_issues(Some(&sage.id), None).unwrap();
        let waiting: Vec<&IssueListing> = here.iter().filter(|i| i.id.is_temporary()).collect();
        assert_eq!(waiting.len(), 1, "{here:?}");
        let fields = issue(&host, &waiting[0].id.to_string()).issue.unwrap();
        assert_eq!(fields.extra[LINEAR_FIELD], "CEL-2");
    }

    #[test]
    fn a_rerun_writes_the_relations_an_import_cut_short_never_wrote() {
        let (_home, mut host, personal, prefix) = personal();
        recorded(&mut host, celadon(vec![]));
        begin(&mut host, &personal).unwrap();
        preview(&mut host, &personal).unwrap();
        (host.start_linear_import(&personal.id, BTreeMap::new())).unwrap();
        assert_eq!(host.linear_step(), Step::Next, "the issues are written");
        host.execute(Command::CancelLinearImport).unwrap();
        let first = format!("{prefix}-1");
        assert!(issue(&host, &first).issue.unwrap().blocks.is_empty());
        begin(&mut host, &personal).unwrap();
        let plan = preview(&mut host, &personal).unwrap();
        assert_eq!((plan.issues, plan.relations_to_add), (0, 3));
        let tools = publish::Tools::default();
        (host.import_from_linear_now(&personal.id, BTreeMap::new(), &tools)).unwrap();
        let blocks = issue(&host, &first).issue.unwrap().blocks;
        assert_eq!(blocks, [format!("{prefix}-2")]);
    }

    #[test]
    fn the_key_is_removed_when_the_import_is_canceled_or_stops() {
        let (_home, mut host, personal, _) = personal();
        let (keys, _) = recorded(&mut host, celadon(vec![]));
        begin(&mut host, &personal).unwrap();
        assert!(keys.kept().is_some());
        host.execute(Command::CancelLinearImport).unwrap();
        assert!(keys.kept().is_none());
        assert!(preview(&mut host, &personal).is_err(), "it begins again");

        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("CEL"));
        join_sage(&mut host, &github).unwrap();
        let sage = space_named(&host, "Sage");
        let (keys, _) = recorded(&mut host, many(3));
        begin(&mut host, &sage).unwrap();
        preview(&mut host, &sage).unwrap();
        github
            .offline
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let tools = github.tools();
        (host.import_from_linear_now(&sage.id, BTreeMap::new(), &tools)).unwrap();
        let status = host.linear.status.clone().unwrap();
        assert!(status.finished && status.error.is_some(), "{status:?}");
        assert!(keys.kept().is_none());
    }

    #[test]
    fn the_key_is_removed_when_a_read_fails_or_waits_too_long_for_the_human() {
        let (_home, mut host, personal, _) = personal();
        let (keys, _) = recorded(&mut host, Box::new(|_, _| fixture("unauthorized.json")));
        assert!(begin(&mut host, &personal).is_err());
        assert!(keys.kept().is_none(), "the teams read failed");

        let refuses_the_team = |operation: &str, variables: &Value| match operation {
            "Teams" => teams_page(variables),
            _ => fixture("unauthorized.json"),
        };
        let (keys, _) = recorded(&mut host, Box::new(refuses_the_team));
        begin(&mut host, &personal).unwrap();
        assert!(preview(&mut host, &personal).is_err());
        assert!(keys.kept().is_none(), "the preview read failed");
        assert!(preview(&mut host, &personal).is_err(), "it begins again");

        let (keys, _) = recorded(&mut host, celadon(vec![]));
        begin(&mut host, &personal).unwrap();
        preview(&mut host, &personal).unwrap();
        let ends = host.expire_linear_key(now()).unwrap();
        assert!(keys.kept().is_some(), "kept while the human decides");
        assert_eq!(host.expire_linear_key(ends), None);
        assert!(
            keys.kept().is_none(),
            "the human left it, as when the app quit"
        );
        let start = host.start_linear_import(&personal.id, BTreeMap::new());
        assert!(start.is_err(), "it begins again");
    }

    #[test]
    fn a_rerun_relates_an_issue_imported_before_to_one_imported_now() {
        let (_home, mut host, personal, prefix) = personal();
        recorded(&mut host, listed(vec![plain(1), plain(2)]));
        import(&mut host, &personal, &[], &publish::Tools::default());
        // CEL-1 now blocks a new CEL-3, a relation Linear keeps on CEL-1 alone.
        let mut first = plain(1);
        first["relations"]["nodes"] =
            json!([{"type": "blocks", "relatedIssue": {"identifier": "CEL-3"}}]);
        recorded(&mut host, listed(vec![first, plain(2), plain(3)]));
        import(&mut host, &personal, &[], &publish::Tools::default());
        let id = |n: u64| format!("{prefix}-{n}");
        assert_eq!(issue(&host, &id(1)).issue.unwrap().blocks, [id(3)]);
        assert_eq!(issue(&host, &id(3)).issue.unwrap().blocked_by, [id(1)]);
    }

    /// A Keychain that will not let the key go.
    struct Stuck(Kept);
    impl KeyStore for Stuck {
        fn keep(&self, key: &LinearKey) -> Result<()> {
            self.0.keep(key)
        }
        fn read(&self) -> Result<LinearKey> {
            self.0.read()
        }
        fn remove(&self) -> Result<()> {
            bail!("The Keychain is locked")
        }
    }

    /// What this process writes to its standard error, its own and its children's, while `run`
    /// runs. Nextest runs each test in a process of its own, so no other test writes here.
    fn logged(run: impl FnOnce()) -> String {
        use std::io::{Read, Seek};
        use std::os::fd::AsRawFd;
        let mut file = tempfile::tempfile().unwrap();
        // SAFETY: duplicates and restores this process's own standard error descriptor.
        let saved = unsafe { libc::dup(2) };
        assert!(saved >= 0 && unsafe { libc::dup2(file.as_raw_fd(), 2) } >= 0);
        run();
        // SAFETY: as above.
        unsafe {
            libc::dup2(saved, 2);
            libc::close(saved);
        }
        let mut log = String::new();
        file.rewind().unwrap();
        file.read_to_string(&mut log).unwrap();
        log
    }

    #[test]
    fn neither_the_key_nor_any_issue_or_comment_text_is_logged() {
        let folder = tempfile::tempdir().unwrap();
        let github = shared_storage(folder.path(), Some("CEL"));
        let (_home, mut host, sage) = cel_tracker(&github);
        let mut told = plain(1);
        told["title"] = json!("Title only Linear knows");
        told["description"] = json!("Call the parent at (415) 555-0134 tonight.");
        told["comments"]["nodes"] = json!([{
            "body": "Comment only Linear knows",
            "createdAt": "2026-09-02T09:00:00.000Z",
            "user": {"id": "user-sam", "name": "Sam Ortiz", "email": "sam@example.com"},
        }]);
        recorded(&mut host, listed(vec![told.clone(), plain(2)]));
        host.linear.tools.keys = Arc::new(Stuck(Kept::default()));
        let log = logged(|| {
            // A scan holds CEL-1, and the key cannot be removed.
            import(&mut host, &sage, &[], &github.tools());
            // Linear refuses, then a push fails.
            begin(&mut host, &sage).unwrap();
            recorded(&mut host, Box::new(|_, _| fixture("unauthorized.json")));
            host.linear.tools.keys = Arc::new(Stuck(Kept::default()));
            assert!(preview(&mut host, &sage).is_err());
            recorded(&mut host, listed(vec![told, plain(2), plain(3)]));
            host.linear.tools.keys = Arc::new(Stuck(Kept::default()));
            github
                .offline
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let status = import(&mut host, &sage, &[], &github.tools());
            assert!(status.error.is_some(), "{status:?}");
        });
        assert!(
            log.contains("the key was not removed"),
            "logs are read: {log}"
        );
        let secrets = [
            KEY,
            "Title only Linear knows",
            "555-0134",
            "Comment only Linear knows",
            "Sam Ortiz",
            "sam@example.com",
        ];
        for secret in secrets {
            assert!(!log.contains(secret), "{secret} was logged: {log}");
        }
    }

    #[test]
    fn nothing_but_a_query_is_ever_sent_to_linear() {
        let api = Recorded {
            answer: Box::new(|_, _| json!({"data":{}})),
            sent: Mutex::default(),
        };
        let key = LinearKey(KEY.into());
        let write = "mutation Archive { issueArchive(id: \"issue-1\") { success } }";
        assert!(read(&api, &key, write, json!({})).is_err());
        assert!(api.documents().is_empty());
    }

    #[test]
    fn linears_refusal_is_its_reason_and_no_login_or_a_malformed_key_keeps_nothing() {
        let (_home, mut host, personal, _) = personal();
        let (keys, _) = recorded(&mut host, Box::new(|_, _| fixture("unauthorized.json")));
        let refused = begin(&mut host, &personal).unwrap_err().to_string();
        assert!(refused.contains("Authentication required"), "{refused}");
        host.execute(Command::CancelLinearImport).unwrap();
        let space_id = personal.id.clone();
        let quoted = LinearKey("lin_api\"\nurl = x".into());
        assert!(
            host.execute(Command::LinearTeams {
                space_id,
                key: quoted
            })
            .is_err()
        );
        host.record_gh_login(None);
        assert!(begin(&mut host, &personal).is_err());
        assert!(keys.kept().is_none());
    }
}
