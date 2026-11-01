//! Who runs each job on this machine, when a subscription counts as spent, and what a job
//! resolves to when an agent starts. Coordinators name jobs, never models; the human is never
//! limited by the table.
use crate::cascade::{self, Layer, Resolved, ResolvedRow, Setting};
use crate::{
    ModelProfile, Provider, QuotaReading, QuotaWindow, Role, VerificationSettings, model_name,
    same_name, smallest_model,
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    ops::RangeInclusive,
};

/// The order pickers and messages list providers in.
pub const PROVIDERS: [Provider; 2] = [Provider::Claude, Provider::Codex];
pub const MAX_GUIDE_BYTES: usize = 32 * 1024;
pub const MAX_REASON_CHARS: usize = 200;
/// Where a subscription counts as spent, in percent of any limit window, unless changed.
pub const DEFAULT_SWITCH_AT: u32 = 95;
pub const SWITCH_AT: RangeInclusive<u32> = 50..=100;

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}
impl fmt::Display for ModelProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} · {} · {}",
            self.provider.id(),
            self.model,
            self.effort
        )
    }
}
impl ModelProfile {
    /// "Opus · medium", as reasons name a choice.
    pub fn short(&self) -> String {
        format!(
            "{} · {}",
            model_name(self.provider, &self.model),
            self.effort
        )
    }
}

/// A job in the "Who runs what" table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Job {
    Coordinator,
    Implementer,
    Tester,
    /// A configured verifier, by its focus.
    Verifier {
        focus: String,
    },
    Keeper,
    /// The assistant's threads.
    Assistant,
    /// The router, which decides where each message to the assistant goes. It never waits.
    Router,
    /// Scheduled jobs' runs. The row is optional: without one, runs resolve the Assistant row.
    Job,
    /// Brain compile runs. The row is optional, with no small-work line: without one, no
    /// compile starts a model.
    Compiler,
}
impl Job {
    /// "Implementer", or a verifier's focus.
    pub fn name(&self) -> String {
        match self {
            Self::Coordinator => "Coordinator".into(),
            Self::Implementer => "Implementer".into(),
            Self::Tester => "Tester".into(),
            Self::Verifier { focus } => focus.clone(),
            Self::Keeper => "PR keeper".into(),
            Self::Assistant => "Assistant".into(),
            Self::Router => "Router".into(),
            Self::Job => "Job".into(),
            Self::Compiler => "Brain compiler".into(),
        }
    }
    /// Verifier focuses compare ignoring surrounding spaces and ASCII case, as levels do.
    pub fn is(&self, other: &Job) -> bool {
        match (self, other) {
            (Self::Verifier { focus: a }, Self::Verifier { focus: b }) => same_name(a, b),
            _ => self == other,
        }
    }
    /// The job an agent of `role` does: a configured verifier's when `focus` names one, and for
    /// any other reviewer the first configured reviewer's, else the tester's.
    pub fn for_role(
        role: Role,
        focus: Option<&str>,
        verification: &VerificationSettings,
    ) -> Option<Job> {
        let configured = focus.and_then(|focus| {
            (verification.verifiers.iter()).find(|v| v.role == role && same_name(&v.focus, focus))
        });
        if let Some(verifier) = configured {
            return Some(Job::Verifier {
                focus: verifier.focus.clone(),
            });
        }
        match role {
            Role::ProjectOrchestrator => Some(Job::Coordinator),
            Role::Implementer => Some(Job::Implementer),
            Role::Tester => Some(Job::Tester),
            Role::Reviewer => Some(first_reviewer(verification).unwrap_or(Job::Tester)),
            Role::Keeper => Some(Job::Keeper),
            Role::Assistant => Some(Job::Assistant),
            Role::JobRun => Some(Job::Job),
            Role::Compiler => Some(Job::Compiler),
            Role::TaskOrchestrator | Role::Maintenance => None,
        }
    }
}
fn first_reviewer(verification: &VerificationSettings) -> Option<Job> {
    let reviewer = (verification.verifiers.iter()).find(|v| v.role == Role::Reviewer)?;
    Some(Job::Verifier {
        focus: reviewer.focus.clone(),
    })
}

/// A first choice and what to do once its subscription is spent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub first: ModelProfile,
    #[serde(default, skip_serializing_if = "Then::is_wait")]
    pub then: Then,
}
impl Line {
    pub fn choices(&self) -> impl Iterator<Item = &ModelProfile> {
        std::iter::once(&self.first).chain(self.then.model())
    }
}
/// What a line does once its first choice is spent. Saved as the model, as `"skip"`, or left
/// out for wait.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedThen", into = "SavedThen")]
pub enum Then {
    /// Waits for the earliest reset.
    #[default]
    Wait,
    /// Moves on to a model on the other subscription.
    Model(ModelProfile),
    /// Leaves the review out; only verifier rows may skip.
    Skip,
}
impl Then {
    pub fn is_wait(&self) -> bool {
        *self == Self::Wait
    }
    pub fn model(&self) -> Option<&ModelProfile> {
        match self {
            Self::Model(profile) => Some(profile),
            Self::Wait | Self::Skip => None,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SavedThen {
    Wait(()),
    Model(ModelProfile),
    Word(ThenWord),
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThenWord {
    Skip,
}
impl From<SavedThen> for Then {
    fn from(saved: SavedThen) -> Self {
        match saved {
            SavedThen::Wait(()) => Self::Wait,
            SavedThen::Model(profile) => Self::Model(profile),
            SavedThen::Word(ThenWord::Skip) => Self::Skip,
        }
    }
}
impl From<Then> for SavedThen {
    fn from(then: Then) -> Self {
        match then {
            Then::Wait => Self::Wait(()),
            Then::Model(profile) => Self::Model(profile),
            Then::Skip => Self::Word(ThenWord::Skip),
        }
    }
}
/// One job's choices, with an optional lighter line for small work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRow {
    pub job: Job,
    pub main: Line,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub small: Option<Line>,
}

/// This machine's "Who runs what" table, with the shared switch threshold and guide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    /// Increases on every save; selections and turns record the revision they were made under.
    pub revision: u64,
    #[serde(default)]
    pub jobs: Vec<JobRow>,
    /// Optional notes for coordinators, shared with every machine. Never parsed.
    #[serde(default)]
    pub guide: String,
    /// The percentage of any limit window at which a subscription counts as spent. Shared.
    #[serde(default = "default_switch_at")]
    pub switch_at: u32,
    /// One-time notes the Models page shows until the human dismisses them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Verifiers without a saved row, which use a copy of the first reviewer's until a save.
    /// Filled in when the table is read; never saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copied: Vec<String>,
}
fn default_switch_at() -> u32 {
    DEFAULT_SWITCH_AT
}
impl Default for ModelSelection {
    fn default() -> Self {
        Self {
            revision: 0,
            jobs: vec![],
            guide: String::new(),
            switch_at: DEFAULT_SWITCH_AT,
            notes: vec![],
            copied: vec![],
        }
    }
}

impl ModelSelection {
    /// Whether the human has saved a setup; a store without one holds revision 0.
    pub fn is_saved(&self) -> bool {
        self.revision > 0
    }
    pub fn row(&self, job: &Job) -> Option<&JobRow> {
        self.jobs.iter().find(|row| row.job.is(job))
    }
    pub fn row_mut(&mut self, job: &Job) -> Option<&mut JobRow> {
        self.jobs.iter_mut().find(|row| row.job.is(job))
    }
    /// The PR keeper's row; none means no PR keeper model is chosen.
    pub fn keeper(&self) -> Option<&JobRow> {
        self.row(&Job::Keeper)
    }
    /// Sets the PR keeper's row's main line, or with `None` removes the row: no PR keeper model.
    pub fn set_keeper(&mut self, main: Option<Line>) {
        self.jobs.retain(|row| row.job != Job::Keeper);
        if let Some(main) = main {
            self.jobs.push(JobRow {
                job: Job::Keeper,
                main,
                small: None,
            });
        }
    }
    /// Sets the Job row's main line, or with `None` removes the row, so that job runs follow
    /// the Assistant row.
    pub fn set_job_row(&mut self, main: Option<Line>) {
        self.jobs.retain(|row| row.job != Job::Job);
        if let Some(main) = main {
            self.jobs.push(JobRow {
                job: Job::Job,
                main,
                small: None,
            });
        }
    }
    /// Sets the Brain compiler row's main line, or with `None` removes the row, so that no
    /// compile starts a model.
    pub fn set_compiler_row(&mut self, main: Option<Line>) {
        self.jobs.retain(|row| row.job != Job::Compiler);
        if let Some(main) = main {
            self.jobs.push(JobRow {
                job: Job::Compiler,
                main,
                small: None,
            });
        }
    }
    /// The only model comment sorting and skill merges run on: the PR keeper row's Claude
    /// choice, since those runs have every tool turned off, which only Claude can be started
    /// with.
    pub fn sorting_model(&self) -> Option<&ModelProfile> {
        (self.keeper()?.main.choices()).find(|p| p.provider == Provider::Claude)
    }
    /// Makes the rows follow the configured verifiers: a verifier no longer configured loses
    /// its row, and one without a row gets a copy, named in `copied`: a reviewer the first
    /// reviewer's row, and a tester, or a reviewer with no reviewer row to copy, the Tester
    /// row. A table saved before the Assistant row existed gets one copied from the
    /// Implementer row's small-work line, or its main line without one, and a Router row on
    /// Haiku at low effort when the table uses Claude, else on Codex's smallest model, both
    /// named in `copied`. Rows are put in the page's order.
    pub fn fit(&mut self, verification: &VerificationSettings) {
        self.copied.clear();
        if self.row(&Job::Assistant).is_none()
            && let Some(implementer) = self.row(&Job::Implementer)
        {
            let main = implementer
                .small
                .clone()
                .unwrap_or(implementer.main.clone());
            self.jobs.push(JobRow {
                job: Job::Assistant,
                main,
                small: None,
            });
            self.copied.push(Job::Assistant.name());
        }
        if self.row(&Job::Router).is_none() && self.row(&Job::Assistant).is_some() {
            // Codex cannot route, so a table with Codex only gets a row that always asks.
            let claude = (self.jobs.iter())
                .flat_map(|row| std::iter::once(&row.main).chain(&row.small))
                .flat_map(Line::choices)
                .any(|p| p.provider == Provider::Claude);
            let provider = match claude {
                true => Provider::Claude,
                false => Provider::Codex,
            };
            let first = ModelProfile {
                provider,
                model: smallest_model(provider).into(),
                effort: "low".into(),
            };
            self.jobs.push(JobRow {
                job: Job::Router,
                main: Line {
                    first,
                    then: Then::Wait,
                },
                small: None,
            });
            self.copied.push(Job::Router.name());
        }
        let tester = self.row(&Job::Tester).cloned();
        let reviewer = (verification.verifiers.iter())
            .filter(|v| v.role == Role::Reviewer)
            .find_map(|v| {
                self.row(&Job::Verifier {
                    focus: v.focus.clone(),
                })
            })
            .cloned()
            .or(tester.clone());
        let take = |jobs: &mut Vec<JobRow>, job: &Job| {
            let index = jobs.iter().position(|row| row.job.is(job))?;
            Some(jobs.remove(index))
        };
        let mut rest = std::mem::take(&mut self.jobs);
        let mut fitted = vec![];
        for job in [Job::Coordinator, Job::Implementer, Job::Tester] {
            fitted.extend(take(&mut rest, &job));
        }
        for verifier in &verification.verifiers {
            let job = Job::Verifier {
                focus: verifier.focus.clone(),
            };
            let source = match verifier.role {
                Role::Reviewer => &reviewer,
                _ => &tester,
            };
            match take(&mut rest, &job) {
                Some(row) => fitted.push(row),
                None => {
                    if let Some(source) = source {
                        self.copied.push(verifier.focus.clone());
                        fitted.push(JobRow {
                            job,
                            ..source.clone()
                        });
                    }
                }
            }
        }
        fitted.extend(take(&mut rest, &Job::Keeper));
        fitted.extend(take(&mut rest, &Job::Assistant));
        fitted.extend(take(&mut rest, &Job::Router));
        fitted.extend(take(&mut rest, &Job::Job));
        fitted.extend(take(&mut rest, &Job::Compiler));
        self.jobs = fitted;
    }
    pub fn validate(&self) -> Result<()> {
        for job in [
            Job::Coordinator,
            Job::Implementer,
            Job::Tester,
            Job::Assistant,
            Job::Router,
        ] {
            let rows = self.jobs.iter().filter(|row| row.job == job).count();
            ensure!(rows == 1, "Choose who runs the {} job", job.name());
        }
        for (job, two) in [
            (Job::Keeper, "The PR keeper has two rows"),
            (Job::Job, "Jobs have two rows"),
            (Job::Compiler, "The Brain compiler has two rows"),
        ] {
            let rows = self.jobs.iter().filter(|row| row.job == job).count();
            ensure!(rows <= 1, "{two}");
        }
        validate_rows(&self.jobs)?;
        ensure!(
            SWITCH_AT.contains(&self.switch_at),
            "Move on at {}–{}%",
            SWITCH_AT.start(),
            SWITCH_AT.end()
        );
        ensure!(
            self.guide.len() <= MAX_GUIDE_BYTES,
            "The notes must be at most 32 KB"
        );
        Ok(())
    }
    /// What `job` would start on now, or why it cannot start here; see [`resolve`]. `when`
    /// writes a reset time, such as "Mon 09:00".
    pub fn resolve(
        &self,
        job: &Job,
        small: bool,
        may_skip: bool,
        limits: &Limits,
        when: &dyn Fn(i64) -> String,
    ) -> Result<Pick> {
        // Without a Job row, jobs run as the assistant's threads do.
        let row = match job {
            Job::Job => self.row(job).or_else(|| self.row(&Job::Assistant)),
            _ => self.row(job),
        };
        let Some(row) = row else {
            bail!(
                "{} has no row in Models → Who runs what. Add one there first.",
                job.name()
            );
        };
        resolve(row, small, may_skip, limits, when)
    }
}

/// The rules every row keeps, in this Mac's table and in a Space's overrides: one row a
/// verifier, valid models and efforts, a then on the other subscription, only reviews skipping
/// and no small-work line for the Brain compiler.
fn validate_rows(rows: &[JobRow]) -> Result<()> {
    ensure!(
        (rows.iter()).all(|row| row.job != Job::Compiler || row.small.is_none()),
        "The Brain compiler has no small-work line"
    );
    let mut focuses = BTreeSet::new();
    for row in rows {
        if let Job::Verifier { focus } = &row.job {
            ensure!(
                focuses.insert(focus.trim().to_lowercase()),
                "Two rows are for the verifier \"{}\"",
                focus.trim()
            );
        }
        for line in std::iter::once(&row.main).chain(&row.small) {
            for profile in line.choices() {
                ensure!(
                    !profile.model.trim().is_empty() && profile.model.len() <= 128,
                    "Invalid model ID"
                );
                ensure!(
                    !profile.effort.trim().is_empty() && profile.effort.len() <= 32,
                    "Invalid effort"
                );
            }
            match &line.then {
                Then::Model(then) => ensure!(
                    then.provider != line.first.provider,
                    "{}: both choices use the {} subscription, so they would be spent together. Choose a {} model or wait.",
                    row.job.name(),
                    then.provider.label(),
                    other(then.provider).label()
                ),
                Then::Skip => ensure!(
                    matches!(row.job, Job::Verifier { .. }),
                    "{}: only reviews can be skipped. Choose a {} model or wait.",
                    row.job.name(),
                    other(line.first.provider).label()
                ),
                Then::Wait => {}
            }
        }
    }
    Ok(())
}

/// A Space's overrides: at most one row a job, each kept to the table's rules.
pub fn validate_overrides(rows: &[JobRow]) -> Result<()> {
    for (at, row) in rows.iter().enumerate() {
        ensure!(
            !rows[..at].iter().any(|before| before.job.is(&row.job)),
            "Two rows are for the {} job",
            row.job.name()
        );
    }
    validate_rows(rows)
}

/// Who runs what as one Space resolves it on this Mac: this Mac's table with each of the Space's
/// rows on top that this Mac can run. A Space row none of whose choices on the line in use is
/// signed in here falls back to this Mac's row.
#[derive(Clone, Debug)]
pub struct SpaceTable {
    /// The table agents in the Space resolve, overrides in place.
    pub selection: ModelSelection,
    /// Each row's provenance, in the table's order.
    pub rows: Vec<ResolvedRow>,
    /// The table small work resolves: a Space row stays where its small-work line, or its main
    /// line without one, can run here.
    small: ModelSelection,
    /// The Space's name; `None` outside a Space, where every row is this Mac's.
    space: Option<String>,
    overrides: Vec<JobRow>,
    unusable: BTreeSet<Provider>,
    /// Why a provider this Mac has signed in cannot run here, for the reason a Space row gives.
    held: BTreeMap<Provider, String>,
}
impl ModelSelection {
    /// This table as the Space `space` resolves it with its `overrides`, when `unusable` are the
    /// providers this Mac cannot run. A Space's verifier row counts only for a configured
    /// verifier, which already has a row here.
    pub fn for_space(
        &self,
        space: Option<&str>,
        overrides: &[JobRow],
        unusable: &BTreeSet<Provider>,
    ) -> SpaceTable {
        let (selection, rows) = self.merge(overrides, unusable, false);
        let (small, _) = self.merge(overrides, unusable, true);
        SpaceTable {
            selection,
            rows,
            small,
            space: space.map(Into::into),
            overrides: overrides.to_vec(),
            unusable: unusable.clone(),
            held: BTreeMap::new(),
        }
    }
    /// This table with the `overrides` this Mac can run on top, for small work when `small`,
    /// with each row's provenance.
    fn merge(
        &self,
        overrides: &[JobRow],
        unusable: &BTreeSet<Provider>,
        small: bool,
    ) -> (ModelSelection, Vec<ResolvedRow>) {
        let mut jobs: Vec<Job> = self.jobs.iter().map(|row| row.job.clone()).collect();
        for row in overrides {
            let configured = !matches!(row.job, Job::Verifier { .. });
            if configured && !jobs.iter().any(|job| job.is(&row.job)) {
                jobs.push(row.job.clone());
            }
        }
        let mut selection = ModelSelection {
            jobs: vec![],
            ..self.clone()
        };
        let mut rows = vec![];
        for job in jobs {
            let setting = RowSetting {
                job: &job,
                small,
                unusable,
            };
            let mut set = vec![];
            set.extend(
                self.row(&job)
                    .map(|row| (Layer::ThisMac, Some(row.clone()))),
            );
            let over = overrides.iter().find(|row| row.job.is(&job));
            set.extend(over.map(|row| {
                (
                    Layer::Space,
                    Some(JobRow {
                        job: job.clone(),
                        ..row.clone()
                    }),
                )
            }));
            let resolved = cascade::resolve(&setting, &set);
            let Some(row) = resolved.value else { continue };
            selection.jobs.push(row.clone());
            rows.push(ResolvedRow {
                job,
                resolved: Resolved {
                    value: row,
                    from: resolved.from,
                    // The table has no stock rows.
                    overrode: (resolved.overrode.into_iter())
                        .filter(|layer| *layer != Layer::Stock)
                        .collect(),
                    notes: resolved.notes,
                },
            });
        }
        (selection, rows)
    }
}
impl SpaceTable {
    /// This table, saying `why` when a Space row falls back because `provider` cannot run,
    /// such as for a job whose CLI source Codex cannot yet hold.
    pub fn because(mut self, provider: Provider, why: String) -> Self {
        self.held.insert(provider, why);
        self
    }
    /// What `job` would start on now, as [`ModelSelection::resolve`] says, with the reason
    /// naming the Space when its row was used or fell back.
    pub fn resolve(
        &self,
        job: &Job,
        small: bool,
        may_skip: bool,
        limits: &Limits,
        when: &dyn Fn(i64) -> String,
    ) -> Result<Pick> {
        let selection = if small { &self.small } else { &self.selection };
        let mut pick = selection.resolve(job, small, may_skip, limits, when)?;
        pick.reason = self.explain(selection, job, small, pick.reason);
        Ok(pick)
    }
    /// `reason`, for a pick of `job` from `selection`, prefixed with the Space's part in it.
    fn explain(
        &self,
        selection: &ModelSelection,
        job: &Job,
        small: bool,
        reason: String,
    ) -> String {
        let Some(space) = &self.space else {
            return reason;
        };
        // Without a Job row, jobs run as the assistant's threads do.
        let used = match selection.row(job) {
            None if *job == Job::Job => Job::Assistant,
            _ => job.clone(),
        };
        let looked_at = std::iter::once(job).chain((used != *job).then_some(&used));
        let mut said: Vec<String> = looked_at
            .filter_map(|job| {
                let row = self.overrides.iter().find(|row| row.job.is(job))?;
                let why = refused(row, small, &self.unusable, &self.held)?;
                Some(format!(
                    "{space}'s {} row falls back to this Mac's: {why}.",
                    job.name()
                ))
            })
            .collect();
        let over = self.overrides.iter().find(|row| row.job.is(&used));
        let from_space = over
            .zip(selection.row(&used))
            .is_some_and(|(over, used)| over.main == used.main && over.small == used.small);
        said.push(match from_space {
            true => format!("{space}'s {} row: {reason}", used.name()),
            false => reason,
        });
        said.join(" ")
    }
}

/// A row of the table as the resolver sees it: this Mac's as the base, and a Space's on top
/// unless this Mac can run none of its choices.
struct RowSetting<'a> {
    job: &'a Job,
    small: bool,
    unusable: &'a BTreeSet<Provider>,
}
impl Setting for RowSetting<'_> {
    type Value = Option<JobRow>;
    fn stock(&self) -> Option<JobRow> {
        None
    }
    fn layers(&self) -> &[Layer] {
        &[Layer::ThisMac, Layer::Space]
    }
    fn refusal(&self, layer: Layer, _: &Option<JobRow>, value: &Option<JobRow>) -> Option<String> {
        let row = value.as_ref().filter(|_| layer == Layer::Space)?;
        debug_assert!(row.job.is(self.job));
        refused(row, self.small, self.unusable, &BTreeMap::new())
    }
}

/// Why a Space's `row` gives way to this Mac's: it breaks the table's rules, or this Mac can run
/// none of the choices on its line in use.
fn refused(
    row: &JobRow,
    small: bool,
    unusable: &BTreeSet<Provider>,
    held: &BTreeMap<Provider, String>,
) -> Option<String> {
    match validate_rows(std::slice::from_ref(row)) {
        Err(error) => Some(format!("it breaks the table's rules: {error:#}")),
        Ok(()) => not_runnable(row, small, unusable, held),
    }
}

/// Why this Mac can run none of the choices on `row`'s line in use, its small-work line for
/// `small` work when it has one, else its main line, such as "Claude is not signed in on this
/// Mac", or what `held` says of a provider; the router never runs on Codex.
fn not_runnable(
    row: &JobRow,
    small: bool,
    unusable: &BTreeSet<Provider>,
    held: &BTreeMap<Provider, String>,
) -> Option<String> {
    let line = (row.small.as_ref()).filter(|_| small).unwrap_or(&row.main);
    let runs = |p: &ModelProfile| {
        !unusable.contains(&p.provider)
            && !(row.job == Job::Router && p.provider == Provider::Codex)
    };
    if line.choices().any(runs) {
        return None;
    }
    let mut providers: Vec<Provider> = line.choices().map(|p| p.provider).collect();
    providers.dedup();
    if let Some(why) = providers.iter().find_map(|p| held.get(p)) {
        return Some(why.clone());
    }
    let names: Vec<&str> = providers.iter().map(|p| p.label()).collect();
    let are = if names.len() > 1 { "are" } else { "is" };
    Some(format!(
        "{} {are} not signed in on this Mac",
        names.join(" and ")
    ))
}

fn other(provider: Provider) -> Provider {
    match provider {
        Provider::Claude => Provider::Codex,
        Provider::Codex => Provider::Claude,
    }
}

/// What a resolution looks at: the latest quota readings, the providers this machine cannot
/// run, the switch threshold and the time.
pub struct Limits<'a> {
    pub quotas: &'a [QuotaReading],
    /// Providers whose CLI detection found missing or signed out.
    pub unusable: &'a BTreeSet<Provider>,
    pub switch_at: u32,
    pub now: i64,
}
/// Why a provider counts as spent: its most used counting window, and when every counting
/// window has reset, unknown when one of them has no reset time.
#[derive(Clone, Debug, PartialEq)]
pub struct Spent {
    pub provider: Provider,
    pub window: String,
    /// `None` when the provider reported the limit reached without a percentage.
    pub used: Option<f64>,
    pub until: Option<i64>,
}
impl Limits<'_> {
    /// A window counts while its reset is unknown or still ahead and it is at or over the
    /// threshold, or reports the limit reached. No reading, or no window, is not spent.
    pub fn spent(&self, provider: Provider) -> Option<Spent> {
        let reading = self.quotas.iter().find(|q| q.provider == provider)?;
        let counting: Vec<&QuotaWindow> = (reading.windows.iter())
            .filter(|w| w.resets_at.is_none_or(|at| at > self.now))
            .filter(|w| {
                w.used_percent
                    .is_some_and(|used| used >= f64::from(self.switch_at))
                    || w.status.as_deref() == Some("rejected")
            })
            .collect();
        let worst = counting.iter().max_by(|a, b| {
            let used = |w: &QuotaWindow| w.used_percent.unwrap_or(100.);
            used(a).total_cmp(&used(b))
        })?;
        let until = (counting.iter())
            .map(|w| w.resets_at)
            .try_fold(i64::MIN, |latest, at| Some(latest.max(at?)));
        Some(Spent {
            provider,
            window: worst.label.to_lowercase(),
            used: worst.used_percent,
            until,
        })
    }
    fn usable(&self, provider: Provider) -> bool {
        !self.unusable.contains(&provider)
    }
}
impl Spent {
    /// Why a turn pinned to this provider waits, as its chat and its parent read it.
    pub fn waiting(&self, switch_at: u32, when: &dyn Fn(i64) -> String) -> String {
        format!(
            "Waits for {}: {} (moves on at {switch_at}%). {}",
            self.provider.label(),
            self.said("is", true),
            starts(self.until, switch_at, when)
        )
    }
    /// "Claude's weekly limit was at 96%", or "… was reached"; "its …" for `own`.
    fn said(&self, tense: &str, own: bool) -> String {
        let whose = match own {
            true => "its".to_owned(),
            false => format!("{}'s", self.provider.label()),
        };
        match self.used {
            Some(used) => format!(
                "{whose} {} limit {tense} at {:.0}%",
                self.window,
                used.floor()
            ),
            None => format!("{whose} {} limit {tense} reached", self.window),
        }
    }
}

/// What a job starts on and why. A pick that `waits` is pinned and held until its provider is
/// no longer spent; one with a `skip` starts nothing, and its review goes ahead without it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub profile: ModelProfile,
    pub waits: bool,
    /// Why the review skips this verifier, as its record says it afterwards, such as "Claude's
    /// weekly limit was at 96%; its row says skip".
    pub skip: Option<String>,
    /// When the wait should end, if known.
    pub until: Option<i64>,
    pub reason: String,
}

/// Why a Router row of Codex choices only never runs.
pub const CODEX_ROUTER: &str = "The router cannot run on Codex: Codex cannot be started with every tool off, so it asks you where each message goes.";

/// First unless it is spent, then `then` unless that is spent; choices this machine cannot run,
/// and for the router Codex, are left out. When every remaining choice is spent, the one that resets first is pinned and
/// waits. A line whose then is skip skips once its first choice is spent or not signed in, when
/// `may_skip`; otherwise it waits.
pub fn resolve(
    row: &JobRow,
    small: bool,
    may_skip: bool,
    limits: &Limits,
    when: &dyn Fn(i64) -> String,
) -> Result<Pick> {
    let (line, small) = match (&row.small, small) {
        (Some(line), true) => (line, true),
        _ => (&row.main, false),
    };
    // The router runs with every tool off, which Codex cannot be started with.
    let router_unusable;
    let router_limits;
    let limits = match row.job {
        Job::Router => {
            ensure!(
                line.choices().any(|p| p.provider != Provider::Codex),
                "{CODEX_ROUTER}"
            );
            let mut without_codex = limits.unusable.clone();
            without_codex.insert(Provider::Codex);
            router_unusable = without_codex;
            router_limits = Limits {
                unusable: &router_unusable,
                ..*limits
            };
            &router_limits
        }
        _ => limits,
    };
    if may_skip && line.then == Then::Skip {
        let provider = line.first.provider;
        let skipped = match limits.usable(provider) {
            false => {
                let why = format!("{} is not signed in on this Mac", provider.label());
                Some((why.clone(), why, None))
            }
            true => (limits.spent(provider)).map(|spent| {
                let now = format!(
                    "{} (moves on at {}%)",
                    spent.said("is", false),
                    limits.switch_at
                );
                (now, spent.said("was", false), spent.until)
            }),
        };
        if let Some((now, then, until)) = skipped {
            return Ok(Pick {
                profile: line.first.clone(),
                waits: false,
                skip: Some(format!("{then}; its row says skip")),
                until,
                reason: now,
            });
        }
    }
    let for_small = if small { " for small work" } else { "" };
    let choices: Vec<&ModelProfile> = line
        .choices()
        .filter(|p| limits.usable(p.provider))
        .collect();
    if choices.is_empty() {
        let names: Vec<&str> = line.choices().map(|p| p.provider.label()).collect();
        let are = if names.len() > 1 { "are" } else { "is" };
        bail!(
            "{} cannot start on this Mac: {} {are} not signed in. Sign in, or change its row in Models → Who runs what.",
            row.job.name(),
            names.join(" and ")
        );
    }
    let spent: Vec<Option<Spent>> = choices.iter().map(|p| limits.spent(p.provider)).collect();
    let moves_on = format!("(moves on at {}%)", limits.switch_at);
    if let Some(index) = spent.iter().position(Option::is_none) {
        let profile = choices[index].clone();
        let reason = if profile == line.first {
            format!("First choice{for_small}")
        } else {
            // Only a usable first choice is among the choices, and then it is the first.
            let why = match spent.first().filter(|_| limits.usable(line.first.provider)) {
                Some(Some(spent)) => format!("{} {moves_on}", spent.said("was", false)),
                _ => format!(
                    "{} is not signed in on this Mac",
                    line.first.provider.label()
                ),
            };
            format!(
                "Started on {}{for_small}: {why}. First choice was {}.",
                profile.provider.label(),
                line.first.short()
            )
        };
        return Ok(Pick {
            profile,
            waits: false,
            skip: None,
            until: None,
            reason,
        });
    }
    let spent: Vec<Spent> = spent.into_iter().flatten().collect();
    // An unknown reset waits for a fresh reading, so it comes last.
    let index = (0..spent.len())
        .min_by_key(|&i| spent[i].until.unwrap_or(i64::MAX))
        .unwrap_or(0);
    let own = spent.len() == 1;
    let said: Vec<String> = spent.iter().map(|s| s.said("is", own)).collect();
    let others = if spent.len() == 1 {
        ", and no other choice can run".to_owned()
    } else {
        String::new()
    };
    let starts = starts(spent[index].until, limits.switch_at, when);
    Ok(Pick {
        profile: choices[index].clone(),
        waits: true,
        skip: None,
        until: spent[index].until,
        reason: format!(
            "Waits for {}{for_small}: {} {moves_on}{others}. {starts}",
            choices[index].provider.label(),
            said.join(" and ")
        ),
    })
}

/// When a waiting turn starts: after a known reset, else once a reading is below the threshold.
fn starts(until: Option<i64>, switch_at: u32, when: &dyn Fn(i64) -> String) -> String {
    match until {
        Some(at) => format!("Starts after the reset, {}.", when(at)),
        None => format!("Starts once a reading comes in below {switch_at}%."),
    }
}

/// How a session's model was chosen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub chosen_by: Chooser,
    pub reason: String,
    pub revision: u64,
    pub at: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Chooser {
    /// Wiffletree, from the Who runs what table.
    Wiffletree,
    /// A coordinator, before the table: kept so older agents still show who chose.
    Coordinator {
        session_id: String,
    },
    Human,
    Default,
}
/// A coordinator's reason for a decision: one non-empty line of at most 200 characters, with no
/// control characters such as a carriage return.
pub fn one_line_reason(reason: &str) -> Result<&str> {
    let reason = reason.trim();
    ensure!(!reason.is_empty(), "Give a one-line reason");
    ensure!(
        !reason.chars().any(char::is_control) && reason.chars().count() <= MAX_REASON_CHARS,
        "The reason must be one line of at most {MAX_REASON_CHARS} characters"
    );
    Ok(reason)
}
