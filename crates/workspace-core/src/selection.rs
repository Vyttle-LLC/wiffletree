//! Which models may run on this machine, which providers each role uses, and the human's guide.
//! Coordinators choose exact models within these; the human never is limited.
use crate::{ModelProfile, Provider, Role, VerifierConfig};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// The order pickers and messages list providers in.
pub const PROVIDERS: [Provider; 2] = [Provider::Claude, Provider::Codex];
/// Roles whose provider set the host enforces. Reviewers take their provider from their verifier.
pub const PROVIDER_ROLES: [Role; 3] = [Role::ProjectOrchestrator, Role::Implementer, Role::Tester];
/// Six legacy role policies of up to 32 profiles each must fit after migration.
pub const MAX_MODELS_PER_PROVIDER: usize = 6 * 32;
pub const MAX_GUIDE_BYTES: usize = 32 * 1024;
pub const MAX_REASON_CHARS: usize = 200;

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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    /// Increases on every save; selections and turns record the revision they were checked against.
    pub revision: u64,
    pub providers: BTreeMap<Provider, ProviderAccess>,
    pub role_providers: BTreeMap<Role, BTreeSet<Provider>>,
    /// The human's advice on choosing a model within a provider. Never parsed.
    pub guide: String,
}
/// Whether this machine has the provider's subscription, and which of its models may run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAccess {
    pub enabled: bool,
    pub models: Vec<AllowedModel>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AllowedModel {
    pub model: String,
    pub effort: String,
}

/// Why a coordinator's choice was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    WrongProvider {
        role: Role,
        provider: Provider,
        allowed: Vec<Provider>,
    },
    WrongVerifierProvider {
        verifier: String,
        provider: Provider,
        configured: Provider,
    },
    NotAllowed {
        profile: ModelProfile,
        enabled: bool,
        allowed: Vec<AllowedModel>,
    },
}
impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongProvider {
                role,
                provider,
                allowed,
            } => {
                let uses: Vec<_> = allowed.iter().map(|p| p.label()).collect();
                write!(
                    f,
                    "{} is not configured for {} on this machine. {} uses: {}.",
                    provider.label(),
                    role.label(),
                    role.label(),
                    if uses.is_empty() {
                        "nothing".into()
                    } else {
                        uses.join(", ")
                    }
                )
            }
            Self::WrongVerifierProvider {
                verifier,
                provider,
                configured,
            } => write!(
                f,
                "{} is not configured for {verifier}. {verifier} uses: {}.",
                provider.label(),
                configured.label()
            ),
            Self::NotAllowed {
                profile,
                enabled: false,
                ..
            } => write!(
                f,
                "{profile} is not allowed on this machine. {} is disabled.",
                profile.provider.label()
            ),
            Self::NotAllowed {
                profile, allowed, ..
            } => {
                let models: Vec<_> = allowed
                    .iter()
                    .map(|m| format!("{} · {}", m.model, m.effort))
                    .collect();
                write!(
                    f,
                    "{profile} is not allowed on this machine. Allowed {} models: {}.",
                    profile.provider.label(),
                    models.join(", ")
                )
            }
        }
    }
}
impl std::error::Error for Rejection {}

impl ModelSelection {
    /// The providers whose models a coordinator may give `role`, in display order.
    pub fn providers_for(&self, role: Role) -> Vec<Provider> {
        PROVIDERS
            .into_iter()
            .filter(|&p| {
                self.is_enabled(p)
                    && (role == Role::Reviewer
                        || self
                            .role_providers
                            .get(&role)
                            .is_some_and(|s| s.contains(&p)))
            })
            .collect()
    }
    pub fn is_enabled(&self, provider: Provider) -> bool {
        self.providers.get(&provider).is_some_and(|a| a.enabled)
    }
    pub fn allowed(&self, provider: Provider) -> &[AllowedModel] {
        self.providers
            .get(&provider)
            .filter(|a| a.enabled)
            .map_or(&[], |a| &a.models)
    }
    /// Checks a coordinator's choice. Reviewers skip the role check; every role needs the
    /// model to be allowed on this machine.
    pub fn permits(&self, role: Role, profile: &ModelProfile) -> Result<(), Rejection> {
        if role != Role::Reviewer
            && !self
                .role_providers
                .get(&role)
                .is_some_and(|s| s.contains(&profile.provider))
        {
            return Err(Rejection::WrongProvider {
                role,
                provider: profile.provider,
                allowed: self.providers_for(role),
            });
        }
        let allowed = self.allowed(profile.provider);
        if allowed
            .iter()
            .any(|m| m.model == profile.model && m.effort == profile.effort)
        {
            return Ok(());
        }
        Err(Rejection::NotAllowed {
            profile: profile.clone(),
            enabled: self.is_enabled(profile.provider),
            allowed: allowed.to_vec(),
        })
    }
    /// Checks a coordinator's choice for a configured verifier: its provider, when it names one,
    /// then everything `permits` checks for its role.
    pub fn permits_verifier(
        &self,
        verifier: &VerifierConfig,
        profile: &ModelProfile,
    ) -> Result<(), Rejection> {
        if let Some(configured) = verifier.provider
            && configured != profile.provider
        {
            return Err(Rejection::WrongVerifierProvider {
                verifier: verifier.role.agent_label(Some(&verifier.focus)),
                provider: profile.provider,
                configured,
            });
        }
        self.permits(verifier.role, profile)
    }
    /// What a human picker starts on: the role's first provider, else the first enabled
    /// provider, and that provider's first allowed model.
    pub fn prefill(&self, role: Role) -> Option<ModelProfile> {
        let preferred = self.role_providers.get(&role);
        let provider = PROVIDERS
            .into_iter()
            .filter(|p| preferred.is_some_and(|s| s.contains(p)))
            .chain(PROVIDERS)
            .find(|&p| !self.allowed(p).is_empty())?;
        let model = self.allowed(provider).first()?;
        Some(ModelProfile {
            provider,
            model: model.model.clone(),
            effort: model.effort.clone(),
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            PROVIDERS.iter().any(|&p| self.is_enabled(p)),
            "Enable at least one provider"
        );
        for (provider, access) in &self.providers {
            ensure!(
                !access.enabled || !access.models.is_empty(),
                "Allow at least one {} model or disable {}",
                provider.label(),
                provider.label()
            );
            ensure!(
                access.models.len() <= MAX_MODELS_PER_PROVIDER,
                "Allow at most {MAX_MODELS_PER_PROVIDER} models per provider"
            );
            let unique: BTreeSet<_> = access.models.iter().collect();
            ensure!(
                unique.len() == access.models.len(),
                "Duplicate {} model",
                provider.label()
            );
            for m in &access.models {
                ensure!(
                    !m.model.trim().is_empty() && m.model.len() <= 128,
                    "Invalid model ID"
                );
                ensure!(
                    !m.effort.trim().is_empty() && m.effort.len() <= 32,
                    "Invalid effort"
                );
            }
        }
        for role in PROVIDER_ROLES {
            let providers = self.role_providers.get(&role);
            ensure!(
                providers.is_some_and(|s| !s.is_empty()),
                "Choose a provider for {}",
                role.label()
            );
            for &p in providers.into_iter().flatten() {
                ensure!(
                    self.is_enabled(p),
                    "{} uses {}, which is disabled",
                    role.label(),
                    p.label()
                );
            }
        }
        ensure!(
            self.role_providers
                .keys()
                .all(|r| PROVIDER_ROLES.contains(r)),
            "Only the coordinator, implementers and testers have role providers"
        );
        ensure!(
            self.guide.len() <= MAX_GUIDE_BYTES,
            "The guide must be at most 32 KB"
        );
        Ok(())
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
    Coordinator { session_id: String },
    Human,
    Default,
}
impl Selection {
    /// A coordinator's reason for a model: see `one_line_reason`.
    pub fn check_reason(reason: &str) -> Result<&str> {
        ensure!(
            !reason.trim().is_empty(),
            "Give a one-line reason for this model"
        );
        one_line_reason(reason)
    }
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
