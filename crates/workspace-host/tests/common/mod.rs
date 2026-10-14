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

/// `verify_ticket`'s arguments for the level "Full", with a valid choice for every configured
/// verifier.
pub fn verify_args(host: &Host, ticket_id: &str) -> Value {
    json!({"ticket_id": ticket_id, "level": "Full", "verifiers": verifier_choices(host)})
}

/// Verification by the default tester alone, for tests that pass or fail one verifier.
pub fn one_tester(host: &mut Host) {
    let mut verification = VerificationSettings::default();
    verification.verifiers.truncate(1);
    verification.levels[0].verifiers.truncate(1);
    host.set_verification(verification).unwrap();
}

/// A process's start time in microseconds, as the kernel reports it.
pub fn process_started(pid: i32) -> i64 {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    assert_eq!(read, size, "process {pid} is not running");
    (info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec) as i64
}
