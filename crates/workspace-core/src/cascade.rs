//! The settings cascade (OpenSpec change `settings-cascade`): a setting resolves Stock →
//! Workspace → Space → Repository for one project, and the resolver says which layer each value
//! came from and which layers it overrode. Behaviors are the first settings it resolves: named,
//! typed choices that each render one sentence into a role's skill.
use crate::{Job, JobRow, LevelKind, ReviewLevel, Skill, SpaceId, VerificationSettings, same_name};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where a setting's value is set, from the most general to the most specific. This Mac is a layer
/// of the Who runs what table only, its base, as subscriptions differ per Mac; every other machine
/// fact never cascades.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// Wiffletree's defaults, in code.
    Stock,
    /// The human, on every Mac: the shared folder.
    Workspace,
    /// This Mac's Who runs what table, which a Space's rows override.
    ThisMac,
    /// One Space, for its members: its `space.yaml`.
    Space,
    /// One repository within a Space: the same `space.yaml`, under `repository_settings`.
    Repository,
}
impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stock => "Stock",
            Self::Workspace => "Workspace",
            Self::ThisMac => "This Mac",
            Self::Space => "Space",
            Self::Repository => "Repository",
        }
    }
    /// A layer anyone who can push the Space's storage repository may write, so it may only make
    /// a safety setting stricter.
    pub fn is_shared_with_a_space(self) -> bool {
        matches!(self, Self::Space | Self::Repository)
    }
}

/// A setting's effective value, the layer it came from, and the earlier layers whose values it
/// replaced, Stock first. `notes` says why a layer's value was ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved<T> {
    pub value: T,
    pub from: Layer,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrode: Vec<Layer>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A setting as the resolver sees it: its stock value, the layers above Stock that may set it,
/// and what a layer may not change it to.
pub trait Setting {
    type Value: Clone + PartialEq;
    fn stock(&self) -> Self::Value;
    fn layers(&self) -> &[Layer];
    /// Why `layer` may not change the value it inherits to `value`, such as a looser safety
    /// value from a Space. Only called for a layer the setting allows.
    fn refusal(&self, layer: Layer, inherited: &Self::Value, value: &Self::Value)
    -> Option<String>;
}

/// Refuses saving `value` at `layer` over the value it inherits, as `resolve` would ignore it.
pub fn check<S: Setting>(
    setting: &S,
    layer: Layer,
    inherited: &S::Value,
    value: &S::Value,
) -> Result<()> {
    if let Some(refusal) = refused(setting, layer, inherited, value) {
        bail!("{refusal}");
    }
    Ok(())
}

fn refused<S: Setting>(
    setting: &S,
    layer: Layer,
    inherited: &S::Value,
    value: &S::Value,
) -> Option<String> {
    if !setting.layers().contains(&layer) {
        return Some(format!("it cannot be set for a {}", layer.name()));
    }
    setting.refusal(layer, inherited, value)
}

/// The effective value of `setting` given what each layer above Stock sets, in layer order. A
/// layer that may not set it, or would loosen a safety value, is ignored with a note.
pub fn resolve<S: Setting>(setting: &S, set: &[(Layer, S::Value)]) -> Resolved<S::Value> {
    let mut resolved = Resolved {
        value: setting.stock(),
        from: Layer::Stock,
        overrode: vec![],
        notes: vec![],
    };
    for (layer, value) in set {
        if let Some(refusal) = refused(setting, *layer, &resolved.value, value) {
            let ignored = format!("The {} value is ignored: {refusal}", layer.name());
            resolved.notes.push(ignored);
            continue;
        }
        resolved.overrode.push(resolved.from);
        resolved.value = value.clone();
        resolved.from = *layer;
    }
    resolved
}

/// A named, typed choice with a stock default that renders one sentence into its skill. Each
/// choice's sentence is part of that skill's stock version: changing one bumps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behavior {
    /// As layers store it, such as `review_depth`.
    pub id: String,
    pub name: String,
    pub skill: Skill,
    /// The layers above Stock that may set it.
    pub layers: Vec<Layer>,
    /// Stock first.
    pub choices: Vec<Choice>,
    /// A safety setting: each choice is stricter than the one before, and a Space or repository
    /// may only move to a stricter one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tightens: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// As layers store it, such as `full`.
    pub value: String,
    pub label: String,
    /// What the role's instructions say for this choice.
    pub sentence: String,
}
impl Behavior {
    pub fn choice(&self, value: &str) -> Option<&Choice> {
        self.choices.iter().find(|c| c.value == value)
    }
    fn strictness(&self, value: &str) -> Option<usize> {
        self.choices.iter().position(|c| c.value == value)
    }
}
impl Setting for Behavior {
    type Value = String;
    fn stock(&self) -> String {
        self.choices[0].value.clone()
    }
    fn layers(&self) -> &[Layer] {
        &self.layers
    }
    fn refusal(&self, layer: Layer, inherited: &String, value: &String) -> Option<String> {
        let Some(strictness) = self.strictness(value) else {
            return Some(format!("{} has no choice {value:?}", self.name));
        };
        let looser = self.strictness(inherited).is_some_and(|i| strictness < i);
        (self.tightens && layer.is_shared_with_a_space() && looser).then(|| {
            format!(
                "a {} may only make {} stricter than “{}”",
                layer.name(),
                self.name,
                self.choices[self.strictness(inherited).unwrap_or_default()].label
            )
        })
    }
}

/// The Behaviors a layer sets, as each Behavior's id and its choice's value. Ids this release
/// does not know are kept, so a write puts them back.
pub type Choices = BTreeMap<String, String>;

/// What a Space sets for one of its repositories, under `repository_settings` in `space.yaml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySettings {
    #[serde(default, skip_serializing_if = "Choices::is_empty")]
    pub behaviors: Choices,
    /// Added after the Space's additions for ticket agents working in this repository. Stored
    /// as `instructions`, as a Space's own are.
    #[serde(
        default,
        rename = "instructions",
        skip_serializing_if = "String::is_empty"
    )]
    pub additions: String,
    #[serde(flatten)]
    pub extra: crate::Extra,
}
impl RepositorySettings {
    pub fn is_empty(&self) -> bool {
        self.behaviors.is_empty() && self.additions.is_empty() && self.extra.is_empty()
    }
}

/// One layer a host command reads or sets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "layer", rename_all = "snake_case")]
pub enum Scope {
    Workspace,
    Space {
        space_id: SpaceId,
    },
    /// A repository, as `owner/name`, within a Space.
    Repository {
        space_id: SpaceId,
        repository: String,
    },
}
impl Scope {
    pub fn layer(&self) -> Layer {
        match self {
            Self::Workspace => Layer::Workspace,
            Self::Space { .. } => Layer::Space,
            Self::Repository { .. } => Layer::Repository,
        }
    }
}

/// What one layer sets. The Workspace's additions are each skill's own rules, saved with the
/// skill, so a Workspace layer has none here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerValues {
    pub behaviors: Choices,
    pub additions: String,
}

/// A Behavior as it resolves for one agent, project or ticket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedBehavior {
    pub id: String,
    #[serde(flatten)]
    pub resolved: Resolved<String>,
}

/// One layer's additions, in the order agents get them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Addition {
    pub layer: Layer,
    /// The skill a Workspace addition belongs to; a Space's or repository's reach every role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<Skill>,
    pub text: String,
}

/// What a project's or ticket's agents get, and where each value came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveSettings {
    pub space_id: SpaceId,
    /// The ticket's repository, as `owner/name`; `None` for a project, which stops at its Space.
    pub repository: Option<String>,
    pub behaviors: Vec<ResolvedBehavior>,
    pub additions: Vec<Addition>,
    /// Who runs what as the Space resolves it on this Mac, row by row.
    #[serde(default)]
    pub models: Vec<ResolvedRow>,
    pub reviews: ResolvedReviews,
}

/// A row of the Who runs what table as a Space resolves it: this Mac's, or the Space's on top.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRow {
    pub job: Job,
    #[serde(flatten)]
    pub resolved: Resolved<JobRow>,
}

/// What a Space sets about its reviews, under `reviews` in `space.yaml`; unset keys inherit the
/// Workspace's review settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewOverrides {
    /// The levels its coordinators may use, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rounds: Option<u32>,
    /// Full reviews per ticket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cycles: Option<u32>,
    #[serde(flatten)]
    pub extra: crate::Extra,
}
impl ReviewOverrides {
    pub fn is_empty(&self) -> bool {
        self.levels.is_none()
            && self.max_rounds.is_none()
            && self.max_cycles.is_none()
            && self.extra.is_empty()
    }
}

/// What a Space sets about models and reviews, as the Models page reads and saves it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceModelSettings {
    /// The rows of Who runs what the Space overrides.
    pub rows: Vec<JobRow>,
    pub reviews: ReviewOverrides,
}

/// The review settings a project's coordinator works under, each with where it came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedReviews {
    /// The levels the coordinator may use, by name, in configured order.
    pub levels: Resolved<Vec<String>>,
    pub max_rounds: Resolved<u32>,
    pub max_cycles: Resolved<u32>,
}
impl ResolvedReviews {
    pub fn allows(&self, level: &str) -> bool {
        self.levels.value.iter().any(|name| same_name(name, level))
    }
    /// `settings` with these caps, for one ticket's reviews.
    pub fn capped(&self, settings: &VerificationSettings) -> VerificationSettings {
        VerificationSettings {
            max_rounds: self.max_rounds.value,
            max_cycles: self.max_cycles.value,
            ..settings.clone()
        }
    }
}

/// The review settings the Workspace's `settings` and a Space's `overrides` give. A level name
/// this Mac does not configure, as a teammate's might be, is left out with a note.
pub fn resolve_reviews(
    settings: &VerificationSettings,
    overrides: &ReviewOverrides,
) -> ResolvedReviews {
    let stock = VerificationSettings::default();
    let cap = |name, stock: u32, max: u32, workspace: u32, space: Option<u32>| {
        let cap = Cap { name, stock, max };
        let mut set = vec![(Layer::Workspace, workspace)];
        set.extend(space.map(|value| (Layer::Space, value)));
        resolve(&cap, &set)
    };
    let levels = AllowedLevels(settings);
    let mut set = vec![(Layer::Workspace, levels.stock())];
    let mut unknown = vec![];
    if let Some(names) = &overrides.levels {
        let (known, missing): (Vec<&String>, Vec<&String>) =
            names.iter().partition(|name| levels.named(name).is_some());
        unknown = missing.into_iter().cloned().collect();
        set.push((Layer::Space, levels.in_order(known)));
    }
    let restricted = overrides.levels.is_some();
    let full = levels.full();
    let mut levels = resolve(&levels, &set);
    if !unknown.is_empty() {
        levels.notes.push(format!(
            "The Space's levels {} are not configured here",
            unknown.join(", ")
        ));
    }
    // A restriction that cannot stand still restricts: to the full reviews alone.
    if restricted && levels.from != Layer::Space {
        levels.value = full;
        levels.from = Layer::Space;
        levels
            .notes
            .push("so only this Mac's full-review levels may be used".into());
    }
    ResolvedReviews {
        levels,
        max_rounds: cap(
            "Rounds per review",
            stock.max_rounds,
            crate::MAX_VERIFICATION_ROUNDS,
            settings.max_rounds,
            overrides.max_rounds,
        ),
        max_cycles: cap(
            "Full reviews per ticket",
            stock.max_cycles,
            crate::MAX_VERIFICATION_CYCLES,
            settings.max_cycles,
            overrides.max_cycles,
        ),
    }
}

/// Refuses a Space's review overrides as `resolve_reviews` would ignore them.
pub fn check_reviews(settings: &VerificationSettings, overrides: &ReviewOverrides) -> Result<()> {
    let levels = AllowedLevels(settings);
    if let Some(names) = &overrides.levels {
        if let Some(missing) = names.iter().find(|name| levels.named(name).is_none()) {
            bail!(
                "No review level is named \"{}\". {}",
                missing.trim(),
                settings.name_a_level()
            );
        }
        let known = levels.in_order(names.iter().collect());
        check(&levels, Layer::Space, &levels.stock(), &known)?;
    }
    let stock = VerificationSettings::default();
    for (name, stock, max, value) in [
        (
            "Rounds per review",
            stock.max_rounds,
            crate::MAX_VERIFICATION_ROUNDS,
            overrides.max_rounds,
        ),
        (
            "Full reviews per ticket",
            stock.max_cycles,
            crate::MAX_VERIFICATION_CYCLES,
            overrides.max_cycles,
        ),
    ] {
        if let Some(value) = value {
            check(&Cap { name, stock, max }, Layer::Space, &stock, &value)?;
        }
    }
    Ok(())
}

/// A review cap: 1 to `max`.
struct Cap {
    name: &'static str,
    stock: u32,
    max: u32,
}
impl Setting for Cap {
    type Value = u32;
    fn stock(&self) -> u32 {
        self.stock
    }
    fn layers(&self) -> &[Layer] {
        &[Layer::Workspace, Layer::Space]
    }
    fn refusal(&self, _: Layer, _: &u32, value: &u32) -> Option<String> {
        (!(1..=self.max).contains(value)).then(|| format!("{} must be 1–{}", self.name, self.max))
    }
}

/// The review levels a coordinator may use: every configured level unless a Space names some.
struct AllowedLevels<'a>(&'a VerificationSettings);
impl AllowedLevels<'_> {
    fn named(&self, name: &str) -> Option<&ReviewLevel> {
        self.0.levels.iter().find(|l| same_name(&l.name, name))
    }
    /// The configured full-review levels' names, in configured order.
    fn full(&self) -> Vec<String> {
        (self.0.levels.iter())
            .filter(|l| l.kind == LevelKind::FullReview)
            .map(|l| l.name.clone())
            .collect()
    }
    /// The configured names of `names`, in configured order.
    fn in_order(&self, names: Vec<&String>) -> Vec<String> {
        (self.0.levels.iter())
            .filter(|l| names.iter().any(|name| same_name(name, &l.name)))
            .map(|l| l.name.clone())
            .collect()
    }
}
impl Setting for AllowedLevels<'_> {
    type Value = Vec<String>;
    fn stock(&self) -> Vec<String> {
        self.0.levels.iter().map(|l| l.name.clone()).collect()
    }
    fn layers(&self) -> &[Layer] {
        &[Layer::Workspace, Layer::Space]
    }
    fn refusal(&self, _: Layer, _: &Vec<String>, value: &Vec<String>) -> Option<String> {
        let full = (value.iter()).any(|name| {
            self.named(name)
                .is_some_and(|l| l.kind == LevelKind::FullReview)
        });
        (!full).then(|| "the levels must keep at least one full review".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Layer::*;

    fn behavior(layers: &[Layer], tightens: bool) -> Behavior {
        let choice = |value: &str| Choice {
            value: value.into(),
            label: value.into(),
            sentence: format!("Say {value}."),
        };
        Behavior {
            id: "publishing".into(),
            name: "Publishing".into(),
            skill: Skill::Coordinating,
            layers: layers.to_vec(),
            choices: vec![choice("owners"), choice("ask")],
            tightens,
        }
    }
    fn set(values: &[(Layer, &str)]) -> Vec<(Layer, String)> {
        values.iter().map(|(l, v)| (*l, v.to_string())).collect()
    }

    #[test]
    fn each_layer_overrides_the_one_before_and_says_which_it_overrode() {
        let behavior = behavior(&[Workspace, Space, Repository], false);
        let stock = resolve(&behavior, &[]);
        assert_eq!((stock.value.as_str(), stock.from), ("owners", Stock));
        assert!(stock.overrode.is_empty());
        let workspace = resolve(&behavior, &set(&[(Workspace, "ask")]));
        assert_eq!(
            (workspace.value.as_str(), workspace.from),
            ("ask", Workspace)
        );
        assert_eq!(workspace.overrode, [Stock]);
        let space = resolve(&behavior, &set(&[(Workspace, "ask"), (Space, "owners")]));
        assert_eq!((space.value.as_str(), space.from), ("owners", Space));
        assert_eq!(space.overrode, [Stock, Workspace]);
        let all = set(&[(Workspace, "ask"), (Space, "owners"), (Repository, "ask")]);
        let repository = resolve(&behavior, &all);
        assert_eq!(
            (repository.value.as_str(), repository.from),
            ("ask", Repository)
        );
        assert_eq!(repository.overrode, [Stock, Workspace, Space]);
        assert!(repository.notes.is_empty());
    }

    #[test]
    fn a_layer_the_setting_does_not_allow_is_refused_and_ignored() {
        let behavior = behavior(&[Workspace, Space], false);
        let refused = check(&behavior, Repository, &"owners".into(), &"ask".into());
        assert!(
            refused
                .unwrap_err()
                .to_string()
                .contains("cannot be set for a Repository")
        );
        let resolved = resolve(&behavior, &set(&[(Space, "ask"), (Repository, "owners")]));
        assert_eq!((resolved.value.as_str(), resolved.from), ("ask", Space));
        assert_eq!(resolved.notes.len(), 1);
        assert!(resolved.notes[0].starts_with("The Repository value is ignored"));
    }

    #[test]
    fn a_space_may_only_tighten_a_safety_setting() {
        let behavior = behavior(&[Workspace, Space, Repository], true);
        // The Workspace is the human's own, so it may loosen what it set itself.
        check(&behavior, Workspace, &"ask".into(), &"owners".into()).unwrap();
        check(&behavior, Space, &"owners".into(), &"ask".into()).unwrap();
        let looser = check(&behavior, Space, &"ask".into(), &"owners".into()).unwrap_err();
        assert!(
            looser
                .to_string()
                .contains("a Space may only make Publishing stricter than “ask”"),
            "{looser}"
        );
        // Read: the looser value is ignored with a note, at either shared layer.
        let read = resolve(&behavior, &set(&[(Workspace, "ask"), (Space, "owners")]));
        assert_eq!((read.value.as_str(), read.from), ("ask", Workspace));
        assert!(read.notes[0].contains("may only make Publishing stricter"));
        let repository = resolve(&behavior, &set(&[(Space, "ask"), (Repository, "owners")]));
        assert_eq!((repository.value.as_str(), repository.from), ("ask", Space));
        assert_eq!(repository.notes.len(), 1);
    }

    #[test]
    fn a_choice_the_behavior_does_not_offer_is_refused() {
        let behavior = behavior(&[Workspace], false);
        let unknown = check(&behavior, Workspace, &"owners".into(), &"often".into());
        assert!(
            unknown
                .unwrap_err()
                .to_string()
                .contains("no choice \"often\"")
        );
    }

    #[test]
    fn a_space_restriction_without_a_full_review_allows_only_the_full_reviews() {
        let mut settings = VerificationSettings::default();
        let full = settings.levels[0].clone();
        settings.levels.push(ReviewLevel {
            name: "Recheck".into(),
            kind: LevelKind::Recheck,
            ..full.clone()
        });
        for names in [vec!["Recheck"], vec!["A teammate's level"]] {
            let overrides = ReviewOverrides {
                levels: Some(names.iter().map(|n| n.to_string()).collect()),
                ..Default::default()
            };
            let levels = resolve_reviews(&settings, &overrides).levels;
            assert_eq!(levels.value, std::slice::from_ref(&full.name), "{names:?}");
            assert_eq!(levels.from, Space);
            let said = levels.notes.join(" ");
            assert!(
                said.contains("only this Mac's full-review levels"),
                "{said}"
            );
            assert!(check_reviews(&settings, &overrides).is_err());
        }
    }
}
