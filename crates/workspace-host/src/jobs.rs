//! Scheduled jobs (stage 3): recipes the user saved, the scheduler that runs them and built-in
//! host jobs on their due times, one catch-up run after missed ones, the check of a run's
//! sources, and run records. A recipe is a file in its Space's storage; the store keeps each as
//! the user saved it, or as sync pulled it from another member's save, and only that copy runs.
//! Jobs belong to the user: no tool, thread or run saves, enables or changes one.
use crate::*;
use anyhow::anyhow;
use chrono::{Datelike, Duration as Days, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::Deserialize;
use workspace_core::assistant::{JobRunOf, Thread};
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
         CREATE INDEX IF NOT EXISTS job_runs_by_key ON job_runs(key, started_at);",
    )?;
    Ok(())
}

/// A job Wiffletree defines, such as a brain's compile: it takes due times, catch-up, Run now
/// and run records from the scheduler, and keeps its own owner, trigger and run. It is no
/// recipe and in no Space; only the host's code registers one.
pub(crate) struct BuiltIn {
    pub(crate) name: &'static str,
    /// Who it belongs to, such as "the Sage brain".
    pub(crate) owner: String,
    pub(crate) schedule: Schedule,
    /// Its own trigger beside its due times: a run starts when this says so.
    pub(crate) triggered: fn(&Host) -> bool,
    /// Runs it by its own rules for sessions and models, and says what it did.
    pub(crate) run: fn(&mut Host) -> Result<String>,
}
impl BuiltIn {
    pub(crate) fn key(&self) -> JobKey {
        JobKey::built_in(self.name)
    }
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

/// `spaces/<folder>/jobs/<id>.yaml`, relative to the storage repository.
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
fn times_between(cron: &Cron, zone: Tz, from: i64, to: i64) -> u32 {
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
            Some(runner) if runner.machine == sync::machine_name() => None,
            Some(runner) => Some(format!("Runs on {}", runner.machine)),
            None => Some("No Mac is its runner".into()),
        }
    }
    /// The model a run of `recipe` starts on: the recipe's, else its Space's default for jobs
    /// when this Mac may use it, else Who runs what's Job row, or the Assistant row without
    /// one. A recipe with a CLI source runs only on Claude, which can hold the CLI to its
    /// listed commands; Codex cannot yet (design, decision 9).
    pub(crate) fn job_model(&self, recipe: &Recipe) -> Result<(ModelProfile, String, bool)> {
        let record = self.space(&recipe.space_id)?;
        let cli = (recipe.sources.iter()).find_map(|source| match source {
            Source::Cli { command, .. } => Some(command.clone()),
            Source::Mcp { .. } => None,
        });
        let holds = |profile: &ModelProfile| cli.is_none() || profile.provider == Provider::Claude;
        let codex_note = |cli: &str| format!("Codex cannot yet hold {cli} to read commands");
        let mut notes = vec![];
        let space_default = (record.space.models.jobs.as_ref()).map(|m| m.profile());
        let candidates = [
            (recipe.model.clone(), "Chosen in the recipe".to_owned()),
            (
                space_default,
                format!("{}'s default for jobs", record.space.name),
            ),
        ];
        for (profile, reason) in candidates {
            let Some(profile) = profile else { continue };
            match (self.may_use(&record.local, &profile), holds(&profile)) {
                (true, true) => return Ok((profile, reason, false)),
                (false, _) => notes.push(format!(
                    "{reason}, {}, is not available on this Mac.",
                    profile.short()
                )),
                (true, false) => notes.push(format!(
                    "{reason}, {}, is on Codex, and {}.",
                    profile.short(),
                    codex_note(cli.as_deref().unwrap_or_default())
                )),
            }
        }
        let selection = self.model_selection()?;
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
        let pick = match &cli {
            Some(_) => self.resolve_job_without(&Job::Job, Provider::Codex)?,
            None => self.resolve_job(&Job::Job, false, false)?,
        };
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
        let key = recipe.key();
        let existed = self.recipe(&key).is_ok();
        let message = match existed {
            true => format!("{}: changed the job {}", record.space.name, recipe.name),
            false => format!("{}: added the job {}", record.space.name, recipe.name),
        };
        self.atomically(|host| {
            host.db.execute(
                "INSERT INTO jobs VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET data=excluded.data",
                params![key.as_str(), recipe.space_id.as_str(), encode(&recipe)?],
            )?;
            host.db
                .execute("DELETE FROM job_due WHERE key=?1", [key.as_str()])?;
            host.write_recipe_file(
                &record.space,
                &recipe.id,
                Some(&recipe_yaml(&recipe)?),
                &message,
            )
        })?;
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
            for table in ["jobs", "job_due", "job_runs"] {
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
            for table in ["jobs", "job_due"] {
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
        let model = match self.job_model(recipe) {
            Ok((profile, reason, _)) => format!("{} · {reason}", profile.short()),
            Err(error) => format!("{error:#}"),
        };
        // Counted apart from the runs shown, so an old unseen failure still counts.
        let (attention, attention_at): (usize, Option<i64>) = self.db.query_row(
            "SELECT count(*), max(started_at) FROM job_runs WHERE key=?1 AND json_extract(data,'$.seen')=0 AND json_extract(data,'$.outcome') IN ('failed','not_started')",
            [recipe.key().as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(JobState {
            attention,
            attention_at,
            schedule: schedule_words(&recipe.schedule),
            next_runs: match recipe.enabled {
                true => next_times(recipe, now(), NEXT_SHOWN),
                false => vec![],
            },
            model,
            runs: self.job_runs(&recipe.key(), MAX_RUNS_SHOWN)?,
            not_here: self.not_here(recipe, &space),
            recipe: recipe.clone(),
        })
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
        due.extend((self.built_ins.iter()).map(|b| (b.key(), b.schedule.clone())));
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
        let triggered: Vec<JobKey> = (self.built_ins.iter())
            .filter(|b| (b.triggered)(self))
            .map(BuiltIn::key)
            .collect();
        for key in triggered {
            self.run_job(&key, None, 0, now)?;
            pass.changed = true;
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
            self.run_job(key, Some(due), catch_up, now)?;
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
        self.run_job(key, None, 0, now())
    }
    /// Records one run and, for a recipe, queues the check of its sources; `sources_checked`
    /// starts its thread or records why it did not start. A built-in job runs now.
    fn run_job(
        &mut self,
        key: &JobKey,
        scheduled: Option<i64>,
        catch_up: u32,
        at: i64,
    ) -> Result<JobRun> {
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
        };
        if let Some(built_in) = self.built_in(key) {
            let (run_it, owner) = (built_in.run, built_in.owner.clone());
            let result = run_it(self);
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
    /// A run stopped while its sources were checked stays stopped.
    pub(crate) fn sources_checked(&mut self, run_id: &str, checks: Vec<SourceCheck>) -> Result<()> {
        let mut run = self.job_run(run_id)?;
        if run.outcome != RunOutcome::Running || run.thread_id.is_some() {
            return Ok(());
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
    /// Keeps an output a job's run saved at its stable path: in the Space's storage when it is
    /// on this Mac only, and in this Mac's private board store for a Space stored on GitHub,
    /// whose first push waits for the runner's approval (stage 5).
    pub(crate) fn save_job_output(
        &mut self,
        thread: &Thread,
        job: &JobRunOf,
        title: &str,
        source: &Path,
    ) -> Result<RunOutput> {
        let recipe = self.recipe(&job.job)?;
        let space = self.space(&thread.space_id)?.space;
        let (repository, base, kept) = match space.storage.repository {
            StorageRepository::Local { .. } => (
                self.storage_folder(&space.storage.repository),
                format!("spaces/{}/boards/{}", space.storage.folder, recipe.id),
                format!("In {}'s storage", space.name),
            ),
            StorageRepository::GitHub { .. } => (
                assistant::board_store(&self.home),
                format!("{}/{}/{}", space.id, assistant::JOB_OUTPUTS, recipe.id),
                "Only on this Mac".to_owned(),
            ),
        };
        let about = json!({"title":title,"job":recipe.key(),"run_id":job.run_id,"saved_at":now()});
        let (file, version) = assistant::store_output(&repository, &base, title, source, about)?;
        let output = RunOutput {
            title: title.into(),
            file,
            version,
            kept,
        };
        let mut run = self.job_run(&job.run_id)?;
        run.outputs.retain(|o| o.title != title);
        run.outputs.push(output.clone());
        self.save_run(&run)?;
        Ok(output)
    }
}

pub(crate) const STOPPED_BY_YOU: &str = "Stopped: you stopped the run.";

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
        for (run, checks) in self.take_source_checks() {
            self.sources_checked(&run, checks.run()).unwrap();
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
            self.host.storage_folder(&self.personal.storage.repository)
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
        let text = s.host.thread_instructions(&session).unwrap();
        assert!(text.contains("# Job run core — v1"), "{text}");
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
            name: "brain-compile",
            owner: "the Personal brain".into(),
            schedule: Schedule {
                cron: "0 2 * * *".into(),
                timezone: "America/New_York".into(),
            },
            triggered: |_| false,
            run: |_| {
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
        assert_eq!(s.host.built_ins.len(), 1);
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
                .storage_folder(&space.storage.repository)
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
            (RunOutcome::Stopped, STOPPED_BY_YOU)
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
    fn a_run_stopped_or_cut_off_while_its_sources_are_checked_never_starts() {
        let mut s = jobs();
        let key = (s.host.save_job(recipe(&s.personal, "Brief", "0 7 * * *")))
            .unwrap()
            .recipe
            .key();
        let run = s.host.run_job_now(&key).unwrap();
        let (checked, checks) = s.host.take_source_checks().remove(0);
        assert_eq!(s.host.stop_job_run(&run.id).unwrap(), None);
        s.host.sources_checked(&checked, checks.run()).unwrap();
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
}
