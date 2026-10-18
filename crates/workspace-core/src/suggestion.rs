//! The setup suggested on first run, from the provider CLIs signed in on this machine, and the
//! rows of the "Who runs what" table it and the upgrade start from.
use crate::{
    DEFAULT_SWITCH_AT, Job, JobRow, LevelKind, Line, ModelOption, ModelProfile, ModelSelection,
    PROVIDERS, Provider, ReviewLevel, Role, VerificationSettings, VerifierConfig, usual_efforts,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What detection found for one provider's CLI.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub installed: bool,
    pub signed_in: bool,
    /// The subscription plan, when the CLI reports one. Shown, never acted on.
    pub plan: Option<String>,
}
impl ProviderStatus {
    pub fn usable(&self) -> bool {
        self.installed && self.signed_in
    }
}
pub type Detection = BTreeMap<Provider, ProviderStatus>;

/// Everything the Models page saves: the table and who verifies tickets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub selection: ModelSelection,
    pub verification: VerificationSettings,
}

/// The starter notes: when the coordinator should pass `small: true`.
pub const STARTER_GUIDE: &str = "Pass small: true for small, contained work, such as a typo, a copy or config tweak or a fix in one file. Leave it off for migrations, data or security changes and anything that crosses repositories.\n";

/// Every earlier starter text, which is replaced by `STARTER_GUIDE` when it was never edited:
/// they told coordinators to pick models, which Wiffletree now does.
pub fn retired_starter_guides() -> Vec<String> {
    let claude = "Claude: small, contained change → Sonnet; large or cross-cutting → Opus.";
    let codex = "Codex: small → GPT 6.1 Sol at medium; large → GPT 6.1 Sol at high.";
    [
        format!("{claude} {codex}"),
        claude.to_owned(),
        codex.to_owned(),
    ]
    .map(|lines| {
        format!(
            "Pick the model by the size and risk of the work. {lines} Raise effort for migrations, data or security changes.\n"
        )
    })
    .into()
}

/// The stock "Use it" line of the re-check level "Light".
pub const LIGHT_USE_WHEN: &str = "after a rebase, a resolved merge conflict or a small fix, on a ticket whose last review passed.";

/// "Light", a re-check that runs the first reviewer among `verifiers`, when there is one.
pub fn light_level(verifiers: &[VerifierConfig]) -> Option<ReviewLevel> {
    let reviewer = verifiers.iter().find(|v| v.role == Role::Reviewer)?;
    Some(ReviewLevel {
        name: "Light".into(),
        kind: LevelKind::Recheck,
        verifiers: vec![reviewer.focus.clone()],
        use_when: LIGHT_USE_WHEN.into(),
    })
}

/// "Full", a full review with every suggested verifier, and "Light".
fn suggested_levels(verifiers: &[VerifierConfig]) -> Vec<ReviewLevel> {
    let full = ReviewLevel {
        name: "Full".into(),
        kind: LevelKind::FullReview,
        verifiers: verifiers.iter().map(|v| v.focus.clone()).collect(),
        use_when: "for a ticket's first review, and after any change to what the code does.".into(),
    };
    std::iter::once(full)
        .chain(light_level(verifiers))
        .collect()
}

/// The kind of row a job starts with, as drawn in the approved mockup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seed {
    Coordinator,
    Implementer,
    Tester,
    /// A reviewer that is one provider's second opinion: it waits rather than switching.
    Lens(Provider),
    /// Any other reviewer, such as a style pass.
    Reviewer,
    Keeper,
}
/// Each provider's model and effort for a line, and which provider comes first.
struct Pair {
    claude: (&'static str, &'static str),
    codex: (&'static str, &'static str),
    first: Provider,
}
const OPUS_HIGH: (&str, &str) = ("opus", "high");
const SOL_HIGH: (&str, &str) = ("gpt-6.1-sol", "high");
const SOL_MEDIUM: (&str, &str) = ("gpt-6.1-sol", "medium");
impl Seed {
    fn lines(self) -> (Pair, Option<Pair>) {
        let pair = |claude, codex, first| Pair {
            claude,
            codex,
            first,
        };
        let claude_first = Provider::Claude;
        match self {
            Self::Coordinator => (pair(OPUS_HIGH, SOL_HIGH, claude_first), None),
            Self::Implementer => (
                pair(("opus", "medium"), SOL_HIGH, claude_first),
                Some(pair(("sonnet", "high"), SOL_MEDIUM, claude_first)),
            ),
            Self::Tester => (pair(("sonnet", "high"), SOL_MEDIUM, claude_first), None),
            Self::Lens(provider) => (pair(OPUS_HIGH, SOL_HIGH, provider), None),
            Self::Reviewer => (
                pair(("sonnet", "medium"), SOL_MEDIUM, Provider::Codex),
                None,
            ),
            Self::Keeper => (pair(("sonnet", "medium"), SOL_MEDIUM, claude_first), None),
        }
    }
    /// The seed of a configured verifier: a tester is seeded as the Tester job, and a reviewer
    /// with a provider's lens waits for that provider.
    pub fn of_verifier(verifier: &VerifierConfig, lens: Option<Provider>) -> Self {
        match (verifier.role, lens) {
            (Role::Tester, _) => Self::Tester,
            (_, Some(provider)) => Self::Lens(provider),
            _ => Self::Reviewer,
        }
    }
}

/// `job`'s seeded row on the `usable` providers, with each model replaced by the provider's
/// closest one in `available` when that lists the provider; `None` with no usable provider.
pub fn seed_row(
    job: Job,
    seed: Seed,
    usable: &[Provider],
    available: &[ModelOption],
) -> Option<JobRow> {
    let waits = matches!(seed, Seed::Lens(_));
    let line = |pair: Pair| {
        let order = match pair.first {
            Provider::Claude => [Provider::Claude, Provider::Codex],
            Provider::Codex => [Provider::Codex, Provider::Claude],
        };
        let mut choices = order
            .into_iter()
            .filter(|p| usable.contains(p))
            .map(|provider| {
                let (model, effort) = match provider {
                    Provider::Claude => pair.claude,
                    Provider::Codex => pair.codex,
                };
                closest(provider, model, effort, available)
            });
        let first = choices.next()?;
        let then = choices.next().filter(|_| !waits);
        Some(Line { first, then })
    };
    let (main, small) = seed.lines();
    Some(JobRow {
        job,
        main: line(main)?,
        small: small.and_then(line),
    })
}

/// The provider's model closest to `model` among `available`: the same model, else the first
/// whose ID contains it or is contained in it, else the provider's first; at `effort`, else the
/// nearest effort it offers. Unchanged when `available` lists nothing for the provider.
pub fn closest(
    provider: Provider,
    model: &str,
    effort: &str,
    available: &[ModelOption],
) -> ModelProfile {
    let options: Vec<&ModelOption> = available
        .iter()
        .filter(|m| m.provider == provider)
        .collect();
    let option = (options.iter())
        .find(|m| m.model == model)
        .or_else(|| options.iter().find(|m| m.model.contains(model)))
        .or_else(|| options.iter().find(|m| model.contains(m.model.as_str())))
        .or(options.first());
    let Some(option) = option else {
        return ModelProfile {
            provider,
            model: model.into(),
            effort: effort.into(),
        };
    };
    let usual: Vec<String> = usual_efforts(provider)
        .iter()
        .map(|e| (*e).into())
        .collect();
    let efforts = if option.efforts.is_empty() {
        &usual
    } else {
        &option.efforts
    };
    let rank = |e: &str| usual.iter().position(|u| u == e).unwrap_or(usual.len()) as i64;
    let effort = (efforts.iter())
        .min_by_key(|e| (rank(e) - rank(effort)).abs())
        .cloned()
        .unwrap_or_else(|| effort.into());
    ModelProfile {
        provider,
        model: option.model.clone(),
        effort,
    }
}

/// The seeded rows for every job, the verifiers' with their lens.
pub fn seed_jobs(
    verifiers: &[(VerifierConfig, Option<Provider>)],
    keeper: bool,
    usable: &[Provider],
    available: &[ModelOption],
) -> Vec<JobRow> {
    let fixed = [
        (Job::Coordinator, Seed::Coordinator),
        (Job::Implementer, Seed::Implementer),
        (Job::Tester, Seed::Tester),
    ];
    let verifiers = verifiers.iter().map(|(verifier, lens)| {
        let job = Job::Verifier {
            focus: verifier.focus.clone(),
        };
        (job, Seed::of_verifier(verifier, *lens))
    });
    let keeper = keeper.then_some((Job::Keeper, Seed::Keeper));
    (fixed.into_iter().chain(verifiers).chain(keeper))
        .filter_map(|(job, seed)| seed_row(job, seed, usable, available))
        .collect()
}

/// A setup that uses only the providers found installed and signed in, with the mockup's rows
/// and each model replaced by the closest one `catalog` lists. With neither provider, the table
/// is empty and the setup cannot be saved.
pub fn suggest(detection: &Detection, catalog: &[ModelOption]) -> Setup {
    let usable: Vec<Provider> = PROVIDERS
        .into_iter()
        .filter(|p| detection.get(p).is_some_and(ProviderStatus::usable))
        .collect();
    let mut verification = VerificationSettings::default();
    // The default reviewers are Claude's and Codex's second opinions, in that order.
    let lenses = [None, Some(Provider::Claude), Some(Provider::Codex)];
    let verifiers: Vec<(VerifierConfig, Option<Provider>)> = (verification.verifiers.iter())
        .cloned()
        .zip(lenses)
        .filter(|(_, lens)| lens.is_none_or(|p| usable.contains(&p)))
        .collect();
    verification.verifiers = verifiers.iter().map(|(v, _)| v.clone()).collect();
    verification.levels = suggested_levels(&verification.verifiers);
    let jobs = if usable.is_empty() {
        vec![]
    } else {
        seed_jobs(&verifiers, true, &usable, catalog)
    };
    Setup {
        selection: ModelSelection {
            revision: 0,
            jobs,
            guide: if usable.is_empty() {
                String::new()
            } else {
                STARTER_GUIDE.to_owned()
            },
            switch_at: DEFAULT_SWITCH_AT,
            notes: vec![],
            copied: vec![],
        },
        verification,
    }
}
