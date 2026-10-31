//! Scheduled jobs (OpenSpec change `assistant-spaces-jobs-boards`, stage 3). A job is a recipe
//! the user saved, stored in its Space's storage as `spaces/<space>/jobs/<job>.yaml`; each run is
//! a fresh read-only thread of role `JobRun`. Built-in host jobs share the scheduler but are no
//! recipe: Wiffletree defines them, and no one saves, changes or deletes one.
use crate::space::Extra;
use crate::{ModelProfile, Source, SpaceId};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

const MAX_NAME_CHARS: usize = 80;
const MAX_PROMPT_BYTES: usize = 16 * 1024;
/// A recipe's file, as this release writes it.
pub const RECIPE_FORMAT: u32 = 1;

/// A saved thread recipe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    /// Its file's name in the Space's `jobs/` folder, without `.yaml`.
    pub id: String,
    pub name: String,
    /// Read from where the file is, never trusted from the file alone.
    pub space_id: SpaceId,
    /// The message each run starts with.
    pub prompt: String,
    pub schedule: Schedule,
    /// The model the user chose; `None` follows the Space's default for jobs, then Who runs
    /// what.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelProfile>,
    /// The only sources a run is offered, in place of its Space's.
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputKind>,
    #[serde(default)]
    pub sharing: Sharing,
    /// The one Mac that runs a "One runner" job in a Space stored on GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<Runner>,
    /// Off while the user has paused it.
    pub enabled: bool,
    /// The keys this release does not know, kept so that a write puts them back.
    #[serde(flatten)]
    pub extra: Extra,
}
impl Recipe {
    pub fn key(&self) -> JobKey {
        JobKey::recipe(&self.space_id, &self.id)
    }
}

/// Five cron fields, minute hour day-of-month month weekday, in an IANA timezone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub cron: String,
    pub timezone: String,
}

/// What a run's output is, when the recipe names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputKind {
    pub title: String,
    /// Such as `html` or `md`.
    pub file_type: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sharing {
    /// One runner, shared result: only the runner's Mac runs it.
    #[default]
    OneRunner,
    /// Everyone runs their own: a template each member's Mac runs, with a private result.
    Everyone,
}
impl Sharing {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneRunner => "One runner, shared result",
            Self::Everyone => "Everyone runs their own · private result",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runner {
    /// Empty when this Mac knew no GitHub login when the job was saved.
    #[serde(default)]
    pub login: String,
    pub machine: String,
}
impl Runner {
    /// "Runs on Jamie's Mac".
    pub fn label(&self) -> String {
        format!("Runs on {}", self.machine)
    }
}

/// A job in the scheduler: `space/job` for a recipe, `built-in:<name>` for a built-in job.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobKey(pub String);
impl JobKey {
    const BUILT_IN: &str = "built-in:";
    pub fn recipe(space: &SpaceId, job: &str) -> Self {
        Self(format!("{space}/{job}"))
    }
    pub fn built_in(name: &str) -> Self {
        Self(format!("{}{name}", Self::BUILT_IN))
    }
    pub fn is_built_in(&self) -> bool {
        self.0.starts_with(Self::BUILT_IN)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A run's record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRun {
    pub id: String,
    pub job: JobKey,
    /// The scheduled time it ran for; `None` for Run now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<i64>,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    pub outcome: RunOutcome,
    /// Why it did not start, failed or stopped, in a sentence for the job's page.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ModelProfile>,
    /// How its model was chosen.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model_reason: String,
    /// How many scheduled times this one run stands for, when it ran late; 0 on time.
    #[serde(default)]
    pub catch_up: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<RunOutput>,
    /// Its thread, for a recipe's run that started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Set at each 30 minutes it has run: the note on the assistant's sidebar row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_minutes: Option<i64>,
    /// The user opened the job's page since it ended.
    #[serde(default)]
    pub seen: bool,
    /// Its recipe's prompt and sources, fingerprinted when it started; sharing its output
    /// approves them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approves: Option<String>,
}
impl JobRun {
    /// A failed run, one that did not start, and one whose output waits for approval count on
    /// the assistant's row until seen.
    pub fn needs_attention(&self) -> bool {
        !self.seen
            && (matches!(self.outcome, RunOutcome::Failed | RunOutcome::NotStarted)
                || self.outputs.iter().any(|output| output.waiting))
    }
    /// "catch-up for 3 missed runs", or nothing on time.
    pub fn catch_up_label(&self) -> Option<String> {
        match self.catch_up {
            0 => None,
            1 => Some("catch-up for 1 missed run".into()),
            n => Some(format!("catch-up for {n} missed runs")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Running,
    /// Ended; with no output when it saved none.
    Finished,
    Failed,
    /// A source, the runner or the model kept it from starting.
    NotStarted,
    /// The user stopped it.
    Stopped,
}
impl RunOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Finished => "Finished",
            Self::Failed => "Failed",
            Self::NotStarted => "Did not start",
            Self::Stopped => "Stopped",
        }
    }
}

/// One source as the check before a run found it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCheck {
    /// The source as the recipe card lists it, such as "gh pr list, pr view".
    pub source: String,
    /// What is wrong and how to fix it; `None` when it is present and signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// An output a run saved, at its stable path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOutput {
    pub title: String,
    pub file: String,
    pub version: u32,
    /// Only on this Mac, or in the Space's storage.
    pub kept: String,
    /// Kept only on this Mac until the runner shares it: the job's first output in a Space
    /// stored on GitHub, or its first since its prompt or sources changed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waiting: bool,
}

/// A job as the job page shows it. Its sources as last checked are in its newest run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobState {
    pub recipe: Recipe,
    /// "Weekdays at 07:00 (America/New_York)".
    pub schedule: String,
    /// The next three runs in the recipe's timezone, none while it is paused.
    #[serde(default)]
    pub next_runs: Vec<String>,
    /// The model its next run would start on, or why it cannot start.
    pub model: String,
    /// That model; `None` when it cannot start. A run names its own model only when it differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ModelProfile>,
    /// Newest first, at most `MAX_RUNS_SHOWN`.
    #[serde(default)]
    pub runs: Vec<JobRun>,
    /// Every run that failed or did not start and is not seen yet, however old, and when the
    /// newest of them started.
    #[serde(default)]
    pub attention: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_at: Option<i64>,
    /// Why this Mac does not run it, such as another Mac being its runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_here: Option<String>,
    /// When its runner last saved an output: this Mac's newest run, or the newest version in
    /// the Space's storage of a job another Mac runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<i64>,
}
impl JobState {
    /// A "One runner" job another Mac runs, or none does, which this Mac may take over.
    pub fn can_take_over(&self) -> bool {
        self.recipe.sharing == Sharing::OneRunner && self.not_here.is_some()
    }
}
/// The runs a job's page shows.
pub const MAX_RUNS_SHOWN: usize = 20;

/// What the recipe card and the job page show about a recipe before it is saved or run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipePreview {
    /// "Weekdays at 07:00 (America/New_York)".
    pub schedule: String,
    /// The next three runs in the recipe's timezone, such as "Thu Oct 30 07:00".
    pub next_runs: Vec<String>,
    /// Each source and what the check found now.
    pub sources: Vec<SourceCheck>,
    /// "Job row · Sonnet · medium", or why it cannot run.
    pub model: String,
}

/// The sentence a source's line on the recipe card starts with: a CLI with its commands, or an
/// MCP server with its tools.
pub fn source_label(source: &Source) -> String {
    match source {
        Source::Cli {
            command, prefixes, ..
        } => {
            let commands: Vec<&str> = (prefixes.iter())
                .map(|p| p.strip_prefix(command.as_str()).unwrap_or(p).trim())
                .collect();
            format!("{command} {}", commands.join(", "))
        }
        Source::Mcp { server, tools, .. } => format!("{server} MCP · {}", tools.join(", ")),
    }
}

/// A recipe's name as a file name: lowercase words joined by hyphens.
pub fn job_id(name: &str) -> String {
    let words: Vec<String> = (name.split(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let id: String = words.join("-").chars().take(60).collect();
    match id.trim_end_matches('-') {
        "" => "job".into(),
        id => id.into(),
    }
}

/// Shell text a listed command may never hold, so it can only name one command.
const SHELL_TEXT: [char; 12] = [';', '&', '|', '<', '>', '$', '`', '(', ')', '\\', '\n', '"'];

/// Refuses a recipe the scheduler could not run as the user confirmed it. The timezone is
/// checked by the host, which knows the timezone names.
pub fn validate(recipe: &Recipe) -> Result<()> {
    let name = recipe.name.trim();
    ensure!(
        !name.is_empty() && name.chars().count() <= MAX_NAME_CHARS,
        "Give the job a name of at most {MAX_NAME_CHARS} characters"
    );
    ensure!(
        recipe.id == job_id(&recipe.id) && recipe.id.len() <= 60,
        "A job's id is lowercase words joined by hyphens"
    );
    ensure!(
        !recipe.prompt.trim().is_empty() && recipe.prompt.len() <= MAX_PROMPT_BYTES,
        "Give the job a prompt of at most 16 KB"
    );
    Cron::parse(&recipe.schedule.cron)?;
    for source in &recipe.sources {
        validate_source(source)?;
    }
    if let Some(output) = &recipe.output {
        ensure!(
            !output.title.trim().is_empty() && output.title.len() <= 200,
            "Give the output a title"
        );
        ensure!(
            !output.file_type.is_empty()
                && output.file_type.len() <= 16
                && output.file_type.chars().all(|c| c.is_ascii_alphanumeric()),
            "An output's file type is a short extension, such as html"
        );
    }
    Ok(())
}

/// The CLIs a job may read through; every other CLI is refused.
pub const JOB_CLIS: [&str; 4] = ["gh", "linctl", "sentry-cli", "aws"];
/// The exact read operations a job may run, by CLI, as the words after its name; an aws one
/// follows `--profile <name>`. Each was checked in its CLI's help (gh 2.x, linctl, sentry-cli
/// 3.8.2, aws-cli 2) to take no argument that writes a file, so `gh api`, `linctl issue get`
/// with its attachment download and `aws s3api get-object` with its outfile are not here.
const READS: [(&str, &[&str]); 4] = [
    (
        "gh",
        &[
            "pr list",
            "pr view",
            "pr diff",
            "pr checks",
            "issue list",
            "issue view",
            "run list",
            "run view",
            "repo view",
            "release list",
            "release view",
            "search prs",
            "search issues",
            "search repos",
            "search code",
            "search commits",
        ],
    ),
    (
        "linctl",
        &[
            "issue list",
            "issue search",
            "project list",
            "project get",
            "team list",
            "team get",
            "team members",
            "user me",
            "comment list",
            "label list",
        ],
    ),
    (
        "sentry-cli",
        &[
            "issues list",
            "events list",
            "releases list",
            "releases info",
            "projects list",
            "organizations list",
        ],
    ),
    (
        "aws",
        &[
            "logs describe-log-groups",
            "logs filter-log-events",
            "logs get-log-events",
            "logs start-query",
            "logs get-query-results",
            "ecs list-services",
            "ecs describe-services",
            "ecs list-tasks",
            "ecs describe-tasks",
            "cloudwatch get-metric-data",
            "cloudwatch get-metric-statistics",
            "cloudwatch describe-alarms",
        ],
    ),
];

fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && (name.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether the words after a CLI's name start one of its operations in `READS` (design,
/// decision 9); an aws command first names its profile.
fn reads(command: &str, words: &[&str]) -> bool {
    let words = match (command, words) {
        ("aws", ["--profile", profile, rest @ ..]) if plain(profile) => rest,
        ("aws", _) => return false,
        (_, words) => words,
    };
    (READS.iter())
        .find(|(cli, _)| *cli == command)
        .is_some_and(|(_, operations)| {
            (operations.iter()).any(|operation| {
                let operation: Vec<&str> = operation.split(' ').collect();
                words.starts_with(&operation)
            })
        })
}

/// A source's name is one plain word. A CLI is one of `JOB_CLIS`, each of its commands one
/// read from the allowlist with no shell text, and it names no sign-in check of its own,
/// since nothing a recipe supplies is run by the host. An MCP server lists the tools it may
/// call.
pub fn validate_source(source: &Source) -> Result<()> {
    match source {
        Source::Mcp { server, tools, .. } => {
            ensure!(
                plain(server),
                "Name the MCP server as its configuration does"
            );
            ensure!(
                !tools.is_empty() && tools.iter().all(|tool| plain(tool)),
                "List the tools the {server} MCP server may call"
            );
        }
        Source::Cli {
            command,
            prefixes,
            check,
            ..
        } => {
            ensure!(
                JOB_CLIS.contains(&command.as_str()),
                "A job reads only through {}, not {command}",
                JOB_CLIS.join(", ")
            );
            ensure!(
                check.is_none(),
                "A job's source cannot name its own sign-in check: Wiffletree checks {command} itself"
            );
            ensure!(!prefixes.is_empty(), "List the commands {command} may run");
            for prefix in prefixes {
                let words: Vec<&str> = prefix.split_whitespace().collect();
                ensure!(
                    !prefix.contains(SHELL_TEXT) && words.join(" ") == *prefix,
                    "{prefix:?} must be one command with single spaces and no shell text"
                );
                ensure!(
                    words.first() == Some(&command.as_str()) && reads(command, &words[1..]),
                    "{prefix:?} is not a read a job may run: a job may list only the read commands Wiffletree allows for {command}"
                );
            }
            if command == "aws" {
                let profile = |prefix: &String| prefix.split_whitespace().nth(2).map(str::to_owned);
                let first = profile(&prefixes[0]);
                ensure!(
                    prefixes.iter().all(|prefix| profile(prefix) == first),
                    "An aws source's commands name two profiles; use one source per profile"
                );
            }
        }
    }
    Ok(())
}

/// A parsed schedule: the minutes, hours, days, months and weekdays it runs on. Days and
/// weekdays both restricted run on either, as cron does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    /// Bit `d` for day `d`, 1 to 31.
    days: u32,
    /// Bit `m` for month `m`, 1 to 12.
    months: u16,
    /// Bit `w` for weekday `w`, Sunday 0 to Saturday 6.
    weekdays: u8,
    any_day: bool,
    any_weekday: bool,
}
impl Cron {
    pub fn parse(text: &str) -> Result<Self> {
        let fields: Vec<&str> = text.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            bail!("A schedule has five fields: minute, hour, day of month, month and weekday");
        };
        let (minutes, _) = field(minute, 0, 59, "minute")?;
        let (hours, _) = field(hour, 0, 23, "hour")?;
        let (days, any_day) = field(day, 1, 31, "day of month")?;
        let (months, _) = field(month, 1, 12, "month")?;
        let (weekdays, any_weekday) = field(weekday, 0, 7, "weekday")?;
        // Sunday is 0 or 7.
        let weekdays = (weekdays | (weekdays >> 7)) & 0x7f;
        Ok(Self {
            minutes,
            hours: hours as u32,
            days: days as u32,
            months: months as u16,
            weekdays: weekdays as u8,
            any_day,
            any_weekday,
        })
    }
    /// The hours and minutes it runs at on a day it runs, in order.
    pub fn times(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (0..24)
            .filter(|h| self.hours & (1 << h) != 0)
            .flat_map(|h| {
                (0..60)
                    .filter(|m| self.minutes & (1 << m) != 0)
                    .map(move |m| (h, m))
            })
    }
    /// Whether it runs on this day, `weekday` counted from Sunday as 0.
    pub fn runs_on(&self, day: u32, month: u32, weekday: u32) -> bool {
        let on_day = self.days & (1 << day) != 0;
        let on_weekday = self.weekdays & (1 << weekday) != 0;
        self.months & (1 << month) != 0
            && match (self.any_day, self.any_weekday) {
                (true, true) => true,
                (false, true) => on_day,
                (true, false) => on_weekday,
                (false, false) => on_day || on_weekday,
            }
    }
    /// The schedule in plain words, such as "Weekdays at 07:00" or "Every hour at :00";
    /// a schedule with no plainer form is quoted.
    pub fn words(&self, cron: &str) -> String {
        let minutes: Vec<u32> = (0..60).filter(|m| self.minutes & (1 << m) != 0).collect();
        let hours: Vec<u32> = (0..24).filter(|h| self.hours & (1 << h) != 0).collect();
        let time = match (&minutes[..], hours.len()) {
            ([minute], 24) => format!("every hour at :{minute:02}"),
            ([0, 30], 24) => "every 30 minutes".into(),
            ([0, 15, 30, 45], 24) => "every 15 minutes".into(),
            ([minute], 1..=4) => {
                let times: Vec<String> = (hours.iter())
                    .map(|h| format!("{h:02}:{minute:02}"))
                    .collect();
                format!("at {}", and_list(&times))
            }
            _ => return format!("On the schedule `{}`", cron.trim()),
        };
        let all_months = self.months == 0x1ffe;
        let days = match (self.any_day, self.any_weekday, self.weekdays) {
            (true, true, _) => "Every day".to_owned(),
            (true, false, 0b0111110) => "Weekdays".into(),
            (true, false, 0b1000001) => "Weekends".into(),
            (true, false, weekdays) => {
                let names: Vec<String> = (0..7)
                    .filter(|w| weekdays & (1 << w) != 0)
                    .map(|w| format!("{}s", WEEKDAYS[w]))
                    .collect();
                and_list(&names)
            }
            (false, true, _) => {
                let days: Vec<String> = (1..32)
                    .filter(|d| self.days & (1 << d) != 0)
                    .map(|d| d.to_string())
                    .collect();
                let each = if all_months { " of each month" } else { "" };
                format!("On day {}{each}", and_list(&days))
            }
            (false, false, _) => return format!("On the schedule `{}`", cron.trim()),
        };
        let months = match all_months {
            true => String::new(),
            false => {
                let names: Vec<String> = (1..13)
                    .filter(|m| self.months & (1 << m) != 0)
                    .map(|m| MONTHS[m - 1].to_owned())
                    .collect();
                format!(" in {}", and_list(&names))
            }
        };
        match (days.as_str(), time.strip_prefix("every")) {
            ("Every day", Some(rest)) => format!("Every{rest}{months}"),
            _ => format!("{days}{months} {time}"),
        }
    }
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// One cron field as a bit set from `min` to `max`, and whether it was `*`: a comma list of
/// `*`, `n`, `a-b`, each optionally `/step`.
fn field(text: &str, min: u32, max: u32, name: &str) -> Result<(u64, bool)> {
    let invalid = || anyhow::anyhow!("The {name} field {text:?} is not a schedule field");
    let number = |part: &str| -> Result<u32> {
        let n: u32 = part.parse().map_err(|_| invalid())?;
        ensure!(
            (min..=max).contains(&n),
            "The {name} must be from {min} to {max}"
        );
        Ok(n)
    };
    let mut bits = 0u64;
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step.parse().map_err(|_| invalid())?;
                ensure!(step > 0, invalid());
                (range, step)
            }
            None => (part, 1),
        };
        let (from, to) = match range {
            "*" => (min, max),
            range => match range.split_once('-') {
                Some((from, to)) => (number(from)?, number(to)?),
                None if step > 1 => (number(range)?, max),
                None => {
                    let n = number(range)?;
                    (n, n)
                }
            },
        };
        ensure!(from <= to, invalid());
        for n in (from..=to).step_by(step as usize) {
            bits |= 1 << n;
        }
    }
    Ok((bits, text == "*"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(cron: &str) -> String {
        Cron::parse(cron).unwrap().words(cron)
    }

    #[test]
    fn a_schedule_reads_in_plain_words() {
        assert_eq!(words("0 7 * * 1-5"), "Weekdays at 07:00");
        assert_eq!(words("0 * * * *"), "Every hour at :00");
        assert_eq!(words("0 8 * * 1"), "Mondays at 08:00");
        assert_eq!(words("0 18 * * 0"), "Sundays at 18:00");
        assert_eq!(words("0 18 * * 7"), "Sundays at 18:00");
        assert_eq!(
            words("30 9,17 * * 1,3,5"),
            "Mondays, Wednesdays and Fridays at 09:30 and 17:30"
        );
        assert_eq!(words("0 2 1 * *"), "On day 1 of each month at 02:00");
        assert_eq!(
            words("0 7 * 1,7 *"),
            "Every day in January and July at 07:00"
        );
        assert_eq!(words("*/30 * * * *"), "Every 30 minutes");
        assert_eq!(words("5,10 3 * * *"), "On the schedule `5,10 3 * * *`");
    }

    #[test]
    fn days_and_weekdays_both_restricted_run_on_either() {
        let cron = Cron::parse("0 7 1 * 1").unwrap();
        assert!(cron.runs_on(1, 3, 4));
        assert!(cron.runs_on(9, 3, 1));
        assert!(!cron.runs_on(9, 3, 2));
        let weekdays = Cron::parse("0 7 * * 1-5").unwrap();
        assert!(weekdays.runs_on(31, 10, 5) && !weekdays.runs_on(1, 11, 6));
        assert_eq!(weekdays.times().collect::<Vec<_>>(), [(7, 0)]);
    }

    #[test]
    fn a_field_out_of_range_or_shape_is_refused() {
        for cron in [
            "0 7 * *",
            "60 7 * * *",
            "0 24 * * *",
            "0 7 0 * *",
            "0 7 * 13 *",
            "0 7 * * 8",
            "a 7 * * *",
            "*/0 * * * *",
            "5-1 * * * *",
        ] {
            assert!(Cron::parse(cron).is_err(), "{cron}");
        }
    }

    fn gh(prefixes: &[&str]) -> Source {
        Source::Cli {
            command: "gh".into(),
            prefixes: prefixes.iter().map(|p| p.to_string()).collect(),
            check: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn a_cli_source_lists_single_commands_that_start_with_it() {
        validate_source(&gh(&["gh pr list", "gh pr view"])).unwrap();
        for prefix in [
            "gh",
            "git log",
            "gh pr list; gh pr comment",
            "gh pr list && rm",
            "gh  pr list",
            "gh pr list $(id)",
        ] {
            assert!(validate_source(&gh(&[prefix])).is_err(), "{prefix}");
        }
        assert_eq!(
            source_label(&gh(&["gh pr list", "gh pr view"])),
            "gh pr list, pr view"
        );
    }

    fn cli(command: &str, prefix: &str) -> Source {
        Source::Cli {
            command: command.into(),
            prefixes: vec![prefix.into()],
            check: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn a_job_may_list_only_the_allowed_reads_of_the_allowed_clis() {
        for (command, prefix) in [
            ("gh", "gh issue view"),
            ("gh", "gh search prs"),
            ("gh", "gh pr list --state open"),
            ("linctl", "linctl issue list"),
            ("sentry-cli", "sentry-cli issues list"),
            ("aws", "aws --profile readonly logs filter-log-events"),
            ("aws", "aws --profile readonly ecs describe-services"),
        ] {
            validate_source(&cli(command, prefix)).unwrap();
        }
        for (command, prefix) in [
            ("gh", "gh pr comment"),
            ("gh", "gh pr merge"),
            ("gh", "gh api"),
            ("gh", "gh pr"),
            ("gh", "gh search"),
            ("gh", "gh run download"),
            ("linctl", "linctl issue create"),
            ("linctl", "linctl issue get"),
            ("linctl", "linctl graphql"),
            ("sentry-cli", "sentry-cli releases new"),
            ("sentry-cli", "sentry-cli issues resolve"),
            ("aws", "aws s3 ls"),
            ("aws", "aws --profile readonly s3api get-object"),
            ("aws", "aws --profile readonly ec2 describe-instances"),
            ("aws", "aws --profile readonly logs delete-log-group"),
            ("aws", "aws logs filter-log-events"),
            ("aws", "aws --profile readonly s3api put-object"),
            (
                "aws",
                "aws --profile readonly secretsmanager get-secret-value",
            ),
            ("aws", "aws --profile readonly ssm get-parameter"),
            ("aws", "aws --profile readonly ecr get-login-password"),
            ("touch", "touch listed.txt"),
            ("git", "git log"),
        ] {
            let refused = validate_source(&cli(command, prefix)).unwrap_err();
            assert!(refused.to_string().contains(command), "{prefix}: {refused}");
        }
        let two_profiles = Source::Cli {
            command: "aws".into(),
            prefixes: vec![
                "aws --profile readonly logs filter-log-events".into(),
                "aws --profile admin ecs list-tasks".into(),
            ],
            check: None,
            extra: Default::default(),
        };
        let refused = validate_source(&two_profiles).unwrap_err();
        assert!(refused.to_string().contains("one source per profile"));
        let mut own = gh(&["gh pr list"]);
        if let Source::Cli { check, .. } = &mut own {
            *check = Some("gh pr list".into());
        }
        assert!(
            validate_source(&own).is_err(),
            "a job names no check of its own"
        );
    }

    #[test]
    fn a_name_becomes_a_job_id() {
        assert_eq!(job_id("Morning brief"), "morning-brief");
        assert_eq!(job_id("Release: “dashboard”"), "release-dashboard");
        assert_eq!(job_id("…"), "job");
        // Cut at 60 characters, it never ends in a hyphen, so it is a valid id.
        let long =
            job_id("Weekly summary of open pull requests in Sage and new Sentry issue counts");
        assert_eq!(
            long,
            "weekly-summary-of-open-pull-requests-in-sage-and-new-sentry"
        );
        assert_eq!(job_id(&long), long);
        assert_eq!(
            JobKey::recipe(&SpaceId("sage".into()), "brief").as_str(),
            "sage/brief"
        );
        assert!(JobKey::built_in("brain-compile").is_built_in());
    }
}
