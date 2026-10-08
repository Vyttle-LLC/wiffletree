//! Helpers shared by the host's integration tests. Each test binary uses only some of them.
#![allow(dead_code)]
use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

/// A valid choice for every configured verifier: the first allowed model of its provider, or of
/// its role's first provider.
pub fn verifier_choices(host: &Host) -> Vec<VerifierChoice> {
    let selection = host.model_selection().unwrap();
    host.settings()
        .verification
        .verifiers
        .iter()
        .map(|verifier| {
            let provider = verifier
                .provider
                .unwrap_or_else(|| selection.providers_for(verifier.role)[0]);
            let model = &selection.allowed(provider)[0];
            VerifierChoice {
                focus: verifier.focus.clone(),
                profile: ModelProfile {
                    provider,
                    model: model.model.clone(),
                    effort: model.effort.clone(),
                },
                reason: "Test verifier".into(),
            }
        })
        .collect()
}

/// `verify_ticket`'s arguments with a valid choice for every configured verifier.
pub fn verify_args(host: &Host, ticket_id: &str) -> Value {
    json!({"ticket_id": ticket_id, "verifiers": verifier_choices(host)})
}

/// Verification by the default tester alone, for tests that pass or fail one verifier.
pub fn one_tester(host: &mut Host) {
    let mut verification = VerificationSettings::default();
    verification.verifiers.truncate(1);
    host.set_verification(verification).unwrap();
}
