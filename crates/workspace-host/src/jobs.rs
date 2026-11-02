//! Scheduled jobs (stage 3): recipes the user saved, the scheduler that runs them and built-in
//! host jobs on their due times, one catch-up run after missed ones, the check of a run's
//! sources, and run records. A recipe is a file in its Space's storage; the store keeps each as
//! the user saved it, or as sync pulled it from another member's save, and only that copy runs.
//! Shared jobs (stage 5) run on their runner's Mac, or on every member's, and a runner's output
//! is pushed only once this Mac approved the prompt and sources it ran with. Jobs belong to the
//! user: no tool, thread or run saves, enables, takes over or approves one.
use crate::*;
use anyhow::anyhow;
use chrono::{Datelike, Duration as Days, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::Deserialize;
use workspace_core::assistant::{JobRunOf, Thread};
use workspace_core::board::plain_file_name;
use workspace_core::job::{
    Cron, JobKey, JobRun, JobState, MAX_RUNS_SHOWN, RECIPE_FORMAT, Recipe, RecipePreview,
    RunOutcome, RunOutput, Runner, Schedule, Sharing, SourceCheck, source_label,
};

/// A due time passed by more than this when the scheduler reaches it is a catch-up.
const LATE_AFTER_MS: i64 = 60_000;
/// How long a source's sign-in check, or an MCP server's handshake, may take.
const CHECK_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
/// The next runs a recipe card and a job's page show.
const NEXT_SHOWN: usize = 3;
/// How far ahead a schedule is searched for its next time: past every Feb 29.
const SEARCH_DAYS: i64 = 8 * 366;
/// Missed times are counted up to this many; a catch-up for more says this many.
const MAX_COUNTED: u32 = 10_000;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS jobs (key TEXT PRIMARY KEY, space_id TEXT NOT NULL, data TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS job_due (key TEXT PRIMARY KEY, next_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS job_runs (id TEXT PRIMARY KEY, key TEXT NOT NULL, started_at INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS job_runs_by_key ON job_runs(key, started_at);
         CREATE TABLE IF NOT EXISTS job_approvals (key TEXT PRIMARY KEY, approved TEXT NOT NULL);",
    )?;
    Ok(())
}

/// Whether this Mac is the runner of a "One runner" job.
pub(crate) fn runs_here(recipe: &Recipe) -> bool {
    recipe.sharing == Sharing::OneRunner
        && (recipe.runner.as_ref()).is_some_and(|runner| runner.machine == sync::machine_name())
}

/// `spaces/<folder>/boards/<id>`: a job's outputs, relative to its repository's folder.
pub(crate) fn board_folder(folder: &str, id: &str) -> String {
    format!("spaces/{folder}/boards/{id}")
}

/// What the runner approves by sharing an output: the recipe's prompt and sources.
fn approval(recipe: &Recipe) -> String {
    use sha2::{Digest, Sha256};
    let approved = json!({"prompt": recipe.prompt, "sources": recipe.sources});
    workspace_core::pairing::hex(&Sha256::digest(approved.to_string().as_bytes()))
}

/// A job Wiffletree defines, such as a brain's compile: it takes due times, catch-up, Run now
/// and run records from the scheduler, and keeps its own owner and run. It is no
/// recipe and in no Space; only the host's code registers one.
pub(crate) struct BuiltIn {
    /// Unique among built-in jobs, such as `brain-compile:<brain>`.
    pub(crate) name: String,
    /// What it works on, such as a brain's `owner/name`, which its functions are given.
    pub(crate) subject: String,
    /// Who it belongs to, such as "the Sage brain".
    pub(crate) owner: String,
    /// Its due times; with none, only Run now starts it.
    pub(crate) schedule: Option<Schedule>,
    /// Runs it on its subject by its own rules for sessions and models, and says what it did.
    pub(crate) run: fn(&mut Host, &str, BuiltInStart) -> Result<String>,
}
impl BuiltIn {
    pub(crate) fn key(&self) -> JobKey {
        JobKey::built_in(&self.name)
    }
}

/// Why a built-in job's run started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BuiltInStart {
    /// At its due time; `catch_up` is how many missed times the run stands for, 0 on time, and
    /// `due` names the latest of them, as every Mac due then names it.
    Due {
        catch_up: u32,
        due: String,
    },
    RunNow,
}

/// What one pass of the scheduler did.
pub(crate) struct JobPass {
    /// The earliest due time ahead.
    pub(crate) next: Option<i64>,
    pub(crate) changed: bool,
}

pub(crate) enum RunEnd {
    Finished,
    Failed(String),
    Stopped(String),
}

/// `spaces/<folder>/jobs/<id>.yaml`, relative to the repository's folder.
pub(crate) fn recipe_file(folder: &str, id: &str) -> String {
    format!("spaces/{folder}/jobs/{id}.yaml")
}

/// A recipe's file as written: the recipe without its id, which is the file's name.
#[derive(Serialize, Deserialize)]
struct RecipeFile {
    format: u32,
    name: String,
    space: SpaceId,
    prompt: String,
    schedule: Schedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<ModelProfile>,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<workspace_core::job::OutputKind>,
    #[serde(default)]
    sharing: Sharing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runner: Option<Runner>,
    enabled: bool,
    #[serde(flatten)]
    extra: workspace_core::Extra,
}

pub(crate) fn recipe_yaml(recipe: &Recipe) -> Result<Vec<u8>> {
    let file = RecipeFile {
        format: RECIPE_FORMAT,
        name: recipe.name.clone(),
        space: recipe.space_id.clone(),
        prompt: recipe.prompt.clone(),
        schedule: recipe.schedule.clone(),
        model: recipe.model.clone(),
        sources: recipe.sources.clone(),
        output: recipe.output.clone(),
        sharing: recipe.sharing,
        runner: recipe.runner.clone(),
        enabled: recipe.enabled,
        extra: recipe.extra.clone(),
    };
    Ok(serde_yaml::to_string(&file)?.into_bytes())
}

/// The recipe a `jobs/<id>.yaml` file of `space` holds, refused when it names another Space or
/// cannot run as written.
pub(crate) fn read_recipe(content: &[u8], space: &SpaceId, id: &str) -> Result<Recipe> {
    let file: RecipeFile =
        serde_yaml::from_slice(content).context("The job's file can't be read")?;
    ensure!(
        file.format == RECIPE_FORMAT,
        "This job was written by a newer Wiffletree. Update to use it."
    );
    ensure!(&file.space == space, "The job's file names another Space");
    let recipe = Recipe {
        id: id.into(),
        name: file.name,
        space_id: file.space,
        prompt: file.prompt,
        schedule: file.schedule,
        model: file.model,
        sources: file.sources,
        output: file.output,
        sharing: file.sharing,
        runner: file.runner,
        enabled: file.enabled,
        extra: file.extra,
    };
    validate(&recipe)?;
    Ok(recipe)
}

/// The recipe's own checks, and that its timezone is an IANA name.
pub(crate) fn validate(recipe: &Recipe) -> Result<()> {
    workspace_core::job::validate(recipe)?;
    timezone(&recipe.schedule.timezone)?;
    Ok(())
}

pub(crate) fn timezone(name: &str) -> Result<Tz> {
    name.parse::<Tz>()
        .map_err(|_| anyhow!("{name:?} is not a timezone; use a name such as America/New_York"))
}

/// This Mac's timezone, which a recipe that names none takes once, when it is shown.
pub(crate) fn local_timezone() -> String {
    let zone = fs::read_link("/etc/localtime").ok().and_then(|link| {
        let link = link.to_string_lossy().into_owned();
        let (_, name) = link.split_once("zoneinfo/")?;
        name.parse::<Tz>().is_ok().then(|| name.to_owned())
    });
    zone.unwrap_or_else(|| "UTC".into())
}

/// The first time after `after` that `cron` names in `zone`. A local time a daylight-saving
/// change skips runs at the next valid minute, and one it repeats runs once, at its first.
pub(crate) fn next_after(cron: &Cron, zone: Tz, after: i64) -> Option<i64> {
    let start = zone.timestamp_millis_opt(after).single()?.date_naive();
    (0..SEARCH_DAYS)
        .map(|offset| start + Days::days(offset))
        .filter(|date| {
            cron.runs_on(
                date.day(),
                date.month(),
                date.weekday().num_days_from_sunday(),
            )
        })
        .flat_map(|date| {
            cron.times()
                .filter_map(move |(h, m)| instant(zone, date, h, m))
        })
        .find(|at| *at > after)
}

/// The instant of a local time in `zone`: the earlier of a repeated time, or for one a change
/// skipped, the first valid minute after it.
fn instant(zone: Tz, date: NaiveDate, hour: u32, minute: u32) -> Option<i64> {
    let local = date.and_time(NaiveTime::from_hms_opt(hour, minute, 0)?);
    (0..24 * 60)
        .map(|later| local + Days::minutes(later))
        .find_map(|time| zone.from_local_datetime(&time).earliest())
        .map(|at| at.timestamp_millis())
}

/// How many times `cron` names from `from` through `to`, up to `MAX_COUNTED`.
pub(crate) fn times_between(cron: &Cron, zone: Tz, from: i64, to: i64) -> u32 {
    let mut count = 0;
    let mut at = from - 1;
    while count < MAX_COUNTED {
        match next_after(cron, zone, at) {
            Some(next) if next <= to => {
                count += 1;
                at = next;
            }
            _ => break,
        }
    }
    count
}

/// The last time `cron` names in `zone` from `from`, a time it names, up to `to`.
fn latest_time(cron: &Cron, zone: Tz, from: i64, to: i64) -> i64 {
    let mut at = from;
    while let Some(next) = next_after(cron, zone, at).filter(|next| *next <= to) {
        at = next;
    }
    at
}

/// A scheduled time as a claim names it: `2026-11-02T06:00 America/Chicago`.
fn due_name(at: i64, zone: Tz) -> String {
    let local = zone.timestamp_millis_opt(at).single();
    let local = local.map(|local| local.format("%Y-%m-%dT%H:%M").to_string());
    format!("{} {}", local.unwrap_or_default(), zone.name())
}

/// Whether `schedule` names the time `at`.
pub(crate) fn names_time(schedule: &Schedule, at: i64) -> Result<bool> {
    let (cron, zone) = (Cron::parse(&schedule.cron)?, timezone(&schedule.timezone)?);
    Ok(next_after(&cron, zone, at - 1) == Some(at))
}

/// How many times `schedule` named after `from`, up to `to`.
pub(crate) fn times_since(schedule: &Schedule, from: i64, to: i64) -> Result<u32> {
    let (cron, zone) = (Cron::parse(&schedule.cron)?, timezone(&schedule.timezone)?);
    Ok(times_between(&cron, zone, from + 1, to))
}

/// The next `count` times after `after`, as the recipe card shows them in the job's zone.
fn next_times(recipe: &Recipe, after: i64, count: usize) -> Vec<String> {
    let (Ok(cron), Ok(zone)) = (
        Cron::parse(&recipe.schedule.cron),
        timezone(&recipe.schedule.timezone),
    ) else {
        return vec![];
    };
    std::iter::successors(next_after(&cron, zone, after), |at| {
        next_after(&cron, zone, *at)
    })
    .take(count)
    .filter_map(|at| zone.timestamp_millis_opt(at).single())
    .map(|at| at.format("%a %b %-d %H:%M").to_string())
    .collect()
}

/// "Weekdays at 07:00 (America/New_York)".
fn schedule_words(schedule: &Schedule) -> String {
    match Cron::parse(&schedule.cron) {
        Ok(cron) => format!("{} ({})", cron.words(&schedule.cron), schedule.timezone),
        Err(error) => format!("{error:#}"),
    }
}

/// Where a source's CLI is looked for and checked, the home folder whose provider
/// configuration names MCP servers, and how long an MCP server may take to answer.
#[derive(Clone)]
pub(crate) struct SourceEnv {
    pub(crate) path: Vec<PathBuf>,
    pub(crate) home: PathBuf,
    pub(crate) mcp_limit: std::time::Duration,
}
impl SourceEnv {
    /// The host's own: its `PATH` and the folders the provider CLIs are found in.
    fn of_host() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let mut folders: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .chain([
                    home.join(".local/bin"),
                    "/opt/homebrew/bin".into(),
                    "/usr/local/bin".into(),
                ])
                .collect();
        folders.dedup();
        Self {
            path: folders,
            home,
            mcp_limit: CHECK_LIMIT,
        }
    }
    fn find(&self, command: &str) -> Option<PathBuf> {
        (self.path.iter())
            .map(|folder| folder.join(command))
            .find(|path| path.is_file())
    }
}

/// The sign-in check of a CLI Wiffletree knows, and the command that fixes a failed one.
fn known_check(command: &str, prefixes: &[String]) -> Option<(Vec<String>, String)> {
    let words = |text: &str| text.split_whitespace().map(str::to_owned).collect();
    match command {
        "gh" => Some((words("gh auth status"), "gh auth login".into())),
        "linctl" => Some((words("linctl auth status"), "linctl auth login".into())),
        "sentry-cli" => Some((words("sentry-cli info"), "sentry-cli login".into())),
        "aws" => {
            let profile = aws_profile(prefixes)?;
            Some((
                words(&format!("aws sts get-caller-identity --profile {profile}")),
                format!("aws sso login --profile {profile}"),
            ))
        }
        _ => None,
    }
}

/// The read-only profile every `aws` command of a source names with `--profile`.
fn aws_profile(prefixes: &[String]) -> Option<String> {
    let profiles: Vec<&str> = (prefixes.iter())
        .map(|prefix| {
            let words: Vec<&str> = prefix.split_whitespace().collect();
            let at = words.iter().position(|w| *w == "--profile")?;
            words.get(at + 1).copied()
        })
        .collect::<Option<_>>()?;
    let first = *profiles.first()?;
    profiles
        .iter()
        .all(|p| *p == first)
        .then(|| first.to_owned())
}

/// What keeps an MCP server from serving a run, in words with none of its output: started as
/// its provider would start it, it must answer `initialize` and list every tool the recipe
/// names, within the limit. A server reached by URL cannot be checked, so it is refused.
fn mcp_problem(env: &SourceEnv, definition: &Value, tools: &[String]) -> Option<String> {
    use std::io::{BufRead, Write};
    use std::os::unix::process::CommandExt;
    let Some(command) = definition["command"].as_str().filter(|c| !c.is_empty()) else {
        return Some("is not a command Wiffletree can start to check it; a job can use only a command-line MCP server".into());
    };
    let program = match command.contains('/') {
        true => Some(PathBuf::from(command)),
        false => env.find(command),
    };
    let Some(program) = program else {
        return Some(format!(
            "names {command}, which is not installed on this Mac"
        ));
    };
    let args = (definition["args"].as_array().into_iter().flatten()).filter_map(Value::as_str);
    let mut start = std::process::Command::new(program);
    start
        .args(args)
        .env("PATH", std::env::join_paths(&env.path).unwrap_or_default())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    for (key, value) in definition["env"].as_object().into_iter().flatten() {
        if let Some(value) = value.as_str() {
            start.env(key, value);
        }
    }
    let Ok(mut child) = start.spawn() else {
        return Some("could not be started".into());
    };
    let group = child.id() as libc::pid_t;
    let (mut stdin, stdout) = (child.stdin.take()?, child.stdout.take()?);
    let (lines, answers) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && lines.send(value).is_err()
            {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + env.mcp_limit;
    let mut answer = |id: i64, message: Value, then: &[Value]| -> Result<Value, String> {
        let mut send = |message: &Value| writeln!(stdin, "{message}").and_then(|_| stdin.flush());
        send(&message).map_err(|_| "stopped before it answered".to_owned())?;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match answers.recv_timeout(left) {
                Ok(value) if value["id"] == id => {
                    if !value["error"].is_null() {
                        return Err(
                            "answered with an error, such as a sign-in that has expired".into()
                        );
                    }
                    for next in then {
                        send(next).map_err(|_| "stopped before it answered".to_owned())?;
                    }
                    return Ok(value["result"].clone());
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "did not answer within {} seconds",
                        env.mcp_limit.as_secs()
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("stopped before it answered".into());
                }
            }
        }
    };
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wiffletree","version":"1"}}});
    let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let checked = answer(1, initialize, &[initialized]).and_then(|_| answer(2, list, &[]));
    // SAFETY: `killpg` only sends a signal; the group is the server's own.
    unsafe { libc::killpg(group, libc::SIGKILL) };
    let _ = child.wait();
    let offered = match checked {
        Ok(listed) => listed,
        Err(why) => return Some(why),
    };
    let names: Vec<&str> = (offered["tools"].as_array().into_iter().flatten())
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    let missing: Vec<&str> = (tools.iter().map(String::as_str))
        .filter(|tool| !names.contains(tool))
        .collect();
    (!missing.is_empty()).then(|| format!("does not offer {}", missing.join(", ")))
}

/// Checks one source as a run on `provider` would use it. An MCP server must be in that
/// provider's configuration, start, finish its handshake and offer the listed tools. A CLI
/// must be installed and pass Wiffletree's own sign-in check for it; nothing a recipe supplies
/// is run, apart from the aws profile name, a plain word the allowlist checked.
fn check_source(provider: Provider, env: &SourceEnv, source: &Source) -> SourceCheck {
    let label = source_label(source);
    let problem = match source {
        Source::Mcp { server, tools, .. } => {
            match provider::server_definition(provider, &env.home, server) {
                None => {
                    let who = provider.label();
                    Some(format!(
                        "The {server} MCP server is not configured for {who}. Add it to {who}'s MCP configuration."
                    ))
                }
                Some(definition) => mcp_problem(env, &definition, tools)
                    .map(|why| format!("The {server} MCP server {why}.")),
            }
        }
        Source::Cli {
            command, prefixes, ..
        } => match env.find(command) {
            None => Some(format!("{command} is not installed on this Mac.")),
            Some(program) => {
                let Some((args, fix)) = known_check(command, prefixes) else {
                    return SourceCheck {
                        source: label,
                        problem: Some(format!(
                            "Wiffletree cannot check {command}; a job reads only through {}.",
                            workspace_core::job::JOB_CLIS.join(", ")
                        )),
                    };
                };
                let fix = format!("Run `{fix}`.");
                let mut run = std::process::Command::new(program);
                run.args(&args[1..])
                    .env("PATH", std::env::join_paths(&env.path).unwrap_or_default());
                let passed = runtime::run_limited(run, CHECK_LIMIT)
                    .ok()
                    .flatten()
                    .is_some_and(|status| status.success());
                (!passed).then(|| format!("{command} is not signed in on this Mac. {fix}"))
            }
        },
    };
    SourceCheck {
        source: label,
        problem,
    }
}

/// The checks of a run's or a card's sources, which run off the host's thread: each MCP
/// handshake and sign-in check may take up to `CHECK_LIMIT`.
pub(crate) struct SourceChecks {
    provider: Provider,
    env: SourceEnv,
    sources: Vec<Source>,
}
impl SourceChecks {
    pub(crate) fn run(&self) -> Vec<SourceCheck> {
        (self.sources.iter())
            .map(|source| check_source(self.provider, &self.env, source))
            .collect()
    }
}

/// A recipe card's preview, waiting for its sources' checks.
pub(crate) struct PendingPreview {
    preview: RecipePreview,
    checks: SourceChecks,
}
impl PendingPreview {
    pub(crate) fn check(mut self) -> RecipePreview {
        self.preview.sources = self.checks.run();
        self.preview
    }
}

impl Host {
    /// The built-in host jobs, which only the host's code adds.
    pub(crate) fn built_in(&self, key: &JobKey) -> Option<&BuiltIn> {
        self.built_ins.iter().find(|b| &b.key() == key)
    }
    pub(crate) fn recipes(&self) -> Result<Vec<Recipe>> {
        self.list_data("SELECT data FROM jobs ORDER BY space_id, key", [])
    }
    pub fn recipe(&self, key: &JobKey) -> Result<Recipe> {
        self.db
            .query_row("SELECT data FROM jobs WHERE key=?1", [key.as_str()], |r| {
                decode(r, 0)
            })
            .optional()?
            .context("Job not found")
    }
    fn source_env(&self) -> SourceEnv {
        (self.source_env.clone()).unwrap_or_else(SourceEnv::of_host)
    }
    pub(crate) fn source_checks(&self, provider: Provider, sources: &[Source]) -> SourceChecks {
        SourceChecks {
            provider,
            env: self.source_env(),
            sources: sources.to_vec(),
        }
    }
    /// The runs whose sources wait to be checked, for the service to check off the host's
    /// thread and hand back to `sources_checked`.
    pub(crate) fn take_source_checks(&mut self) -> Vec<(String, SourceChecks)> {
        std::mem::take(&mut self.source_checks)
    }
    /// Why this Mac does not run a recipe: a "One runner" job in a Space stored on GitHub
    /// runs only on its runner's Mac.
    fn not_here(&self, recipe: &Recipe, space: &Space) -> Option<String> {
        if matches!(space.storage.repository, StorageRepository::Local { .. })
            || recipe.sharing == Sharing::Everyone
        {
            return None;
        }
        match &recipe.runner {
            _ if runs_here(recipe) => None,
            Some(runner) => Some(runner.label()),
            None => Some("No Mac is its runner".into()),
        }
    }
    /// The prompt and sources this Mac approved for a job by sharing one of its outputs.
    fn approved(&self, key: &JobKey) -> Result<Option<String>> {
        Ok((self.db)
            .query_row(
                "SELECT approved FROM job_approvals WHERE key=?1",
                [key.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    }
    /// The model a run of `recipe` starts on: the recipe's when this Mac may use it, else Who
    /// runs what's Job row, or the Assistant row without one, as the recipe's Space resolves
    /// them. A recipe with a CLI source runs only on Claude, which can hold the CLI to its
    /// listed commands; Codex cannot yet (design, decision 9), so a Space row of Codex choices
    /// falls back to this Mac's.
    pub(crate) fn job_model(&self, recipe: &Recipe) -> Result<(ModelProfile, String, bool)> {
        let space = Some(&recipe.space_id);
        let cli = (recipe.sources.iter()).find_map(|source| match source {
            Source::Cli { command, .. } => Some(command.clone()),
            Source::Mcp { .. } => None,
        });
        let codex_note = |cli: &str| format!("Codex cannot yet hold {cli} to read commands");
        let mut notes = vec![];
        if let Some(profile) = recipe.model.clone() {
            let reason = "Chosen in the recipe";
            match (self.may_use(&profile), &cli) {
                (false, _) => notes.push(format!(
                    "{reason}, {}, is not available on this Mac.",
                    profile.short()
                )),
                (true, Some(cli)) if profile.provider != Provider::Claude => notes.push(format!(
                    "{reason}, {}, is on Codex, and {}.",
                    profile.short(),
                    codex_note(cli)
                )),
                (true, _) => return Ok((profile, reason.into(), false)),
            }
        }
        let (quotas, mut unusable, switch_at) = self.limits()?;
        // Codex signed in here still cannot run a recipe with a CLI source.
        let held = cli.is_some() && unusable.insert(Provider::Codex);
        let mut table = self.space_table(space, &unusable)?;
        if let (true, Some(cli)) = (held, &cli) {
            table = table.because(Provider::Codex, codex_note(cli));
        }
        let selection = &table.selection;
        let row = (selection.row(&Job::Job)).or_else(|| selection.row(&Job::Assistant));
        let row_name = match selection.row(&Job::Job) {
            Some(_) => "Job row",
            None => "Assistant row",
        };
        if let (Some(cli), Some(row)) = (&cli, row) {
            ensure!(
                row.main.choices().any(|p| p.provider == Provider::Claude),
                "{}, and the {row_name} has no Claude choice.",
                codex_note(cli)
            );
        }
        let limits = Limits {
            quotas: &quotas,
            unusable: &unusable,
            switch_at,
            now: now(),
        };
        let pick = table.resolve(&Job::Job, false, false, &limits, &policies::reset_time)?;
        notes.push(format!("{row_name}: {}", pick.reason));
        Ok((pick.profile, notes.join(" "), pick.waits))
    }
    /// What a recipe card shows: its schedule in words, its next runs, its model, and once
    /// checked, its sources as the provider it would run on finds them.
    pub(crate) fn preview_job(&self, recipe: &Recipe) -> Result<PendingPreview> {
        validate(recipe)?;
        let (provider, model) = match self.job_model(recipe) {
            Ok((profile, reason, _)) => {
                (profile.provider, format!("{} · {reason}", profile.short()))
            }
            Err(error) => (Provider::Claude, format!("{error:#}")),
        };
        Ok(PendingPreview {
            preview: RecipePreview {
                schedule: schedule_words(&recipe.schedule),
                next_runs: next_times(recipe, now(), NEXT_SHOWN),
                sources: vec![],
                model,
            },
            checks: self.source_checks(provider, &recipe.sources),
        })
    }
    /// Saves a job as the user confirmed it: the one way a job is created or changed. A "One
    /// runner" job in a Space stored on GitHub names this Mac as its runner until someone
    /// takes over. A saved schedule runs from its next time.
    pub fn save_job(&mut self, mut recipe: Recipe) -> Result<JobState> {
        let record = self.space(&recipe.space_id)?;
        ensure!(
            record.local.enabled,
            "{} is off on this Mac",
            record.space.name
        );
        validate(&recipe)?;
        let stored = !matches!(
            record.space.storage.repository,
            StorageRepository::Local { .. }
        );
        if stored && recipe.sharing == Sharing::OneRunner && recipe.runner.is_none() {
            recipe.runner = Some(Runner {
                login: self.gh_login.clone().unwrap_or_default(),
                machine: sync::machine_name(),
            });
        }
        let existed = self.recipe(&recipe.key()).is_ok();
        let message = match existed {
            true => format!("{}: changed the job {}", record.space.name, recipe.name),
            false => format!("{}: added the job {}", record.space.name, recipe.name),
        };
        self.store_recipe(&record.space, &recipe, &message)?;
        self.job_state(&recipe)
    }
    /// Keeps a recipe in the store and writes its file to its Space's storage, to run from its
    /// next time.
    fn store_recipe(&mut self, space: &Space, recipe: &Recipe, message: &str) -> Result<()> {
        let key = recipe.key();
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO jobs VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET data=excluded.data",
                params![key.as_str(), recipe.space_id.as_str(), encode(recipe)?],
            )?;
            host.db
                .execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?;
            host.write_recipe_file(space, &recipe.id, Some(&recipe_yaml(recipe)?), message)
        })
    }
    /// Take over running: this Mac becomes the runner of a "One runner" job in a Space stored on
    /// GitHub, as a commit to its recipe, and runs it from its next time. Its first output here
    /// waits for this Mac's approval, as a new job's does.
    pub fn take_over_job(&mut self, space: &SpaceId, id: &str) -> Result<JobState> {
        let mut recipe = self.recipe(&JobKey::recipe(space, id))?;
        let record = self.space(space)?;
        ensure!(
            record.local.enabled,
            "{} is off on this Mac",
            record.space.name
        );
        ensure!(
            recipe.sharing == Sharing::OneRunner && self.not_here(&recipe, &record.space).is_some(),
            "This Mac already runs {}, or every member runs their own",
            recipe.name
        );
        let machine = sync::machine_name();
        recipe.runner = Some(Runner {
            login: self.gh_login.clone().unwrap_or_default(),
            machine: machine.clone(),
        });
        let message = format!(
            "{}: {machine} took over running {}",
            record.space.name, recipe.name
        );
        self.store_recipe(&record.space, &recipe, &message)?;
        (self.db).execute(
            "DELETE FROM job_approvals WHERE key=?1",
            [recipe.key().as_str()],
        )?;
        self.job_state(&recipe)
    }
    /// Refuses a new job whose name another job in its Space already has, so a card never
    /// replaces one.
    pub(crate) fn refuse_taken_job(&self, recipe: &Recipe) -> Result<()> {
        if let Ok(taken) = self.recipe(&recipe.key()) {
            let space = self.space(&recipe.space_id)?.space.name;
            bail!(
                "A job named {} already exists in {space}. Change it on its page, or give this one another name.",
                taken.name
            );
        }
        Ok(())
    }
    /// Pauses a job, or resumes it from its next time: what it missed while paused does not run.
    pub fn set_job_enabled(
        &mut self,
        space: &SpaceId,
        id: &str,
        enabled: bool,
    ) -> Result<JobState> {
        let mut recipe = self.recipe(&JobKey::recipe(space, id))?;
        recipe.enabled = enabled;
        self.save_job(recipe)
    }
    /// Removes a job's recipe, its runs and their threads.
    pub fn delete_job(&mut self, space: &SpaceId, id: &str) -> Result<()> {
        let key = JobKey::recipe(space, id);
        let recipe = self.recipe(&key)?;
        let runs = self.job_runs(&key, usize::MAX)?;
        ensure!(
            !runs.iter().any(|run| run.outcome == RunOutcome::Running),
            "Stop the job's run before deleting it"
        );
        let record = self.space(space)?;
        let message = format!("{}: removed the job {}", record.space.name, recipe.name);
        self.atomically(|host| {
            for table in ["jobs", "job_due", "job_runs", "job_approvals"] {
                host.db
                    .execute(&format!("DELETE FROM {table} WHERE key=?1"), [key.as_str()])?;
            }
            host.write_recipe_file(&record.space, id, None, &message)
        })?;
        for thread in runs.iter().filter_map(|run| run.thread_id.as_deref()) {
            if let Err(error) = self.delete_thread(thread) {
                eprintln!("Removing a deleted job's run: {error:#}");
            }
        }
        Ok(())
    }
    /// A pulled recipe file of a Space on this Mac, taken into the store, or its job removed
    /// when the file is gone.
    pub(crate) fn take_pulled_recipe(
        &mut self,
        space: &Space,
        id: &str,
        content: Option<&[u8]>,
    ) -> Result<()> {
        let key = JobKey::recipe(&space.id, id);
        let Some(content) = content else {
            for table in ["jobs", "job_due", "job_approvals"] {
                self.db
                    .execute(&format!("DELETE FROM {table} WHERE key=?1"), [key.as_str()])?;
            }
            return Ok(());
        };
        let recipe = read_recipe(content, &space.id, id)?;
        self.db.execute(
            "INSERT INTO jobs VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET data=excluded.data",
            params![key.as_str(), space.id.as_str(), encode(&recipe)?],
        )?;
        self.db
            .execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?;
        Ok(())
    }
    pub(crate) fn job_runs(&self, key: &JobKey, limit: usize) -> Result<Vec<JobRun>> {
        self.list_data(
            "SELECT data FROM job_runs WHERE key=?1 ORDER BY started_at DESC, rowid DESC LIMIT ?2",
            params![key.as_str(), limit.min(i64::MAX as usize) as i64],
        )
    }
    pub fn job_run(&self, id: &str) -> Result<JobRun> {
        self.db
            .query_row("SELECT data FROM job_runs WHERE id=?1", [id], |r| {
                decode(r, 0)
            })
            .optional()?
            .context("Run not found")
    }
    fn save_run(&self, run: &JobRun) -> Result<()> {
        self.db.execute(
            "INSERT INTO job_runs VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![run.id, run.job.as_str(), run.started_at, encode(run)?],
        )?;
        Ok(())
    }
    fn job_state(&self, recipe: &Recipe) -> Result<JobState> {
        let space = self.space(&recipe.space_id)?.space;
        let (model, profile) = match self.job_model(recipe) {
            Ok((profile, reason, _)) => (format!("{} · {reason}", profile.short()), Some(profile)),
            Err(error) => (format!("{error:#}"), None),
        };
        // Counted apart from the runs shown, so an old unseen failure still counts.
        let (attention, attention_at): (usize, Option<i64>) = self.db.query_row(
            "SELECT count(*), max(started_at) FROM job_runs WHERE key=?1 AND json_extract(data,'$.seen')=0 AND (json_extract(data,'$.outcome') IN ('failed','not_started') OR EXISTS (SELECT 1 FROM json_each(data,'$.outputs') WHERE json_extract(value,'$.waiting')=1))",
            [recipe.key().as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let runs = self.job_runs(&recipe.key(), MAX_RUNS_SHOWN)?;
        let not_here = self.not_here(recipe, &space);
        let last_run_at = match not_here {
            None => runs.first().map(|run| run.started_at),
            Some(_) => self.last_stored_output(recipe, &space),
        };
        Ok(JobState {
            last_run_at,
            attention,
            attention_at,
            schedule: schedule_words(&recipe.schedule),
            next_runs: match recipe.enabled {
                true => next_times(recipe, now(), NEXT_SHOWN),
                false => vec![],
            },
            model,
            profile,
            runs,
            not_here,
            recipe: recipe.clone(),
        })
    }
    /// When the newest output of a job in its Space's storage was saved, as its metadata says.
    fn last_stored_output(&self, recipe: &Recipe, space: &Space) -> Option<i64> {
        let storage = self.root_folder(&space.storage.repository).ok()?;
        let folder = storage.join(board_folder(&space.storage.folder, &recipe.id));
        (fs::read_dir(folder).ok()?.flatten())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let about: Value = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
                about["saved_at"].as_i64()
            })
            .max()
    }
    /// Every saved job in a Space on this Mac, for the snapshot.
    pub(crate) fn job_states(&self) -> Result<Vec<JobState>> {
        let mut states = vec![];
        for recipe in self.recipes()? {
            if self.find_space(&recipe.space_id)?.is_some() {
                states.push(self.job_state(&recipe)?);
            }
        }
        Ok(states)
    }
    /// Marks a job's ended runs seen: they no longer count on the assistant's row.
    pub fn see_job_runs(&mut self, key: &JobKey) -> Result<()> {
        for mut run in self.job_runs(key, usize::MAX)? {
            if !run.seen && run.outcome != RunOutcome::Running {
                run.seen = true;
                self.save_run(&run)?;
            }
        }
        Ok(())
    }
    /// One pass of the scheduler at `now`: each enabled job this Mac runs, and each built-in
    /// job, whose due time has come runs once, a catch-up for every time it missed while the
    /// host was stopped or the Mac slept. A job with a run still running skips its time.
    pub(crate) fn fire_due_jobs(&mut self, now: i64) -> Result<JobPass> {
        let mut pass = JobPass {
            next: None,
            changed: false,
        };
        let mut due: Vec<(JobKey, Schedule)> = vec![];
        for recipe in self.recipes()? {
            let Some(record) = self.find_space(&recipe.space_id)? else {
                continue;
            };
            if recipe.enabled
                && record.local.enabled
                && self.not_here(&recipe, &record.space).is_none()
            {
                due.push((recipe.key(), recipe.schedule.clone()));
            }
        }
        if let Err(error) = self.refresh_compile_jobs() {
            eprintln!("Brain compile jobs: {error:#}");
        }
        due.extend((self.built_ins.iter()).filter_map(|b| Some((b.key(), b.schedule.clone()?))));
        for (key, schedule) in due {
            // One job's failure must never stop the others.
            match self.fire_job(&key, &schedule, now) {
                Ok((next, fired)) => {
                    pass.changed |= fired;
                    if let Some(next) = next {
                        pass.next = Some(pass.next.map_or(next, |n| n.min(next)));
                    }
                }
                Err(error) => eprintln!("Job {}: {error:#}", key.as_str()),
            }
        }
        Ok(pass)
    }
    /// Fires `key` when its time has come and returns its next time and whether it fired.
    fn fire_job(
        &mut self,
        key: &JobKey,
        schedule: &Schedule,
        now: i64,
    ) -> Result<(Option<i64>, bool)> {
        let cron = Cron::parse(&schedule.cron)?;
        let zone = timezone(&schedule.timezone)?;
        let stored: Option<i64> = self
            .db
            .query_row(
                "SELECT next_at FROM job_due WHERE key=?1",
                [key.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(due) = stored else {
            let next = next_after(&cron, zone, now);
            self.set_due(key, next)?;
            return Ok((next, false));
        };
        if due > now {
            return Ok((Some(due), false));
        }
        let missed = times_between(&cron, zone, due, now).max(1);
        let catch_up = match now - due > LATE_AFTER_MS {
            true => missed,
            false => 0,
        };
        let running =
            (self.job_runs(key, 1)?.first()).is_some_and(|r| r.outcome == RunOutcome::Running);
        let next = next_after(&cron, zone, now);
        self.set_due(key, next)?;
        if !running {
            let start = BuiltInStart::Due {
                catch_up,
                due: match key.is_built_in() {
                    true => due_name(latest_time(&cron, zone, due, now), zone),
                    false => String::new(),
                },
            };
            self.run_job(key, Some(due), start, now)?;
        }
        Ok((next, !running))
    }
    fn set_due(&self, key: &JobKey, next: Option<i64>) -> Result<()> {
        match next {
            Some(next) => self.db.execute(
                "INSERT INTO job_due VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET next_at=excluded.next_at",
                params![key.as_str(), next],
            )?,
            None => self
                .db
                .execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?,
        };
        Ok(())
    }
    /// Run now, for a job or a built-in job.
    pub fn run_job_now(&mut self, key: &JobKey) -> Result<JobRun> {
        if !key.is_built_in() {
            let recipe = self.recipe(key)?;
            let space = self.space(&recipe.space_id)?.space;
            if let Some(why) = self.not_here(&recipe, &space) {
                bail!("This Mac does not run {}: {why}", recipe.name);
            }
        }
        let running =
            (self.job_runs(key, 1)?.first()).is_some_and(|r| r.outcome == RunOutcome::Running);
        ensure!(!running, "The job is running now");
        self.run_job(key, None, BuiltInStart::RunNow, now())
    }
    /// Records one run and, for a recipe, queues the check of its sources; `sources_checked`
    /// starts its thread or records why it did not start. A built-in job runs now, told why
    /// by `start`, which for a recipe says only how many missed times its run stands for.
    fn run_job(
        &mut self,
        key: &JobKey,
        scheduled: Option<i64>,
        start: BuiltInStart,
        at: i64,
    ) -> Result<JobRun> {
        let catch_up = match start {
            BuiltInStart::Due { catch_up, .. } => catch_up,
            BuiltInStart::RunNow => 0,
        };
        let mut run = JobRun {
            id: new_id(),
            job: key.clone(),
            scheduled_at: scheduled,
            started_at: at,
            ended_at: None,
            outcome: RunOutcome::Running,
            reason: String::new(),
            profile: None,
            model_reason: String::new(),
            catch_up,
            sources: vec![],
            outputs: vec![],
            thread_id: None,
            long_minutes: None,
            seen: false,
            approves: None,
        };
        if let Some(built_in) = self.built_in(key) {
            let (run_it, owner) = (built_in.run, built_in.owner.clone());
            let subject = built_in.subject.clone();
            let result = run_it(self, &subject, start);
            run.ended_at = Some(now());
            (run.outcome, run.reason) = match result {
                Ok(note) => (RunOutcome::Finished, format!("For {owner}: {note}")),
                Err(error) => (RunOutcome::Failed, format!("For {owner}: {error:#}")),
            };
            self.save_run(&run)?;
            return Ok(run);
        }
        let recipe = self.recipe(key)?;
        match self.job_model(&recipe) {
            Ok((profile, reason, _)) => {
                let checks = self.source_checks(profile.provider, &recipe.sources);
                self.source_checks.push((run.id.clone(), checks));
                run.profile = Some(profile);
                run.model_reason = reason;
            }
            Err(error) => {
                run.outcome = RunOutcome::NotStarted;
                run.ended_at = Some(at);
                run.reason = format!("{error:#}");
            }
        }
        self.save_run(&run)?;
        Ok(run)
    }
    /// Starts a run's thread once its sources passed their checks, or records what is missing.
    /// A run stopped while its sources were checked stays stopped. When the recipe's sources or
    /// the provider its run takes changed meanwhile, the run's sources are checked again, so a
    /// run only ever starts with sources checked for it.
    pub(crate) fn sources_checked(
        &mut self,
        run_id: &str,
        checked: &SourceChecks,
        checks: Vec<SourceCheck>,
    ) -> Result<()> {
        let mut run = self.job_run(run_id)?;
        if run.outcome != RunOutcome::Running || run.thread_id.is_some() {
            return Ok(());
        }
        let current = (self.recipe(&run.job))
            .and_then(|recipe| Ok((self.job_model(&recipe)?, recipe.sources)));
        let ((profile, reason, _), sources) = match current {
            Ok(current) => current,
            Err(error) => {
                run.outcome = RunOutcome::NotStarted;
                run.ended_at = Some(now());
                run.reason = format!("{error:#}");
                return self.save_run(&run);
            }
        };
        if sources != checked.sources || profile.provider != checked.provider {
            let again = self.source_checks(profile.provider, &sources);
            self.source_checks.push((run.id.clone(), again));
            run.profile = Some(profile);
            run.model_reason = reason;
            return self.save_run(&run);
        }
        run.sources = checks;
        let missing: Vec<&str> = (run.sources.iter())
            .filter_map(|check| check.problem.as_deref())
            .collect();
        if !missing.is_empty() {
            run.reason = missing.join(" ");
            run.outcome = RunOutcome::NotStarted;
            run.ended_at = Some(now());
            return self.save_run(&run);
        }
        let started = self.start_run_thread(&mut run);
        if let Err(error) = started {
            run.outcome = RunOutcome::Failed;
            run.ended_at = Some(now());
            run.reason = format!("Could not start: {error:#}");
            run.thread_id = None;
            self.save_run(&run)?;
        }
        Ok(())
    }
    /// Opens a checked run's thread on the model its record names, with the recipe's prompt.
    fn start_run_thread(&mut self, run: &mut JobRun) -> Result<()> {
        let key = run.job.clone();
        let recipe = self.recipe(&key)?;
        let profile = run.profile.clone().context("The run has no model")?;
        let reason = run.model_reason.clone();
        let waits = self.job_model(&recipe).is_ok_and(|(_, _, waits)| waits);
        let thread_id = new_id();
        let zone = timezone(&recipe.schedule.timezone)?;
        let at = run.scheduled_at.unwrap_or(run.started_at);
        let when = (zone.timestamp_millis_opt(at).single())
            .map(|t| t.format("%a %b %-d %H:%M").to_string())
            .unwrap_or_default();
        let title =
            workspace_core::assistant::title_from_message(&format!("{} · {when}", recipe.name));
        let first = assistant::FirstMessage {
            id: &new_id(),
            body: &recipe.prompt,
            files: &[],
        };
        let job = JobRunOf {
            job: key.clone(),
            run_id: run.id.clone(),
        };
        run.thread_id = Some(thread_id.clone());
        run.approves = Some(approval(&recipe));
        self.atomically(|host| {
            host.save_run(run)?;
            let model = (profile, Chooser::Wiffletree, reason, waits);
            host.open_thread(
                &thread_id,
                &recipe.space_id,
                &title,
                Some(job),
                model,
                first,
            )?;
            Ok(())
        })
    }
    /// The run a job run's thread is, while it runs.
    fn running_run_of(&self, thread: &str) -> Result<Option<JobRun>> {
        let Some(job) = self.thread(thread)?.job else {
            return Ok(None);
        };
        let run = self.job_run(&job.run_id)?;
        Ok((run.outcome == RunOutcome::Running).then_some(run))
    }
    /// Records how a run's turn ended. A run that saved nothing finished with no output.
    pub(crate) fn job_run_ended(&mut self, thread: &str, end: RunEnd) -> Result<()> {
        let Some(mut run) = self.running_run_of(thread)? else {
            return Ok(());
        };
        run.ended_at = Some(now());
        run.long_minutes = None;
        (run.outcome, run.reason) = match end {
            RunEnd::Finished => (RunOutcome::Finished, String::new()),
            RunEnd::Failed(why) => (RunOutcome::Failed, why),
            RunEnd::Stopped(why) => (RunOutcome::Stopped, why),
        };
        self.save_run(&run)
    }
    /// At a start, ends each run whose turn the host's stop cut off: its prompt is held, not
    /// queued, and a run is never retried. A run whose turn had not started yet runs now.
    pub(crate) fn end_interrupted_job_runs(&mut self) -> Result<()> {
        let running: Vec<JobRun> = self.list_data(
            "SELECT data FROM job_runs WHERE json_extract(data,'$.outcome')='running'",
            [],
        )?;
        for mut run in running {
            // A run without a thread was still having its sources checked.
            if let Some(thread) = run.thread_id.clone() {
                let queued: i64 = self.db.query_row(
                    "SELECT count(*) FROM messages WHERE recipient=?1 AND receipt='queued'",
                    [&thread],
                    |r| r.get(0),
                )?;
                if queued > 0 {
                    continue;
                }
                self.db.execute(
                    "UPDATE messages SET receipt='completed' WHERE recipient=?1 AND receipt='held'",
                    [&thread],
                )?;
            }
            run.outcome = RunOutcome::Failed;
            run.ended_at = Some(now());
            run.long_minutes = None;
            run.reason = "Wiffletree stopped during the run.".into();
            self.save_run(&run)?;
        }
        Ok(())
    }
    /// The note a run that has run 30 minutes, and every 30 minutes after, puts on the
    /// assistant's row.
    pub(crate) fn note_long_job_run(&mut self, thread: &str, minutes: i64) -> Result<bool> {
        let Some(mut run) = self.running_run_of(thread)? else {
            return Ok(false);
        };
        run.long_minutes = Some(minutes);
        self.save_run(&run)?;
        Ok(true)
    }
    /// Stop on a run: the thread whose turn the service must stop, or `None` when the run had
    /// not started its turn yet and is now stopped.
    pub fn stop_job_run(&mut self, run_id: &str) -> Result<Option<String>> {
        let mut run = self.job_run(run_id)?;
        ensure!(run.outcome == RunOutcome::Running, "The run has ended");
        // A run still having its sources checked has no thread, and never gets one now.
        if let Some(thread) = run.thread_id.clone() {
            if self.in_turn.contains(&thread) {
                return Ok(Some(thread));
            }
            self.db.execute(
                "UPDATE messages SET receipt='completed' WHERE recipient=?1 AND receipt IN ('queued','held')",
                [&thread],
            )?;
        }
        run.outcome = RunOutcome::Stopped;
        run.ended_at = Some(now());
        run.reason = STOPPED_BY_YOU.into();
        self.save_run(&run)?;
        Ok(None)
    }
    /// Keeps an output a job's run saved at its stable path. In a Space stored on this Mac only,
    /// it is committed to the Space's storage. In one stored on GitHub, the runner's output is
    /// pushed with the Space's storage once this Mac approved the prompt and sources its run
    /// had, and until then waits in this Mac's private board store, as every member's own
    /// output of an "Everyone runs their own" job stays.
    pub(crate) fn save_job_output(
        &mut self,
        thread: &Thread,
        job: &JobRunOf,
        title: &str,
        source: &Path,
    ) -> Result<RunOutput> {
        let recipe = self.recipe(&job.job)?;
        let space = self.space(&thread.space_id)?.space;
        let mut run = self.job_run(&job.run_id)?;
        let about = json!({"title":title,"job":recipe.key(),"run_id":job.run_id,"saved_at":now()});
        let local = matches!(space.storage.repository, StorageRepository::Local { .. });
        let shared = !local && runs_here(&recipe);
        // A run from before approvals has nothing to compare, so it waits too.
        let waiting =
            shared && (run.approves.is_none() || self.approved(&recipe.key())? != run.approves);
        let output = if local {
            let storage = self.storage_folder(&space.storage.repository);
            let base = board_folder(&space.storage.folder, &recipe.id);
            let root = folder::Root::new(&space.storage.root);
            let (file, version) = assistant::store_output_in(
                &root,
                &storage,
                &root.join(&base),
                title,
                source,
                about,
            )?;
            RunOutput {
                title: title.into(),
                file,
                version,
                kept: format!("In {}'s storage", space.name),
                waiting: false,
            }
        } else if let Some(output) = (shared && !waiting)
            .then(|| self.push_shared_output(&space, &recipe, title, source, about.clone()))
            .transpose()?
            .flatten()
        {
            output
        } else {
            // Waiting for approval, or kept here while its shared copy was edited outside
            // Wiffletree and is paused.
            let store = assistant::board_store(&self.home);
            let base = private_folder(&space, &recipe);
            let (file, version) = assistant::store_output(&store, &base, title, source, about)?;
            RunOutput {
                title: title.into(),
                file,
                version,
                kept: "Only on this Mac".into(),
                waiting,
            }
        };
        run.outputs.retain(|o| o.title != title);
        run.outputs.push(output.clone());
        self.save_run(&run)?;
        Ok(output)
    }
    /// A runner's approved save of `source`, written for the next pass to push, or `None` when
    /// the shared copy was edited outside Wiffletree.
    fn push_shared_output(
        &mut self,
        space: &Space,
        recipe: &Recipe,
        title: &str,
        source: &Path,
        about: Value,
    ) -> Result<Option<RunOutput>> {
        let file = (source.file_name())
            .context("Name a file")?
            .to_string_lossy()
            .into_owned();
        let version = self.push_job_output(space, recipe, &file, fs::read(source)?, about)?;
        if version.is_some() {
            // An older output still waiting must never be shared over this one.
            self.answer_waiting(&recipe.key(), title, None)?;
        }
        Ok(version.map(|version| RunOutput {
            title: title.into(),
            file,
            version,
            kept: format!("Shared in {}", space.name),
            waiting: false,
        }))
    }
    /// Writes a runner's output, which `about` titles, into its Space's storage on GitHub, for
    /// the next pass to push; returns the version it will be, or `None`, writing nothing, when
    /// its files there were edited outside Wiffletree.
    fn push_job_output(
        &mut self,
        space: &Space,
        recipe: &Recipe,
        file: &str,
        bytes: Vec<u8>,
        about: Value,
    ) -> Result<Option<u32>> {
        let base = board_folder(&space.storage.folder, &recipe.id);
        let storage = self.storage_folder(&space.storage.repository);
        let title = about["title"].as_str().context("An output has a title")?;
        let root = folder::Root::new(&space.storage.root);
        let in_repository = root.join(&base);
        let folder = format!(
            "{in_repository}/{}",
            assistant::output_folder(&storage.join(&in_repository), title)?
        );
        let metadata = format!("{folder}.json");
        let key = repos::key(&space.storage.repository.key());
        let record = repos::Record::read(&self.db, &key)?.context("The storage has no record")?;
        if self.edited_outside(&storage, &folder, &record)? {
            return Ok(None);
        }
        let kept_here = self.keep_replaced_here(space, recipe, title, &metadata, &record)?;
        let message = format!("{}: saved an output of {}", space.name, recipe.name);
        self.write_output(space, &base, file, bytes, about, &message)?;
        let pushed = folder::saves(&storage, &root, &metadata).unwrap_or(0);
        Ok(Some(pushed + kept_here + 1))
    }
    /// Whether the files of the output at `folder` in the clone are not what Wiffletree last
    /// wrote or synced: an edit made outside Wiffletree, which the next pass pauses and no save
    /// overwrites.
    fn edited_outside(&self, storage: &Path, folder: &str, record: &repos::Record) -> Result<bool> {
        let metadata = format!("{folder}.json");
        let inside = |path: &str| path == metadata || path.starts_with(&format!("{folder}/"));
        let mut here = std::collections::BTreeMap::new();
        if let Ok(content) = fs::read(storage.join(&metadata)) {
            here.insert(metadata.clone(), content);
        }
        for entry in (fs::read_dir(storage.join(folder)).into_iter().flatten()).flatten() {
            let path = format!("{folder}/{}", entry.file_name().to_string_lossy());
            here.insert(path, fs::read(entry.path())?);
        }
        let paths: std::collections::BTreeSet<&String> = (record.written.keys())
            .filter(|path| inside(path))
            .chain(here.keys())
            .collect();
        // What Wiffletree last wrote or synced of each file, `None` for no file.
        let wrote = |path: &String| record.written.get(path).cloned().flatten();
        Ok(paths
            .iter()
            .any(|path| here.get(*path).cloned() != wrote(path)))
    }
    /// A save is about to replace the version at `metadata` that no pass has pushed: keeps that
    /// version, as Wiffletree wrote it, in this Mac's private board store, where Boards lists it
    /// among the output's versions as only on this Mac. Returns how many such versions it has.
    fn keep_replaced_here(
        &self,
        space: &Space,
        recipe: &Recipe,
        title: &str,
        metadata: &str,
        record: &repos::Record,
    ) -> Result<u32> {
        let store = assistant::board_store(&self.home);
        let base = unpushed_folder(space, recipe);
        let kept = || -> Result<u32> {
            let folder = assistant::output_folder(&store.join(&base), title)?;
            let kept = format!("{base}/{folder}.json");
            Ok(
                runtime::git_output(&store, &["rev-list", "--count", "HEAD", "--", &kept])
                    .map_or(0, |count| count.trim().parse().unwrap_or(0)),
            )
        };
        let wrote = |path: &str| record.written.get(path).cloned().flatten();
        let written = wrote(metadata);
        if written.is_none() || written == record.synced.get(metadata).cloned().flatten() {
            return kept();
        }
        let about: Value = serde_json::from_slice(written.as_deref().unwrap_or_default())?;
        let file = about["file"].as_str().unwrap_or_default().to_owned();
        let folder = metadata.strip_suffix(".json").unwrap_or(metadata);
        let (Some(bytes), true) = (wrote(&format!("{folder}/{file}")), plain_file_name(&file))
        else {
            return kept();
        };
        let staged = tempfile::tempdir_in(&self.home)?;
        fs::write(staged.path().join(&file), bytes)?;
        assistant::store_output(&store, &base, title, &staged.path().join(&file), about)?;
        kept()
    }
    /// Share or Keep private, for the latest output of `title` that waits for approval, which
    /// `run_id` saved. Share pushes it with its Space's storage, takes it out of the private
    /// board store and approves the prompt and sources its run had, so later outputs push
    /// without asking until those change. Keep private leaves it only on this Mac and the job
    /// unapproved.
    pub fn approve_job_output(&mut self, run_id: &str, title: &str, share: bool) -> Result<()> {
        let run = self.job_run(run_id)?;
        ensure!(
            (run.outputs.iter()).any(|output| output.title == title && output.waiting),
            "That output no longer waits for approval"
        );
        let recipe = self.recipe(&run.job)?;
        let space = self.space(&recipe.space_id)?.space;
        let store = assistant::board_store(&self.home);
        let base = private_folder(&space, &recipe);
        let folder = format!(
            "{base}/{}",
            assistant::output_folder(&store.join(&base), title)?
        );
        let about: Value =
            serde_json::from_slice(&fs::read(store.join(format!("{folder}.json")))?)?;
        ensure!(
            about["run_id"] == json!(run_id),
            "A newer output replaced this one; answer that one"
        );
        if share {
            ensure!(
                runs_here(&recipe),
                "This Mac no longer runs {}, so it cannot share its output",
                recipe.name
            );
            let file = about["file"].as_str().unwrap_or_default().to_owned();
            let path = store.join(&folder).join(&file);
            ensure!(
                plain_file_name(&file) && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()),
                "The output's file is missing"
            );
            let pushed = self.push_job_output(&space, &recipe, &file, fs::read(&path)?, about)?;
            ensure!(
                pushed.is_some(),
                "{}'s shared copy was changed outside Wiffletree. Settle that change first, then share.",
                title
            );
            fs::remove_dir_all(store.join(&folder))?;
            fs::remove_file(store.join(format!("{folder}.json")))?;
            let metadata = format!("{folder}.json");
            storage::commit_local(&store, &[&folder, &metadata], &format!("Shared {folder}"))?;
            let approves = run
                .approves
                .clone()
                .context("The run has no recipe to approve")?;
            self.db.execute(
                "INSERT INTO job_approvals VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET approved=excluded.approved",
                params![recipe.key().as_str(), approves],
            )?;
        }
        // Every earlier output of the title is answered with the latest.
        let shared = share.then(|| (run.id.as_str(), format!("Shared in {}", space.name)));
        self.answer_waiting(&run.job, title, shared)
    }
    /// Ends the wait of every output of `title` of the job, which a later output answered:
    /// `shared` names the run whose output was shared, and where it is now kept.
    fn answer_waiting(
        &self,
        job: &JobKey,
        title: &str,
        shared: Option<(&str, String)>,
    ) -> Result<()> {
        for mut earlier in self.job_runs(job, usize::MAX)? {
            let kept = (shared.as_ref())
                .filter(|(run, _)| *run == earlier.id)
                .map(|(_, kept)| kept.clone());
            let waited = (earlier.outputs.iter_mut())
                .filter(|output| output.title == title && output.waiting)
                .map(|output| {
                    output.waiting = false;
                    if let Some(kept) = &kept {
                        output.kept = kept.clone();
                    }
                })
                .count();
            if waited > 0 {
                self.save_run(&earlier)?;
            }
        }
        Ok(())
    }
}

/// Where a job's outputs kept only on this Mac are, in the private board store.
fn private_folder(space: &Space, recipe: &Recipe) -> String {
    format!("{}/{}/{}", space.id, assistant::JOB_OUTPUTS, recipe.id)
}

/// The folder of the private board store, under a job's own, that keeps a runner's saves that
/// a later save replaced before any pass pushed them. Boards lists them among the shared
/// output's versions, never as outputs of their own.
pub(crate) const UNPUSHED: &str = "_unpushed";
pub(crate) fn unpushed_folder(space: &Space, recipe: &Recipe) -> String {
    format!("{}/{UNPUSHED}", private_folder(space, recipe))
}

pub(crate) const STOPPED_BY_YOU: &str = "You stopped the run.";

/// Starts and proposes jobs as the service does, with sources checked here, not off the thread.
#[cfg(test)]
impl Host {
    pub(crate) fn run_now(&mut self, key: &JobKey) -> Result<JobRun> {
        let run = self.run_job_now(key)?;
        self.check_queued_sources();
        self.job_run(&run.id)
    }
    pub(crate) fn fire_and_check(&mut self, now: i64) -> Result<JobPass> {
        let pass = self.fire_due_jobs(now)?;
        self.check_queued_sources();
        Ok(pass)
    }
    fn check_queued_sources(&mut self) {
        // A check that finds the recipe changed queues another.
        loop {
            let queued = self.take_source_checks();
            if queued.is_empty() {
                break;
            }
            for (run, checks) in queued {
                let found = checks.run();
                self.sources_checked(&run, &checks, found).unwrap();
            }
        }
    }
    pub(crate) fn propose(&mut self, session: &Session, args: &Value) -> Result<Value> {
        let (recipe, pending) = self.propose_job(session, args)?;
        self.add_recipe_card(&session.id, recipe, pending.check())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use workspace_core::assistant::CardKind;

    fn at(zone: Tz, text: &str) -> i64 {
        let local = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap();
        zone.from_local_datetime(&local)
            .earliest()
            .unwrap()
            .timestamp_millis()
    }
    fn shown(zone: Tz, ms: i64) -> String {
        zone.timestamp_millis_opt(ms)
            .unwrap()
            .format("%Y-%m-%d %H:%M %Z")
            .to_string()
    }
    const NEW_YORK: Tz = chrono_tz::America::New_York;

    #[test]
    fn a_time_repeated_by_fall_back_runs_once() {
        let cron = Cron::parse("30 1 * * *").unwrap();
        let first = next_after(&cron, NEW_YORK, at(NEW_YORK, "2026-10-31 12:00")).unwrap();
        assert_eq!(shown(NEW_YORK, first), "2026-11-01 01:30 EDT");
        let second = next_after(&cron, NEW_YORK, first).unwrap();
        assert_eq!(shown(NEW_YORK, second), "2026-11-02 01:30 EST");
    }

    #[test]
    fn a_time_skipped_by_spring_forward_runs_at_the_next_valid_minute() {
        let cron = Cron::parse("30 2 * * *").unwrap();
        let next = next_after(&cron, NEW_YORK, at(NEW_YORK, "2027-03-13 12:00")).unwrap();
        assert_eq!(shown(NEW_YORK, next), "2027-03-14 03:00 EDT");
        let after = next_after(&cron, NEW_YORK, next).unwrap();
        assert_eq!(shown(NEW_YORK, after), "2027-03-15 02:30 EDT");
    }

    #[test]
    fn times_follow_the_recipes_zone_not_the_macs() {
        let cron = Cron::parse("0 7 * * 1-5").unwrap();
        let tokyo: Tz = chrono_tz::Asia::Tokyo;
        // A Friday evening in New York is Saturday in Tokyo; the next run is Monday 07:00 in
        // New York whatever zone the Mac is in.
        let friday = at(NEW_YORK, "2026-10-30 20:00");
        let next = next_after(&cron, NEW_YORK, friday).unwrap();
        assert_eq!(shown(NEW_YORK, next), "2026-11-02 07:00 EST");
        assert_ne!(next_after(&cron, tokyo, friday), Some(next));
    }

    #[test]
    fn missed_times_are_counted() {
        let cron = Cron::parse("0 * * * *").unwrap();
        let from = at(NEW_YORK, "2026-10-29 10:00");
        let to = at(NEW_YORK, "2026-10-29 12:20");
        assert_eq!(times_between(&cron, NEW_YORK, from, to), 3);
        assert_eq!(times_between(&cron, NEW_YORK, from, from), 1);
    }

    #[test]
    fn an_aws_source_names_one_profile() {
        let prefixes = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            aws_profile(&prefixes(&[
                "aws --profile readonly s3 ls",
                "aws --profile readonly ce get-cost-and-usage"
            ])),
            Some("readonly".into())
        );
        assert_eq!(aws_profile(&prefixes(&["aws s3 ls"])), None);
        assert_eq!(
            aws_profile(&prefixes(&[
                "aws --profile a s3 ls",
                "aws --profile b s3 ls"
            ])),
            None
        );
    }

    struct Jobs {
        home: tempfile::TempDir,
        host: Host,
        personal: Space,
    }
    /// A store whose sources are looked for only in its own `bin/` and `user/` folders.
    fn jobs() -> Jobs {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        fs::create_dir_all(home.path().join("bin")).unwrap();
        host.source_env = Some(SourceEnv {
            path: vec![home.path().join("bin")],
            home: home.path().join("user"),
            mcp_limit: std::time::Duration::from_secs(2),
        });
        let personal = host.space(&host.default_space().unwrap()).unwrap().space;
        Jobs {
            home,
            host,
            personal,
        }
    }
    impl Jobs {
        /// A stand-in CLI whose every command exits with `code`.
        fn cli(&self, name: &str, code: i32) {
            self.script(name, &format!("exit {code}"));
        }
        /// A stand-in program in `bin/` running `body` as a shell script.
        fn script(&self, name: &str, body: &str) {
            let path = self.home.path().join("bin").join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        /// A stand-in MCP server that answers the handshake, offering `tools`, or with an
        /// error for `initialize` when `tools` is `None`.
        fn mcp_server(&self, name: &str, tools: Option<&[&str]>) {
            let initialized = match tools {
                Some(_) => {
                    r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fake","version":"1"}}}"#
                }
                None => {
                    r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"token expired"}}"#
                }
            };
            let tools: Vec<Value> = (tools.unwrap_or_default().iter())
                .map(|name| json!({"name": name}))
                .collect();
            let listed = json!({"jsonrpc":"2.0","id":2,"result":{"tools":tools}});
            self.script(
                name,
                &format!(
                    "while IFS= read -r line; do case \"$line\" in\n*'\"method\":\"initialize\"'*) echo '{initialized}' ;;\n*'\"method\":\"tools/list\"'*) echo '{listed}' ;;\nesac; done"
                ),
            );
        }
        /// Claude's configuration with these MCP servers, by name and command.
        fn claude_servers(&self, servers: &[(&str, &str)]) {
            let servers: serde_json::Map<String, Value> = (servers.iter())
                .map(|(name, command)| ((*name).to_owned(), json!({"command": command})))
                .collect();
            fs::create_dir_all(self.home.path().join("user")).unwrap();
            let config = json!({ "mcpServers": servers }).to_string();
            fs::write(self.home.path().join("user/.claude.json"), config).unwrap();
        }
        fn storage(&self) -> PathBuf {
            let repository = &self.personal.storage.repository;
            self.host.root_folder(repository).unwrap()
        }
        fn thread_of(&self, run: &JobRun) -> Thread {
            self.host.thread(run.thread_id.as_ref().unwrap()).unwrap()
        }
    }
    fn recipe(space: &Space, name: &str, cron: &str) -> Recipe {
        Recipe {
            id: workspace_core::job::job_id(name),
            name: name.into(),
            space_id: space.id.clone(),
            prompt: "Brief me on my open PRs.".into(),
            schedule: Schedule {
                cron: cron.into(),
                timezone: "America/New_York".into(),
            },
            model: None,
            sources: vec![],
            output: None,
            sharing: Sharing::OneRunner,
            runner: None,
            enabled: true,
            extra: Default::default(),
        }
    }
    fn gh() -> Source {
        Source::Cli {
            command: "gh".into(),
            prefixes: vec!["gh pr list".into(), "gh pr view".into()],
            check: None,
            extra: Default::default(),
        }
    }
    fn linear() -> Source {
        Source::Mcp {
            server: "linear".into(),
            tools: vec!["list_issues".into()],
            extra: Default::default(),
        }
    }
    fn ny(text: &str) -> i64 {
        at(NEW_YORK, text)
    }

    #[test]
    fn only_the_users_save_makes_a_job_and_it_is_a_commit_in_the_spaces_storage() {
        let mut s = jobs();
        let thread = (s.host)
            .start_thread("t1", &s.personal.id, "Brief me each weekday", &[], None)
            .unwrap();
        let session = s.host.session(&thread.id).unwrap();
        let args = json!({"name":"Morning brief","prompt":"Brief me.","cron":"0 7 * * 1-5","timezone":"America/New_York","sources":[]});
        s.host.propose(&session, &args).unwrap();
        let card = s.host.thread("t1").unwrap().cards.remove(0);
        let CardKind::Recipe { recipe, preview } = card.kind else {
            panic!("a recipe card");
        };
        assert_eq!(preview.schedule, "Weekdays at 07:00 (America/New_York)");
        assert_eq!(preview.next_runs.len(), 3);
        assert!(s.host.recipes().unwrap().is_empty(), "a card saves nothing");
        // No thread tool saves, enables, runs or removes a job.
        for tool in ["save_job", "set_job_enabled", "run_job_now", "delete_job"] {
            assert!(
                s.host.thread_tool(&session, tool, &json!({})).is_err(),
                "{tool}"
            );
        }
        let saved = s.host.save_job(*recipe).unwrap();
        let file = s
            .storage()
            .join(recipe_file(&s.personal.storage.folder, "morning-brief"));
        let read = read_recipe(&fs::read(&file).unwrap(), &s.personal.id, "morning-brief").unwrap();
        assert_eq!(read, saved.recipe);
        let subject = crate::test_support::git(&s.storage(), &["log", "-1", "--format=%s"]);
        assert!(subject.contains("added the job Morning brief"), "{subject}");
        // Every job command is the user's alone.
        let key = saved.recipe.key();
        for command in [
            Command::SaveJob {
                recipe: Box::new(saved.recipe.clone()),
                new: true,
            },
            Command::SetJobEnabled {
                space_id: s.personal.id.clone(),
                job_id: "morning-brief".into(),
                enabled: false,
            },
            Command::RunJobNow { job: key.clone() },
            Command::DeleteJob {
                space_id: s.personal.id.clone(),
                job_id: "morning-brief".into(),
            },
            Command::StopJobRun { run_id: "r".into() },
            Command::SeeJobRuns { job: key },
            Command::TakeOverJob {
                space_id: s.personal.id.clone(),
                job_id: "morning-brief".into(),
            },
            Command::ApproveJobOutput {
                run_id: "r".into(),
                title: "Brief".into(),
                share: true,
            },
        ] {
            assert!(command.is_human_only(), "{command:?}");
        }
        s.host.delete_job(&s.personal.id, "morning-brief").unwrap();
        assert!(!file.exists() && s.host.recipes().unwrap().is_empty());
        let subject = crate::test_support::git(&s.storage(), &["log", "-1", "--format=%s"]);
        assert!(
            subject.contains("removed the job Morning brief"),
            "{subject}"
        );
    }

    #[test]
    fn an_hourly_job_asleep_over_three_runs_runs_once_as_a_catch_up() {
        let mut s = jobs();
        let key = (s
            .host
            .save_job(recipe(&s.personal, "Hourly", "0 * * * *"))
            .unwrap())
        .recipe
        .key();
        let pass = s.host.fire_and_check(ny("2026-10-29 09:10")).unwrap();
        assert_eq!(pass.next, Some(ny("2026-10-29 10:00")));
        assert!(s.host.job_runs(&key, 10).unwrap().is_empty());
        // Asleep from 09:10 to 12:20.
        let pass = s.host.fire_and_check(ny("2026-10-29 12:20")).unwrap();
        assert_eq!(pass.next, Some(ny("2026-10-29 13:00")));
        let runs = s.host.job_runs(&key, 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].catch_up_label().as_deref(),
            Some("catch-up for 3 missed runs")
        );
        assert_eq!(runs[0].scheduled_at, Some(ny("2026-10-29 10:00")));
        // The run is a fresh read-only thread in its Space with the prompt as its one message.
        let thread = s.thread_of(&runs[0]);
        let session = s.host.session(&thread.id).unwrap();
        assert_eq!(session.role, Role::JobRun);
        assert_eq!(
            session.owner,
            Some(SessionOwner::Space(s.personal.id.clone()))
        );
        let messages = s.host.messages(&thread.id, None, 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "Brief me on my open PRs.");
        assert!(s.host.thread_tabs(now()).unwrap().is_empty(), "no tab");
        assert!(
            s.host
                .send(&new_id(), None, &thread.id, "and more")
                .is_err()
        );
        s.host.job_run_ended(&thread.id, RunEnd::Finished).unwrap();
        let ended = s.host.job_run(&runs[0].id).unwrap();
        assert_eq!(
            (ended.outcome, ended.outputs.len()),
            (RunOutcome::Finished, 0)
        );
        // The next time runs on time, in a thread of its own.
        s.host
            .fire_and_check(ny("2026-10-29 13:00") + 1000)
            .unwrap();
        let runs = s.host.job_runs(&key, 10).unwrap();
        assert_eq!((runs.len(), runs[0].catch_up), (2, 0));
        assert_ne!(runs[0].thread_id, runs[1].thread_id);
    }

    #[test]
    fn a_paused_job_skips_its_times_and_resumes_from_the_next() {
        let mut s = jobs();
        let key = (s
            .host
            .save_job(recipe(&s.personal, "Hourly", "0 * * * *"))
            .unwrap())
        .recipe
        .key();
        s.host.fire_and_check(ny("2026-10-29 09:10")).unwrap();
        s.host
            .set_job_enabled(&s.personal.id, "hourly", false)
            .unwrap();
        s.host.fire_and_check(ny("2026-10-29 12:20")).unwrap();
        assert!(s.host.job_runs(&key, 10).unwrap().is_empty());
        s.host
            .set_job_enabled(&s.personal.id, "hourly", true)
            .unwrap();
        let pass = s.host.fire_and_check(ny("2026-10-29 12:30")).unwrap();
        assert_eq!(pass.next, Some(ny("2026-10-29 13:00")));
        assert!(s.host.job_runs(&key, 10).unwrap().is_empty());
        s.host.fire_and_check(ny("2026-10-29 13:00")).unwrap();
        assert_eq!(s.host.job_runs(&key, 10).unwrap()[0].catch_up, 0);
    }

    #[test]
    fn a_signed_out_cli_or_a_missing_mcp_server_keeps_a_run_from_starting() {
        let mut s = jobs();
        s.cli("gh", 1);
        let mut brief = recipe(&s.personal, "Morning brief", "0 7 * * 1-5");
        brief.sources = vec![gh(), linear()];
        let key = s.host.save_job(brief).unwrap().recipe.key();
        let run = s.host.run_now(&key).unwrap();
        assert_eq!(run.outcome, RunOutcome::NotStarted);
        assert_eq!(run.thread_id, None);
        assert!(
            run.reason
                .contains("gh is not signed in on this Mac. Run `gh auth login`."),
            "{}",
            run.reason
        );
        assert!(run.reason.contains("The linear MCP server is not configured for Claude. Add it to Claude's MCP configuration."), "{}", run.reason);
        assert!(run.needs_attention());
        s.host.see_job_runs(&key).unwrap();
        assert!(!s.host.job_run(&run.id).unwrap().needs_attention());

        s.cli("gh", 0);
        s.mcp_server("linear-mcp", Some(&["list_issues", "create_issue"]));
        s.claude_servers(&[("linear", "linear-mcp")]);
        let run = s.host.run_now(&key).unwrap();
        assert_eq!(run.outcome, RunOutcome::Running);
        assert!(run.sources.iter().all(|check| check.problem.is_none()));
        // The run is offered only its recipe's sources: gh's commands and linear's tool.
        let session = s.host.session(run.thread_id.as_ref().unwrap()).unwrap();
        let launch = s.host.thread_launch(&session).unwrap();
        assert!(launch.job_run);
        assert_eq!(launch.sources, [gh(), linear()]);
    }

    #[test]
    fn a_write_command_is_refused_at_propose_and_save_and_the_host_runs_only_its_own_checks() {
        let mut s = jobs();
        // A stand-in gh that records each command it is given.
        let calls = s.home.path().join("calls.log");
        s.script("gh", &format!("echo \"$*\" >> '{}'", calls.display()));
        let thread = (s.host)
            .start_thread("t1", &s.personal.id, "Brief me", &[], None)
            .unwrap();
        let session = s.host.session(&thread.id).unwrap();
        let propose = |prefix: &str, check: Option<&str>| {
            let command = prefix.split(' ').next().unwrap();
            json!({"name":"Brief","prompt":"Brief me.","cron":"0 7 * * *","timezone":"America/New_York",
                "sources":[{"kind":"cli","command":command,"prefixes":[prefix],"check":check}]})
        };
        for (prefix, check) in [
            ("gh pr comment", None),
            ("gh api", None),
            ("gh pr list", Some("gh pr comment 1 --body hi")),
            // Its outfile writes anywhere, and an operation not listed is refused too.
            ("aws --profile readonly s3api get-object", None),
            ("aws --profile readonly ec2 describe-instances", None),
        ] {
            let args = propose(prefix, check);
            assert!(s.host.propose(&session, &args).is_err(), "{prefix}");
            let mut write = recipe(&s.personal, "Brief", "0 7 * * *");
            write.sources = vec![Source::Cli {
                command: prefix.split(' ').next().unwrap().into(),
                prefixes: vec![prefix.into()],
                check: check.map(str::to_owned),
                extra: Default::default(),
            }];
            let save = Command::SaveJob {
                recipe: Box::new(write),
                new: false,
            };
            assert!(s.host.execute(save).is_err(), "{prefix}");
        }
        assert!(!calls.exists(), "nothing ran for a refused recipe");
        assert!(s.host.recipes().unwrap().is_empty());
        // A read is proposed, saved and run, and the host runs only `gh auth status`.
        let args = propose("gh pr list", None);
        s.host.propose(&session, &args).unwrap();
        let mut brief = recipe(&s.personal, "Brief", "0 7 * * *");
        brief.sources = vec![gh()];
        let key = s.host.save_job(brief).unwrap().recipe.key();
        s.host.run_now(&key).unwrap();
        assert_eq!(
            fs::read_to_string(&calls).unwrap(),
            "auth status\nauth status\n"
        );
    }

    #[test]
    fn an_mcp_server_must_start_answer_and_offer_its_tools_before_a_run() {
        let s = jobs();
        s.mcp_server("answers", Some(&["list_issues"]));
        s.mcp_server("expired", None);
        s.mcp_server("lacks", Some(&["get_cycle"]));
        s.script("exits", "exit 1");
        s.script("hangs", "exec /bin/sleep 30");
        s.claude_servers(&[
            ("answers", "answers"),
            ("expired", "expired"),
            ("lacks", "lacks"),
            ("exits", "exits"),
            ("hangs", "hangs"),
            ("gone", "not-installed"),
        ]);
        let problem = |server: &str| {
            let source = Source::Mcp {
                server: server.into(),
                tools: vec!["list_issues".into()],
                extra: Default::default(),
            };
            s.host
                .source_checks(Provider::Claude, &[source])
                .run()
                .remove(0)
                .problem
        };
        assert_eq!(problem("answers"), None);
        let said = |server: &str, why: &str| {
            assert_eq!(
                problem(server),
                Some(format!("The {server} MCP server {why}."))
            );
        };
        said(
            "expired",
            "answered with an error, such as a sign-in that has expired",
        );
        said("lacks", "does not offer list_issues");
        said("exits", "stopped before it answered");
        said("hangs", "did not answer within 2 seconds");
        said(
            "gone",
            "names not-installed, which is not installed on this Mac",
        );
        // None of what a server said is repeated.
        assert!(!problem("expired").unwrap().contains("token expired"));
    }

    #[test]
    fn a_runs_instructions_are_the_job_run_core_not_the_assistant_core() {
        let mut s = jobs();
        let key = (s
            .host
            .save_job(recipe(&s.personal, "Brief", "0 7 * * *"))
            .unwrap())
        .recipe
        .key();
        let run = s.host.run_now(&key).unwrap();
        let session = s.host.session(run.thread_id.as_ref().unwrap()).unwrap();
        let text = s.host.session_instructions(&session).unwrap();
        assert!(text.contains("# Job run core — v3"), "{text}");
        assert!(!text.contains("# Assistant core") && !text.contains("`propose_job`"));
    }

    #[test]
    fn a_card_never_replaces_a_job_of_the_same_name() {
        let mut s = jobs();
        let first = s
            .host
            .save_job(recipe(&s.personal, "Morning brief", "0 7 * * *"));
        first.unwrap();
        let thread = (s.host)
            .start_thread("t1", &s.personal.id, "Another brief", &[], None)
            .unwrap();
        let session = s.host.session(&thread.id).unwrap();
        let args = json!({"name":"Morning brief","prompt":"Other.","cron":"0 8 * * *","timezone":"America/New_York","sources":[]});
        let refused = s.host.propose(&session, &args).unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("A job named Morning brief already exists in Personal"),
            "{refused}"
        );
        let mut other = recipe(&s.personal, "Morning brief", "0 8 * * *");
        other.prompt = "Other.".into();
        let save = |new| Command::SaveJob {
            recipe: Box::new(other.clone()),
            new,
        };
        assert!(s.host.execute(save(true)).is_err());
        let kept = s.host.recipes().unwrap().remove(0);
        assert_eq!(kept.prompt, "Brief me on my open PRs.");
        // Saving from the job's own page changes it.
        s.host.execute(save(false)).unwrap();
        assert_eq!(s.host.recipes().unwrap()[0].prompt, "Other.");
    }

    #[test]
    fn an_unseen_failure_counts_however_many_runs_follow_it() {
        let mut s = jobs();
        s.cli("gh", 1);
        let mut brief = recipe(&s.personal, "Brief", "0 7 * * *");
        brief.sources = vec![gh()];
        let key = s.host.save_job(brief.clone()).unwrap().recipe.key();
        assert_eq!(
            s.host.run_now(&key).unwrap().outcome,
            RunOutcome::NotStarted
        );
        brief.sources = vec![];
        s.host.save_job(brief).unwrap();
        for _ in 0..MAX_RUNS_SHOWN {
            let run = s.host.run_now(&key).unwrap();
            s.host
                .job_run_ended(run.thread_id.as_ref().unwrap(), RunEnd::Finished)
                .unwrap();
        }
        let state = s.host.job_states().unwrap().remove(0);
        assert_eq!(state.runs.len(), MAX_RUNS_SHOWN);
        assert!(state.runs.iter().all(|run| !run.needs_attention()));
        assert_eq!(state.attention, 1);
        s.host.see_job_runs(&key).unwrap();
        assert_eq!(s.host.job_states().unwrap()[0].attention, 0);
    }

    #[test]
    fn a_cli_source_moves_a_codex_run_to_claude_or_keeps_it_from_starting() {
        let mut s = jobs();
        s.cli("gh", 0);
        let codex = ModelProfile {
            provider: Provider::Codex,
            model: "gpt-6.1-sol".into(),
            effort: "medium".into(),
        };
        let mut brief = recipe(&s.personal, "PR queue", "0 7 * * 1-5");
        brief.sources = vec![gh()];
        brief.model = Some(codex.clone());
        let key = s.host.save_job(brief).unwrap().recipe.key();
        let run = s.host.run_now(&key).unwrap();
        assert_eq!(run.profile.map(|p| p.provider), Some(Provider::Claude));
        // The job's page names the same model, so the run need not name its own.
        let state = &s.host.job_states().unwrap()[0];
        assert_eq!(
            state.profile.as_ref().map(|p| p.provider),
            Some(Provider::Claude)
        );
        assert!(
            run.model_reason
                .contains("is on Codex, and Codex cannot yet hold gh to read commands"),
            "{}",
            run.model_reason
        );
        s.host
            .job_run_ended(run.thread_id.as_ref().unwrap(), RunEnd::Finished)
            .unwrap();
        // With only Codex on the Job row, the run does not start and says why.
        let mut selection = s.host.model_selection().unwrap();
        selection.set_job_row(Some(Line {
            first: codex,
            then: Then::Wait,
        }));
        s.host.set_model_selection(&selection).unwrap();
        let run = s.host.run_now(&key).unwrap();
        assert_eq!(run.outcome, RunOutcome::NotStarted);
        assert_eq!(
            run.reason,
            "Codex cannot yet hold gh to read commands, and the Job row has no Claude choice."
        );
        assert_eq!(s.host.job_states().unwrap()[0].profile, None);
    }

    #[test]
    fn a_cli_source_moves_a_run_off_a_spaces_codex_job_row_saying_why() {
        let mut s = jobs();
        s.cli("gh", 0);
        let signed_in = ProviderStatus {
            installed: true,
            signed_in: true,
            plan: None,
        };
        s.host.record_detection(Provider::Codex, &signed_in);
        s.host.record_detection(Provider::Claude, &signed_in);
        let row = JobRow {
            job: Job::Job,
            main: Line {
                first: ModelProfile {
                    provider: Provider::Codex,
                    model: "gpt-6.1-sol".into(),
                    effort: "high".into(),
                },
                then: Then::Wait,
            },
            small: None,
        };
        let settings = SpaceModelSettings {
            rows: vec![row],
            reviews: Default::default(),
        };
        let space = s.personal.id.clone();
        s.host.set_space_model_settings(&space, settings).unwrap();
        let mut brief = recipe(&s.personal, "PR queue", "0 7 * * 1-5");
        brief.sources = vec![gh()];
        let (model, reason, _) = s.host.job_model(&brief).unwrap();
        assert_eq!(model.provider, Provider::Claude);
        assert!(
            reason.contains(
                "Personal's Job row falls back to this Mac's: Codex cannot yet hold gh to read commands."
            ),
            "{reason}"
        );
    }

    #[test]
    fn a_run_starts_on_its_recipes_model_then_its_spaces_job_row_then_this_macs() {
        let mut s = jobs();
        let profile = |provider, model: &str| ModelProfile {
            provider,
            model: model.into(),
            effort: "high".into(),
        };
        let (space, sol) = (
            s.personal.id.clone(),
            profile(Provider::Codex, "gpt-6.1-sol"),
        );
        let row = JobRow {
            job: Job::Job,
            main: Line {
                first: sol.clone(),
                then: Then::Wait,
            },
            small: None,
        };
        let settings = SpaceModelSettings {
            rows: vec![row],
            reviews: Default::default(),
        };
        s.host.set_space_model_settings(&space, settings).unwrap();
        let mut brief = recipe(&s.personal, "PR queue", "0 7 * * 1-5");
        let (model, reason, _) = s.host.job_model(&brief).unwrap();
        assert_eq!(model, sol);
        assert_eq!(reason, "Job row: Personal's Job row: First choice");

        let opus = profile(Provider::Claude, "opus");
        brief.model = Some(opus.clone());
        assert_eq!(s.host.job_model(&brief).unwrap().0, opus);

        brief.model = None;
        let signed_out = ProviderStatus {
            installed: true,
            signed_in: false,
            plan: None,
        };
        s.host.record_detection(Provider::Codex, &signed_out);
        let (model, reason, _) = s.host.job_model(&brief).unwrap();
        assert_eq!(model.provider, Provider::Claude);
        assert!(
            reason.contains("Personal's Job row falls back to this Mac's: Codex is not signed in"),
            "{reason}"
        );
    }

    #[test]
    fn each_title_keeps_one_path_under_the_jobs_board_folder_with_a_commit_per_run() {
        let mut s = jobs();
        let key = (s
            .host
            .save_job(recipe(&s.personal, "Release dashboard", "0 8 * * 1"))
            .unwrap())
        .recipe
        .key();
        for (version, body) in [(1, "<p>one</p>"), (2, "<p>two</p>")] {
            let run = s.host.run_now(&key).unwrap();
            let thread = s.thread_of(&run);
            let session = s.host.session(&thread.id).unwrap();
            let scratch = assistant::scratch_folder(s.home.path(), &thread.id);
            fs::write(scratch.join("dashboard.html"), body).unwrap();
            let args = json!({"path":"dashboard.html","title":"Release dashboard"});
            let saved = s.host.thread_tool(&session, "save_output", &args).unwrap();
            assert_eq!(saved["version"], version);
            // A run has no other tool.
            for tool in ["propose_job", "suggest_space", "project_brief"] {
                assert!(
                    s.host.thread_tool(&session, tool, &json!({})).is_err(),
                    "{tool}"
                );
            }
            s.host.job_run_ended(&thread.id, RunEnd::Finished).unwrap();
            let run = s.host.job_run(&run.id).unwrap();
            assert_eq!(run.outputs[0].kept, "In Personal's storage");
        }
        let folder = format!(
            "spaces/{}/boards/release-dashboard",
            s.personal.storage.folder
        );
        let file = s
            .storage()
            .join(&folder)
            .join("release-dashboard/dashboard.html");
        assert_eq!(fs::read_to_string(file).unwrap(), "<p>two</p>");
        let metadata = format!("{folder}/release-dashboard.json");
        let commits = crate::test_support::git(
            &s.storage(),
            &["rev-list", "--count", "HEAD", "--", &metadata],
        );
        assert_eq!(commits.trim(), "2");
    }

    static COMPILES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    #[test]
    fn a_built_in_job_catches_up_once_and_no_command_reaches_it() {
        let mut s = jobs();
        s.host.built_ins.push(BuiltIn {
            name: "brain-compile".into(),
            subject: "Personal".into(),
            owner: "the Personal brain".into(),
            schedule: Some(Schedule {
                cron: "0 2 * * *".into(),
                timezone: "America/New_York".into(),
            }),
            run: |_, subject, _| {
                assert_eq!(subject, "Personal");
                COMPILES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok("compiled".into())
            },
        });
        let key = JobKey::built_in("brain-compile");
        s.host.fire_and_check(ny("2026-10-30 01:00")).unwrap();
        // Asleep from 01:00 to 07:00.
        let pass = s.host.fire_and_check(ny("2026-10-30 07:00")).unwrap();
        assert_eq!(pass.next, Some(ny("2026-10-31 02:00")));
        let runs = s.host.job_runs(&key, 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(
            (runs[0].outcome, runs[0].catch_up),
            (RunOutcome::Finished, 1)
        );
        assert_eq!(runs[0].reason, "For the Personal brain: compiled");
        assert_eq!(COMPILES.load(std::sync::atomic::Ordering::SeqCst), 1);
        // No Space lists it, and Run now is all a command can do to it.
        assert!(s.host.job_states().unwrap().is_empty());
        s.host.run_now(&key).unwrap();
        assert_eq!(COMPILES.load(std::sync::atomic::Ordering::SeqCst), 2);
        for command in [
            Command::SetJobEnabled {
                space_id: s.personal.id.clone(),
                job_id: "brain-compile".into(),
                enabled: false,
            },
            Command::DeleteJob {
                space_id: s.personal.id.clone(),
                job_id: "brain-compile".into(),
            },
        ] {
            assert!(s.host.execute(command).is_err());
        }
        // A recipe of the same name is the Space's own job, never the built-in one.
        let saved = s
            .host
            .save_job(recipe(&s.personal, "Brain compile", "0 2 * * *"))
            .unwrap();
        assert_ne!(saved.recipe.key(), key);
        assert!(s.host.built_in(&key).is_some());
    }

    #[test]
    fn in_a_space_on_github_only_the_runner_runs_a_job_and_keeps_its_output_here() {
        let mut s = jobs();
        let mut space = s.personal.clone();
        space.storage.repository = StorageRepository::GitHub {
            owner: "Sage-Social".into(),
            name: "wiffletree-storage".into(),
        };
        (s.host.db)
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![space.id.as_str(), encode(&space).unwrap()],
            )
            .unwrap();
        let saved = s
            .host
            .save_job(recipe(&space, "Release dashboard", "0 8 * * 1"))
            .unwrap();
        let runner = saved.recipe.runner.clone().unwrap();
        assert_eq!(
            runner.machine,
            sync::machine_name(),
            "its creator's Mac runs it"
        );
        assert_eq!(saved.not_here, None);
        // The file waits for the next pass to push it.
        let file = recipe_file(&space.storage.folder, "release-dashboard");
        assert!(
            s.host
                .root_folder(&space.storage.repository)
                .unwrap()
                .join(&file)
                .exists()
        );

        let key = saved.recipe.key();
        let run = s.host.run_now(&key).unwrap();
        let thread = s.thread_of(&run);
        let scratch = assistant::scratch_folder(s.home.path(), &thread.id);
        fs::write(scratch.join("dashboard.html"), "<p>one</p>").unwrap();
        let session = s.host.session(&thread.id).unwrap();
        let args = json!({"path":"dashboard.html","title":"Release dashboard"});
        let saved_output = s.host.thread_tool(&session, "save_output", &args).unwrap();
        assert_eq!(saved_output["kept"], "Only on this Mac");
        let private = assistant::board_store(s.home.path())
            .join(space.id.as_str())
            .join("_jobs/release-dashboard/release-dashboard/dashboard.html");
        assert!(private.exists());
        s.host.job_run_ended(&thread.id, RunEnd::Finished).unwrap();

        // Another Mac's job does not run here, on time or by Run now.
        let mut theirs = saved.recipe;
        theirs.runner = Some(Runner {
            login: "jamie".into(),
            machine: "Jamie's Mac".into(),
        });
        s.host.save_job(theirs).unwrap();
        s.host.fire_and_check(ny("2026-11-02 07:00")).unwrap();
        s.host.fire_and_check(ny("2026-11-02 08:00")).unwrap();
        assert_eq!(s.host.job_runs(&key, 10).unwrap().len(), 1);
        let refused = s.host.run_now(&key).unwrap_err().to_string();
        assert!(refused.contains("Runs on Jamie's Mac"), "{refused}");
    }

    #[test]
    fn a_pulled_recipe_must_name_its_own_space_and_be_readable() {
        let mut s = jobs();
        let mut other = recipe(&s.personal, "Brief", "0 7 * * *");
        other.space_id = SpaceId("someone-else".into());
        let content = recipe_yaml(&other).unwrap();
        assert!(
            s.host
                .take_pulled_recipe(&s.personal.clone(), "brief", Some(&content))
                .is_err()
        );
        let path = format!("spaces/{}/jobs/brief.yaml", s.personal.storage.folder);
        assert!(storage::check(&path, b"name: [").is_err());
        let mine = recipe_yaml(&recipe(&s.personal, "Brief", "0 7 * * *")).unwrap();
        storage::check(&path, &mine).unwrap();
        s.host
            .take_pulled_recipe(&s.personal.clone(), "brief", Some(&mine))
            .unwrap();
        assert_eq!(s.host.recipes().unwrap().len(), 1);
        s.host
            .take_pulled_recipe(&s.personal.clone(), "brief", None)
            .unwrap();
        assert!(s.host.recipes().unwrap().is_empty());
    }

    #[test]
    fn stop_ends_a_run_and_a_restart_ends_one_it_cut_off() {
        let mut s = jobs();
        let key = (s
            .host
            .save_job(recipe(&s.personal, "Brief", "0 7 * * *"))
            .unwrap())
        .recipe
        .key();
        let run = s.host.run_now(&key).unwrap();
        assert!(s.host.run_now(&key).is_err(), "one run at a time");
        assert_eq!(s.host.stop_job_run(&run.id).unwrap(), None);
        let stopped = s.host.job_run(&run.id).unwrap();
        assert_eq!(
            (stopped.outcome, stopped.reason.as_str()),
            (RunOutcome::Stopped, "You stopped the run.")
        );
        let thread = run.thread_id.unwrap();
        assert_eq!(
            s.host.messages(&thread, None, 10).unwrap()[0].receipt,
            Receipt::Completed
        );
        assert!(!s.host.note_long_job_run(&thread, 30).unwrap());

        let run = s.host.run_now(&key).unwrap();
        let thread = run.thread_id.clone().unwrap();
        assert!(s.host.note_long_job_run(&thread, 30).unwrap());
        assert_eq!(s.host.job_run(&run.id).unwrap().long_minutes, Some(30));
        let prompt = s.host.messages(&thread, None, 10).unwrap().remove(0);
        s.host
            .advance_receipt(&prompt.id, Receipt::Delivered)
            .unwrap();
        drop(s.host);
        let host = Host::open_set_up(s.home.path()).unwrap();
        let ended = host.job_run(&run.id).unwrap();
        assert_eq!(ended.outcome, RunOutcome::Failed);
        assert_eq!(ended.reason, "Wiffletree stopped during the run.");
    }

    #[test]
    fn a_recipe_changed_while_its_sources_are_checked_is_checked_again_before_it_starts() {
        let mut s = jobs();
        s.cli("gh", 0);
        let mut brief = recipe(&s.personal, "Brief", "0 7 * * *");
        brief.sources = vec![gh()];
        let key = s.host.save_job(brief.clone()).unwrap().recipe.key();
        let run = s.host.run_job_now(&key).unwrap();
        let (checked, checks) = s.host.take_source_checks().remove(0);
        brief.sources = vec![linear()];
        s.host.save_job(brief).unwrap();
        let found = checks.run();
        s.host.sources_checked(&checked, &checks, found).unwrap();
        let waiting = s.host.job_run(&run.id).unwrap();
        assert_eq!(
            (waiting.outcome, waiting.thread_id),
            (RunOutcome::Running, None)
        );
        let (_, again) = s.host.take_source_checks().remove(0);
        let found = again.run();
        s.host.sources_checked(&run.id, &again, found).unwrap();
        let ended = s.host.job_run(&run.id).unwrap();
        assert_eq!(ended.outcome, RunOutcome::NotStarted);
        assert!(
            ended.reason.contains("The linear MCP server"),
            "{}",
            ended.reason
        );
    }

    #[test]
    fn a_run_stopped_or_cut_off_while_its_sources_are_checked_never_starts() {
        let mut s = jobs();
        let key = (s.host.save_job(recipe(&s.personal, "Brief", "0 7 * * *")))
            .unwrap()
            .recipe
            .key();
        let run = s.host.run_job_now(&key).unwrap();
        let (checked, checks) = s.host.take_source_checks().remove(0);
        assert_eq!(s.host.stop_job_run(&run.id).unwrap(), None);
        let found = checks.run();
        s.host.sources_checked(&checked, &checks, found).unwrap();
        let stopped = s.host.job_run(&run.id).unwrap();
        assert_eq!(
            (stopped.outcome, stopped.thread_id),
            (RunOutcome::Stopped, None)
        );

        let run = s.host.run_job_now(&key).unwrap();
        drop(s.host);
        let host = Host::open_set_up(s.home.path()).unwrap();
        let ended = host.job_run(&run.id).unwrap();
        assert_eq!(ended.outcome, RunOutcome::Failed);
        assert_eq!(ended.reason, "Wiffletree stopped during the run.");
    }

    /// Shared jobs (stage 5), in Sage, stored on a local stand-in for GitHub.
    mod shared {
        use super::*;
        use crate::storage::tests::{host, join_sage, shared_storage, space_named, sync};
        use crate::sync::tests::{GitHub, NAME};
        use std::sync::Arc;
        use workspace_core::board::{BoardOutput, Keeping, LATEST};

        const OUTPUT: &str =
            "spaces/sage/boards/release-dashboard/release-dashboard/dashboard.html";
        const ABOUT: &str = "spaces/sage/boards/release-dashboard/release-dashboard.json";
        const TITLE: &str = "Release dashboard";

        struct Shared {
            _folder: tempfile::TempDir,
            _home: tempfile::TempDir,
            host: Host,
            github: Arc<GitHub>,
            sage: Space,
        }
        /// This Mac with Sage joined; `settings` makes its storage the settings repository too.
        fn shared(settings: bool) -> Shared {
            let folder = tempfile::tempdir().unwrap();
            let github = shared_storage(folder.path(), Some("SAGE"));
            let (home, mut host) = host();
            if settings {
                (host.choose_settings_repository(NAME, None, &github.tools())).unwrap();
            }
            join_sage(&mut host, &github).unwrap();
            let sage = space_named(&host, "Sage");
            Shared {
                _folder: folder,
                _home: home,
                host,
                github,
                sage,
            }
        }
        impl Shared {
            /// A pass of the storage, by the settings pass when it is the settings repository.
            fn sync(&mut self) {
                match self.host.settings_key() {
                    Some(_) => self.host.sync_now(&self.github.tools()).unwrap(),
                    None => sync(&mut self.host, &self.github),
                }
            }
            /// Runs `key` now, saving `body` as its dashboard, to its end.
            fn run_saving(&mut self, key: &JobKey, body: &str) -> JobRun {
                self.run_saving_as(key, "dashboard.html", body)
            }
            /// Runs `key` now, saving `body` as `file`, titled as the dashboard, to its end.
            fn run_saving_as(&mut self, key: &JobKey, file: &str, body: &str) -> JobRun {
                let run = self.host.run_now(key).unwrap();
                let thread = run.thread_id.clone().unwrap();
                let scratch = assistant::scratch_folder(&self.host.home, &thread);
                fs::write(scratch.join(file), body).unwrap();
                let session = self.host.session(&thread).unwrap();
                let args = json!({"path":file,"title":TITLE});
                (self.host.thread_tool(&session, "save_output", &args)).unwrap();
                self.host.job_run_ended(&thread, RunEnd::Finished).unwrap();
                self.host.job_run(&run.id).unwrap()
            }
            fn boards(&self) -> Vec<BoardOutput> {
                self.host.shelves().unwrap().list()
            }
            /// The IDs of Sage's issues listed here.
            fn issues(&self) -> Vec<String> {
                let listed = self.host.list_issues(Some(&self.sage.id), None).unwrap();
                listed
                    .into_iter()
                    .map(|issue| issue.id.to_string())
                    .collect()
            }
            fn paused(&self) -> Vec<String> {
                (self
                    .host
                    .db
                    .prepare("SELECT path FROM sync_paused")
                    .unwrap())
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
            }
            /// A version of the dashboard another Mac pushed, saved at `saved_at`.
            fn pushed_elsewhere(&self, body: &str, saved_at: i64) {
                self.github.commit(OUTPUT, body);
                let about = json!({"title":TITLE,"file":"dashboard.html","saved_at":saved_at});
                self.github.commit(ABOUT, about.to_string());
            }
            /// An approved runner here, its first output shared.
            fn approved(&mut self) -> JobKey {
                let key = (self.host.save_job(dashboard(&self.sage)))
                    .unwrap()
                    .recipe
                    .key();
                let run = self.run_saving(&key, "<p>first</p>");
                (self.host.approve_job_output(&run.id, TITLE, true)).unwrap();
                self.sync();
                key
            }
        }
        fn dashboard(sage: &Space) -> Recipe {
            recipe(sage, TITLE, "0 8 * * 1")
        }
        fn jamies(sage: &Space) -> Recipe {
            Recipe {
                runner: Some(Runner {
                    login: "jamie".into(),
                    machine: "Jamie's Mac".into(),
                }),
                ..dashboard(sage)
            }
        }

        #[test]
        fn a_first_output_waits_for_the_runner_and_later_ones_push_until_the_prompt_changes() {
            let mut s = shared(false);
            let saved = s.host.save_job(dashboard(&s.sage)).unwrap();
            let key = saved.recipe.key();
            let run = s.run_saving(&key, "<p>one</p>");
            assert_eq!(
                (run.outputs[0].waiting, run.outputs[0].kept.as_str()),
                (true, "Only on this Mac")
            );
            assert_eq!(s.host.job_state(&saved.recipe).unwrap().attention, 1);
            s.sync();
            assert_eq!(
                s.github.show(OUTPUT),
                None,
                "nothing is pushed before Share"
            );
            let listed = s.boards();
            assert_eq!(listed.len(), 1);
            assert!(listed[0].waiting && listed[0].keeping == Keeping::Private);
            assert_eq!(listed[0].run_id.as_deref(), Some(run.id.as_str()));
            let around = s.host.share_board(&listed[0].id, &s.sage.id).unwrap_err();
            assert!(format!("{around:#}").contains("waits for your approval"));

            // Keep private leaves it here and the job unapproved.
            (s.host.approve_job_output(&run.id, TITLE, false)).unwrap();
            assert!(!s.host.job_run(&run.id).unwrap().outputs[0].waiting);
            s.sync();
            assert_eq!(s.github.show(OUTPUT), None);
            let replaced = s.run_saving(&key, "<p>replaced</p>");
            let run = s.run_saving(&key, "<p>two</p>");
            assert!(run.outputs[0].waiting);
            // An answer reaches only the latest output, which replaced the one before.
            for share in [false, true] {
                let refused = (s.host.approve_job_output(&replaced.id, TITLE, share)).unwrap_err();
                assert!(format!("{refused:#}").contains("A newer output replaced"));
            }

            // Share pushes it and moves it out of the private store.
            (s.host.approve_job_output(&run.id, TITLE, true)).unwrap();
            assert!(s.host.approve_job_output(&run.id, TITLE, true).is_err());
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>two</p>"));
            let listed = s.boards();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].keeping, Keeping::SharedStorage);
            assert!(!listed[0].waiting && listed[0].runner.is_some());

            let run = s.run_saving(&key, "<p>three</p>");
            assert_eq!(
                (run.outputs[0].waiting, run.outputs[0].kept.as_str()),
                (false, "Shared in Sage")
            );
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>three</p>"));

            // A changed prompt, or changed sources, wait again.
            let changed = Recipe {
                prompt: "Brief me on the release.".into(),
                ..saved.recipe
            };
            let changed = s.host.save_job(changed).unwrap().recipe;
            let run = s.run_saving(&key, "<p>four</p>");
            assert!(run.outputs[0].waiting);
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>three</p>"));

            // So does a teammate's change, pulled.
            (s.host.approve_job_output(&run.id, TITLE, true)).unwrap();
            s.sync();
            let theirs = Recipe {
                prompt: "Brief me on the release, with crash rates.".into(),
                ..changed
            };
            let file = recipe_file("sage", "release-dashboard");
            s.github.commit(&file, recipe_yaml(&theirs).unwrap());
            s.sync();
            assert_eq!(s.host.recipe(&key).unwrap().prompt, theirs.prompt);
            let run = s.run_saving(&key, "<p>five</p>");
            assert!(run.outputs[0].waiting);
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>four</p>"));

            // And changed sources, with the prompt as it was.
            (s.host.approve_job_output(&run.id, TITLE, true)).unwrap();
            s.sync();
            let bin = s.host.home.join("bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("gh"), "#!/bin/sh\nexit 0\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(bin.join("gh"), fs::Permissions::from_mode(0o755)).unwrap();
            s.host.source_env = Some(SourceEnv {
                path: vec![bin],
                home: s.host.home.join("user"),
                mcp_limit: std::time::Duration::from_secs(2),
            });
            let sourced = Recipe {
                sources: vec![gh()],
                ..s.host.recipe(&key).unwrap()
            };
            s.host.save_job(sourced).unwrap();
            let run = s.run_saving(&key, "<p>six</p>");
            assert!(run.outputs[0].waiting, "{run:?}");
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>five</p>"));
        }

        /// The runner rule set, in a storage repository of its own and in one that is also the
        /// settings repository.
        fn a_runners_output_pushes_without_comparing_and_again_when_github_moves(settings: bool) {
            let mut s = shared(settings);
            let key = s.approved();
            // A former runner's version lands on GitHub while this Mac saves its own.
            s.pushed_elsewhere("<p>from Jamie</p>", now());
            s.run_saving(&key, "<p>mine</p>");
            let github = s.github.clone();
            *s.github.before.lock().unwrap() = Some((
                "push",
                Box::new(move || github.commit("README.md", "Changed meanwhile.\n")),
            ));
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>mine</p>"));
            assert_eq!(
                s.github.show("README.md").as_deref(),
                Some("Changed meanwhile.\n")
            );
            assert_eq!(s.paused(), Vec::<String>::new());
            let status = s.host.storage_status(&s.sage.id).unwrap();
            assert_eq!(status.error, None);
        }
        #[test]
        fn a_runners_output_pushes_without_comparing_and_again_when_github_moves_under_it() {
            a_runners_output_pushes_without_comparing_and_again_when_github_moves(false);
        }
        #[test]
        fn in_storage_that_is_the_settings_repository_a_runners_output_pushes_the_same() {
            a_runners_output_pushes_without_comparing_and_again_when_github_moves(true);
        }

        /// One pass with a runner's output and a new issue, while another Mac files SAGE-1
        /// before the push. Returns the issues listed here after it, and the new issue's ID.
        fn an_output_and_a_new_issue_while_github_moves(s: &mut Shared) -> (Vec<String>, String) {
            let key = s.approved();
            s.host.record_gh_login(Some("jlin".into()));
            let new = NewIssue {
                title: "Watch app drops replies".into(),
                ..NewIssue::default()
            };
            let filed = s.host.create_issue(&s.sage.id, new).unwrap().id;
            s.run_saving(&key, "<p>mine</p>");
            let github = s.github.clone();
            let theirs = tracker::issue_md(&tracker::tests::blurry("SAGE-1")).unwrap();
            *s.github.before.lock().unwrap() = Some((
                "push",
                Box::new(move || github.commit("issues/SAGE-1/issue.md", theirs)),
            ));
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>mine</p>"));
            let theirs = s.github.show("issues/SAGE-1/issue.md").unwrap();
            assert!(!theirs.contains("Watch app drops replies"), "{theirs}");
            (s.issues(), filed.to_string())
        }
        #[test]
        fn an_output_and_a_new_issue_push_again_together_and_the_issue_takes_the_next_number() {
            let mut s = shared(false);
            let (listed, filed) = an_output_and_a_new_issue_while_github_moves(&mut s);
            let pushed = s.github.show("issues/SAGE-2/issue.md").unwrap();
            assert!(
                pushed.contains("title: Watch app drops replies"),
                "{pushed}"
            );
            assert_eq!(listed, ["SAGE-1", "SAGE-2"], "{filed} took SAGE-2");
        }
        #[test]
        fn in_the_settings_repository_a_new_issue_sits_out_the_outputs_retry_unnumbered() {
            let mut s = shared(true);
            let (listed, filed) = an_output_and_a_new_issue_while_github_moves(&mut s);
            assert_eq!(s.github.show("issues/SAGE-2/issue.md"), None);
            // Not SAGE-1, which the issue's commit took on the head it was refused on.
            assert_eq!(listed, [filed], "it waits under its temporary ID");
            // The next pass pulls SAGE-1 and numbers it on the head it pushes to.
            s.sync();
            let pushed = s.github.show("issues/SAGE-2/issue.md").unwrap();
            assert!(
                pushed.contains("title: Watch app drops replies"),
                "{pushed}"
            );
            assert_eq!(s.issues(), ["SAGE-1", "SAGE-2"]);
        }

        #[test]
        fn of_two_saves_before_a_push_the_newest_is_pushed_and_the_other_kept_here() {
            let mut s = shared(false);
            let key = s.approved();
            let two = s.run_saving(&key, "<p>two</p>");
            let three = s.run_saving(&key, "<p>three</p>");
            assert_eq!((two.outputs[0].version, three.outputs[0].version), (2, 3));
            s.sync();
            assert_eq!(s.pushed_versions(ABOUT), 2, "first and three");
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>three</p>"));
            let shared = s.shared_output();
            let shelves = s.host.shelves().unwrap();
            let versions = shelves.versions(&shared.id).unwrap();
            let kept: Vec<_> = versions.iter().filter(|v| v.local_only).collect();
            assert_eq!((versions.len(), kept.len()), (3, 1));
            let folder = shelves.version_folder(&shared.id, &kept[0].commit).unwrap();
            let file = folder.join("dashboard.html");
            assert_eq!(fs::read_to_string(file).unwrap(), "<p>two</p>");
            assert_eq!(
                s.boards().len(),
                1,
                "a kept save is a version, not an output"
            );
        }

        #[test]
        fn the_picker_starts_with_the_current_output_before_and_after_a_push() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>two</p>");
            s.run_saving(&key, "<p>three</p>");
            let shared = s.shared_output();
            // Each version's label and what picking it serves.
            let picker = |s: &Shared| {
                let shelves = s.host.shelves().unwrap();
                let versions = shelves.versions(&shared.id).unwrap();
                (versions.into_iter())
                    .map(|v| {
                        let folder = shelves.version_folder(&shared.id, &v.commit).unwrap();
                        let body = fs::read_to_string(folder.join("dashboard.html")).unwrap();
                        (v.commit == LATEST, v.local_only, body)
                    })
                    .collect::<Vec<_>>()
            };
            let version = |latest, local_only, body: &str| (latest, local_only, body.to_owned());
            // Version 1 is pushed, 2 is only here and 3 is not committed anywhere yet.
            assert_eq!(
                picker(&s),
                [
                    version(true, false, "<p>three</p>"),
                    version(false, true, "<p>two</p>"),
                    version(false, false, "<p>first</p>"),
                ]
            );
            // Pushed, 3 is a commit, which the latest stands for rather than repeats.
            s.sync();
            assert_eq!(
                picker(&s),
                [
                    version(true, false, "<p>three</p>"),
                    version(false, true, "<p>two</p>"),
                    version(false, false, "<p>first</p>"),
                ]
            );
        }

        impl Shared {
            fn edit_outside(&self, body: &str) {
                let clone = self.host.storage_folder(&self.sage.storage.repository);
                fs::write(clone.join(OUTPUT), body).unwrap();
            }
            fn pushed_versions(&self, path: &str) -> usize {
                let args = ["log", "--format=%H", "main", "--", path];
                crate::test_support::git(&self.github.bare, &args)
                    .lines()
                    .count()
            }
            fn shared_output(&self) -> BoardOutput {
                (self.boards().into_iter())
                    .find(|output| output.keeping == Keeping::SharedStorage)
                    .unwrap()
            }
            /// The shared output's versions kept only on this Mac.
            fn kept_versions(&self) -> usize {
                let shelves = self.host.shelves().unwrap();
                let versions = shelves.versions(&self.shared_output().id).unwrap();
                versions.iter().filter(|v| v.local_only).count()
            }
        }

        #[test]
        fn a_save_over_a_shared_copy_edited_outside_wiffletree_stays_here_and_nothing_is_pushed() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>two</p>");
            s.edit_outside("<p>outside</p>");
            let run = s.run_saving(&key, "<p>three</p>");
            assert_eq!(run.outputs[0].kept, "Only on this Mac");
            assert_eq!(
                s.kept_versions(),
                0,
                "nothing is kept or replaced over an outside edit"
            );
            let clone = s.host.storage_folder(&s.sage.storage.repository);
            assert_eq!(
                fs::read_to_string(clone.join(OUTPUT)).unwrap(),
                "<p>outside</p>"
            );
            s.sync();
            assert_eq!(s.paused(), [OUTPUT]);
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>first</p>"));
            assert_eq!(
                (s.pushed_versions(OUTPUT), s.pushed_versions(ABOUT)),
                (1, 1)
            );
        }

        #[test]
        fn an_output_with_a_file_paused_this_pass_pushes_none_of_its_files() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>two</p>");
            s.run_saving(&key, "<p>three</p>");
            assert_eq!(s.kept_versions(), 1);
            s.edit_outside("<p>outside</p>");
            s.sync();
            assert_eq!(s.paused(), [OUTPUT]);
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>first</p>"));
            assert_eq!(
                (s.pushed_versions(OUTPUT), s.pushed_versions(ABOUT)),
                (1, 1)
            );
        }

        #[test]
        fn a_newest_save_equal_to_githubs_is_githubs_latest_tree() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>changed</p>");
            let newest = s.run_saving(&key, "<p>first</p>");
            s.sync();
            assert_eq!(s.pushed_versions(ABOUT), 2);
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>first</p>"));
            let about: Value = serde_json::from_str(&s.github.show(ABOUT).unwrap()).unwrap();
            assert_eq!(about["run_id"], json!(newest.id));
            assert_eq!(s.kept_versions(), 1);
            s.sync();
            let clone = s.host.storage_folder(&s.sage.storage.repository);
            assert_eq!(
                fs::read_to_string(clone.join(OUTPUT)).unwrap(),
                "<p>first</p>"
            );
        }

        #[test]
        fn a_paused_json_output_file_holds_its_whole_output_with_its_metadata() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>two</p>");
            s.run_saving_as(&key, "dashboard.json", "{\"three\":3}");
            assert_eq!(s.kept_versions(), 1);
            let json = OUTPUT.replace("dashboard.html", "dashboard.json");
            let clone = s.host.storage_folder(&s.sage.storage.repository);
            fs::write(clone.join(&json), "{\"outside\":true}").unwrap();
            s.sync();
            assert_eq!(s.paused(), std::slice::from_ref(&json));
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>first</p>"));
            assert_eq!(s.github.show(&json), None);
            assert_eq!(
                (s.pushed_versions(OUTPUT), s.pushed_versions(ABOUT)),
                (1, 1)
            );
            assert_eq!(s.kept_versions(), 1);
        }

        #[test]
        fn a_retry_for_a_runners_output_keeps_the_pauses_of_the_rule_sets_that_sit_it_out() {
            let mut s = shared(false);
            let key = s.approved();
            s.run_saving(&key, "<p>two</p>");
            let file = "spaces/sage/space.yaml";
            let here = Space {
                description: "Changed here".into(),
                ..s.sage.clone()
            };
            s.host.update_space(here).unwrap();
            let theirs = s
                .github
                .show(file)
                .unwrap()
                .replace("Secure family messaging", "Changed by a teammate");
            s.github.commit(file, theirs);
            let github = s.github.clone();
            *s.github.before.lock().unwrap() = Some((
                "push",
                Box::new(move || github.commit("README.md", "Changed meanwhile.\n")),
            ));
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>two</p>"));
            assert_eq!(s.paused(), [file]);
            assert!(
                s.github
                    .show(file)
                    .unwrap()
                    .contains("Changed by a teammate")
            );
        }

        #[test]
        fn an_approved_push_ends_the_wait_of_an_older_output_of_its_title() {
            let mut s = shared(false);
            let key = s.approved();
            let approved = s.host.recipe(&key).unwrap();
            let changed = Recipe {
                prompt: "Brief me on the release.".into(),
                ..approved.clone()
            };
            s.host.save_job(changed).unwrap();
            let older = s.run_saving(&key, "<p>older</p>");
            assert!(older.outputs[0].waiting);
            s.host.save_job(approved).unwrap();
            let newer = s.run_saving(&key, "<p>newer</p>");
            assert!(!newer.outputs[0].waiting);
            assert!(!s.host.job_run(&older.id).unwrap().outputs[0].waiting);
            let refused = (s.host.approve_job_output(&older.id, TITLE, true)).unwrap_err();
            assert!(format!("{refused:#}").contains("no longer waits"));
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>newer</p>"));
        }

        #[test]
        fn another_macs_output_is_pulled_and_pauses_only_over_a_change_here() {
            let mut s = shared(false);
            let saved = s.host.save_job(jamies(&s.sage)).unwrap();
            assert_eq!(saved.not_here.as_deref(), Some("Runs on Jamie's Mac"));
            let saved_at = now() - 60_000;
            s.pushed_elsewhere("<p>Jamie's</p>", saved_at);
            s.sync();
            assert_eq!(s.paused(), Vec::<String>::new());
            let listed = s.boards();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].keeping, Keeping::SharedStorage);
            assert_eq!(listed[0].runner, jamies(&s.sage).runner);
            let state = s.host.job_state(&saved.recipe).unwrap();
            assert_eq!(state.last_run_at, Some(saved_at));
            assert!(state.can_take_over());

            s.host
                .write_storage_files(&s.sage, &[(OUTPUT.into(), Some(b"<p>here</p>"))], "edit")
                .unwrap();
            s.github.commit(OUTPUT, "<p>Jamie's second</p>");
            s.sync();
            assert_eq!(s.paused(), [OUTPUT]);
            assert_eq!(
                s.github.show(OUTPUT).as_deref(),
                Some("<p>Jamie's second</p>")
            );
        }

        #[test]
        fn taking_over_a_stale_job_is_a_commit_and_its_first_output_here_waits() {
            let mut s = shared(false);
            s.host.save_job(jamies(&s.sage)).unwrap();
            s.pushed_elsewhere("<p>Jamie's</p>", now() - 15 * 24 * 3_600_000);
            s.sync();
            assert!(s.boards()[0].stale, "two Mondays passed without a version");
            let state = (s.host.take_over_job(&s.sage.id, "release-dashboard")).unwrap();
            let runner = state.recipe.runner.clone().unwrap();
            assert_eq!(runner.machine, sync::machine_name());
            assert!(state.not_here.is_none() && !state.can_take_over());
            assert!(
                s.host
                    .take_over_job(&s.sage.id, "release-dashboard")
                    .is_err()
            );
            s.sync();
            let pushed = s
                .github
                .show("spaces/sage/jobs/release-dashboard.yaml")
                .unwrap();
            assert!(pushed.contains(&sync::machine_name()), "{pushed}");
            let run = s.run_saving(&state.recipe.key(), "<p>mine</p>");
            assert!(run.outputs[0].waiting);
            s.sync();
            assert_eq!(s.github.show(OUTPUT).as_deref(), Some("<p>Jamie's</p>"));
        }

        #[test]
        fn everyone_runs_their_own_and_keeps_the_result_on_this_mac() {
            let mut s = shared(false);
            let template = Recipe {
                sharing: Sharing::Everyone,
                ..dashboard(&s.sage)
            };
            let saved = s.host.save_job(template).unwrap();
            assert_eq!((&saved.recipe.runner, &saved.not_here), (&None, &None));
            let run = s.run_saving(&saved.recipe.key(), "<p>my queue</p>");
            assert_eq!(
                (run.outputs[0].waiting, run.outputs[0].kept.as_str()),
                (false, "Only on this Mac")
            );
            s.sync();
            assert_eq!(s.github.show(OUTPUT), None);
            assert_eq!(s.boards()[0].keeping, Keeping::Private);
        }
    }
}
