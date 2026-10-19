use std::collections::BTreeSet;
use workspace_core::*;

const NOW: i64 = 1_000_000;
const MONDAY: i64 = NOW + 3_600_000;
const THURSDAY: i64 = NOW + 7_200_000;

fn profile(provider: Provider, model: &str, effort: &str) -> ModelProfile {
    ModelProfile {
        provider,
        model: model.into(),
        effort: effort.into(),
    }
}
fn opus() -> ModelProfile {
    profile(Provider::Claude, "opus", "medium")
}
fn sol() -> ModelProfile {
    profile(Provider::Codex, "gpt-6.1-sol", "high")
}
fn sonnet() -> ModelProfile {
    profile(Provider::Claude, "sonnet", "high")
}
fn window(label: &str, used: Option<f64>, resets_at: Option<i64>) -> QuotaWindow {
    QuotaWindow {
        key: label.to_lowercase(),
        label: label.into(),
        used_percent: used,
        observed_at: NOW - 60_000,
        duration_mins: None,
        resets_at,
        status: None,
    }
}
fn reading(provider: Provider, windows: Vec<QuotaWindow>) -> QuotaReading {
    QuotaReading {
        provider,
        account: None,
        observed_at: NOW - 60_000,
        source: "test".into(),
        windows,
        error: None,
    }
}
fn limits<'a>(quotas: &'a [QuotaReading], unusable: &'a BTreeSet<Provider>) -> Limits<'a> {
    Limits {
        quotas,
        unusable,
        switch_at: 95,
        now: NOW,
    }
}
fn when(at: i64) -> String {
    match at {
        MONDAY => "Mon 09:00".into(),
        THURSDAY => "Thu 14:00".into(),
        _ => at.to_string(),
    }
}
fn implementer() -> JobRow {
    JobRow {
        job: Job::Implementer,
        main: Line {
            first: opus(),
            then: Then::Model(sol()),
        },
        small: Some(Line {
            first: sonnet(),
            then: Then::Model(profile(Provider::Codex, "gpt-6.1-sol", "medium")),
        }),
    }
}
fn claude_weekly(used: f64) -> Vec<QuotaReading> {
    vec![reading(
        Provider::Claude,
        vec![
            window("5-hour", Some(41.), Some(NOW + 60_000)),
            window("Weekly", Some(used), Some(MONDAY)),
        ],
    )]
}

#[test]
fn a_window_at_the_threshold_is_spent_until_it_resets() {
    let none = BTreeSet::new();
    let quotas = claude_weekly(96.);
    let spent = limits(&quotas, &none).spent(Provider::Claude).unwrap();
    assert_eq!(
        spent,
        Spent {
            provider: Provider::Claude,
            window: "weekly".into(),
            used: Some(96.),
            until: Some(MONDAY),
        }
    );
    // Exactly at the threshold counts; below it does not.
    assert!(
        limits(&claude_weekly(95.), &none)
            .spent(Provider::Claude)
            .is_some()
    );
    assert!(
        limits(&claude_weekly(94.9), &none)
            .spent(Provider::Claude)
            .is_none()
    );
    // A reset that has passed no longer counts.
    let mut passed = claude_weekly(99.);
    passed[0].windows[1].resets_at = Some(NOW - 1);
    assert!(limits(&passed, &none).spent(Provider::Claude).is_none());
}

#[test]
fn an_unknown_reset_counts_until_a_reading_below_the_threshold() {
    let none = BTreeSet::new();
    let mut unknown = claude_weekly(97.);
    unknown[0].windows[1].resets_at = None;
    let spent = limits(&unknown, &none).spent(Provider::Claude).unwrap();
    assert_eq!(spent.until, None);
    unknown[0].windows[1].used_percent = Some(60.);
    assert!(limits(&unknown, &none).spent(Provider::Claude).is_none());
}

#[test]
fn no_reading_is_not_spent_and_a_reached_limit_is() {
    let none = BTreeSet::new();
    assert!(limits(&[], &none).spent(Provider::Claude).is_none());
    // A reading with no windows, such as one that failed, says nothing.
    let empty = [reading(Provider::Codex, vec![])];
    assert!(limits(&empty, &none).spent(Provider::Codex).is_none());
    // Claude's report that the limit is reached has no percentage.
    let mut rejected = window("5-hour", None, Some(MONDAY));
    rejected.status = Some("rejected".into());
    let quotas = [reading(Provider::Claude, vec![rejected])];
    let spent = limits(&quotas, &none).spent(Provider::Claude).unwrap();
    assert_eq!((spent.used, spent.until), (None, Some(MONDAY)));
    // A warning is not a reached limit.
    let mut warning = window("5-hour", None, Some(MONDAY));
    warning.status = Some("allowed_warning".into());
    let quotas = [reading(Provider::Claude, vec![warning])];
    assert!(limits(&quotas, &none).spent(Provider::Claude).is_none());
}

#[test]
fn a_spent_provider_waits_for_the_latest_of_its_counting_resets() {
    let none = BTreeSet::new();
    let quotas = [reading(
        Provider::Codex,
        vec![
            window("5-hour", Some(99.), Some(MONDAY)),
            window("Weekly", Some(96.), Some(THURSDAY)),
        ],
    )];
    let spent = limits(&quotas, &none).spent(Provider::Codex).unwrap();
    assert_eq!(
        (spent.window.as_str(), spent.until),
        ("5-hour", Some(THURSDAY))
    );
}

#[test]
fn the_first_choice_runs_unless_spent_then_the_next() {
    let none = BTreeSet::new();
    let row = implementer();
    for quotas in [vec![], claude_weekly(80.)] {
        let pick = resolve(&row, false, false, &limits(&quotas, &none), &when).unwrap();
        assert_eq!(
            pick,
            Pick {
                profile: opus(),
                waits: false,
                skip: None,
                until: None,
                reason: "First choice".into()
            }
        );
    }
    let quotas = claude_weekly(96.);
    let pick = resolve(&row, false, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!(pick.profile, sol());
    assert!(!pick.waits);
    assert_eq!(
        pick.reason,
        "Started on Codex: Claude's weekly limit was at 96% (moves on at 95%). First choice was Opus · medium."
    );
}

#[test]
fn small_work_uses_its_own_line_when_the_row_has_one() {
    let none = BTreeSet::new();
    let pick = resolve(&implementer(), true, false, &limits(&[], &none), &when).unwrap();
    assert_eq!(
        (pick.profile, pick.reason.as_str()),
        (sonnet(), "First choice for small work")
    );
    let quotas = claude_weekly(99.);
    let pick = resolve(&implementer(), true, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!(
        pick.profile,
        profile(Provider::Codex, "gpt-6.1-sol", "medium")
    );
    assert!(pick.reason.starts_with("Started on Codex for small work: "));
    // Without a small line, small work resolves the main line.
    let tester = JobRow {
        small: None,
        ..implementer()
    };
    let pick = resolve(&tester, true, false, &limits(&[], &none), &when).unwrap();
    assert_eq!(
        (pick.profile, pick.reason.as_str()),
        (opus(), "First choice")
    );
}

#[test]
fn a_job_without_another_choice_waits_for_the_reset() {
    let none = BTreeSet::new();
    let reviewer = JobRow {
        job: Job::Verifier {
            focus: "Correctness".into(),
        },
        main: Line {
            first: opus(),
            then: Then::Wait,
        },
        small: None,
    };
    let quotas = claude_weekly(96.);
    let pick = resolve(&reviewer, false, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!(
        pick,
        Pick {
            profile: opus(),
            waits: true,
            skip: None,
            until: Some(MONDAY),
            reason: "Waits for Claude: its weekly limit is at 96% (moves on at 95%), and no other choice can run. Starts after the reset, Mon 09:00.".into()
        }
    );
}

#[test]
fn every_choice_spent_pins_the_earliest_reset() {
    let none = BTreeSet::new();
    let codex_until = |at| reading(Provider::Codex, vec![window("Weekly", Some(97.), at)]);
    // Claude resets Monday, Codex Thursday: Claude.
    let quotas = [claude_weekly(96.).remove(0), codex_until(Some(THURSDAY))];
    let pick = resolve(&implementer(), false, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!(
        (pick.profile, pick.waits, pick.until),
        (opus(), true, Some(MONDAY))
    );
    assert!(
        pick.reason.ends_with("Starts after the reset, Mon 09:00."),
        "{}",
        pick.reason
    );
    // Codex resets first: Codex, though it is the second choice.
    let mut later = claude_weekly(96.).remove(0);
    later.windows[1].resets_at = Some(THURSDAY);
    let quotas = [later, codex_until(Some(MONDAY))];
    let pick = resolve(&implementer(), false, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!((pick.profile, pick.until), (sol(), Some(MONDAY)));
    // An unknown reset comes after any known one.
    let quotas = [claude_weekly(96.).remove(0), codex_until(None)];
    let pick = resolve(&implementer(), false, false, &limits(&quotas, &none), &when).unwrap();
    assert_eq!(pick.profile, opus());
}

#[test]
fn a_signed_out_provider_is_never_chosen() {
    let claude_out = BTreeSet::from([Provider::Claude]);
    let pick = resolve(
        &implementer(),
        false,
        false,
        &limits(&[], &claude_out),
        &when,
    )
    .unwrap();
    assert_eq!(pick.profile, sol());
    assert_eq!(
        pick.reason,
        "Started on Codex: Claude is not signed in on this Mac. First choice was Opus · medium."
    );
    // A spent then choice is waited for rather than running on the signed-out first.
    let quotas = [reading(
        Provider::Codex,
        vec![window("Weekly", Some(99.), Some(MONDAY))],
    )];
    let pick = resolve(
        &implementer(),
        false,
        false,
        &limits(&quotas, &claude_out),
        &when,
    )
    .unwrap();
    assert_eq!((pick.profile, pick.waits), (sol(), true));
    let both_out = BTreeSet::from(PROVIDERS);
    let refused =
        resolve(&implementer(), false, false, &limits(&[], &both_out), &when).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "Implementer cannot start on this Mac: Claude and Codex are not signed in. Sign in, or change its row in Models → Who runs what."
    );
}

fn table() -> ModelSelection {
    let row = |job, first: ModelProfile, then: Then| JobRow {
        job,
        main: Line { first, then },
        small: None,
    };
    ModelSelection {
        revision: 1,
        jobs: vec![
            row(Job::Coordinator, opus(), Then::Model(sol())),
            implementer(),
            row(Job::Tester, sonnet(), Then::Wait),
            row(
                Job::Verifier {
                    focus: "Correctness".into(),
                },
                opus(),
                Then::Wait,
            ),
        ],
        ..ModelSelection::default()
    }
}

#[test]
fn the_table_needs_each_fixed_job_once_and_a_then_on_another_subscription() {
    table().validate().unwrap();
    let mut missing = table();
    missing.jobs.remove(2);
    assert_eq!(
        missing.validate().unwrap_err().to_string(),
        "Choose who runs the Tester job"
    );
    let mut twice = table();
    twice.jobs.push(twice.jobs[3].clone());
    twice.jobs[4].job = Job::Verifier {
        focus: " correctness ".into(),
    };
    assert!(twice.validate().is_err());
    let mut same = table();
    same.jobs[1].main.then = Then::Model(sonnet());
    assert_eq!(
        same.validate().unwrap_err().to_string(),
        "Implementer: both choices use the Claude subscription, so they would be spent together. Choose a Codex model or wait."
    );
    let mut threshold = table();
    threshold.switch_at = 49;
    assert!(threshold.validate().is_err());
    let mut keepers = table();
    let keeper = JobRow {
        job: Job::Keeper,
        ..keepers.jobs[2].clone()
    };
    keepers.jobs.extend([keeper.clone(), keeper]);
    assert!(keepers.validate().is_err());
}

fn skipping_reviewer() -> JobRow {
    JobRow {
        job: Job::Verifier {
            focus: "Correctness".into(),
        },
        main: Line {
            first: opus(),
            then: Then::Skip,
        },
        small: None,
    }
}

#[test]
fn a_skip_row_skips_only_once_its_first_choice_cannot_run() {
    let none = BTreeSet::new();
    let row = skipping_reviewer();
    let pick = resolve(
        &row,
        false,
        true,
        &limits(&claude_weekly(80.), &none),
        &when,
    )
    .unwrap();
    assert_eq!((pick.profile, pick.skip), (opus(), None));
    let pick = resolve(
        &row,
        false,
        true,
        &limits(&claude_weekly(96.), &none),
        &when,
    )
    .unwrap();
    assert!(!pick.waits);
    assert_eq!(pick.until, Some(MONDAY));
    // Right now says it in the present; the review's record, afterwards.
    assert_eq!(
        pick.reason,
        "Claude's weekly limit is at 96% (moves on at 95%)"
    );
    assert_eq!(
        pick.skip.as_deref(),
        Some("Claude's weekly limit was at 96%; its row says skip")
    );
    let claude_out = BTreeSet::from([Provider::Claude]);
    let pick = resolve(&row, false, true, &limits(&[], &claude_out), &when).unwrap();
    assert_eq!(
        pick.skip.as_deref(),
        Some("Claude is not signed in on this Mac; its row says skip")
    );
    // Where it may not skip, such as with an open finding, it waits as a wait row does.
    let pick = resolve(
        &row,
        false,
        false,
        &limits(&claude_weekly(96.), &none),
        &when,
    )
    .unwrap();
    assert_eq!((pick.waits, pick.skip), (true, None));
}

#[test]
fn only_verifier_rows_may_skip() {
    let mut selection = table();
    selection.jobs[3] = skipping_reviewer();
    selection.jobs[3].small = Some(selection.jobs[3].main.clone());
    selection.validate().unwrap();
    for index in 0..3 {
        let mut refused = table();
        refused.jobs[index].main.then = Then::Skip;
        let name = refused.jobs[index].job.name();
        assert_eq!(
            refused.validate().unwrap_err().to_string(),
            format!("{name}: only reviews can be skipped. Choose a Codex model or wait.")
        );
    }
}

#[test]
fn a_saved_line_reads_wait_model_or_skip() {
    let first = serde_json::to_value(opus()).unwrap();
    let read = |line: serde_json::Value| serde_json::from_value::<Line>(line).unwrap().then;
    assert_eq!(read(serde_json::json!({"first": first})), Then::Wait);
    assert_eq!(
        read(serde_json::json!({"first": first, "then": sol()})),
        Then::Model(sol())
    );
    assert_eq!(
        read(serde_json::json!({"first": first, "then": "skip"})),
        Then::Skip
    );
    for then in [Then::Wait, Then::Model(sol()), Then::Skip] {
        let line = Line {
            first: opus(),
            then,
        };
        let saved = serde_json::to_value(&line).unwrap();
        assert_eq!(saved.get("then").is_none(), line.then == Then::Wait);
        assert_eq!(serde_json::from_value::<Line>(saved).unwrap(), line);
    }
}

#[test]
fn rows_follow_the_configured_verifiers() {
    let mut verification = VerificationSettings::default();
    let mut selection = table();
    selection.fit(&verification);
    // Regressions has no row: it uses a copy of the first reviewer's; Tests, a tester, too.
    let names: Vec<_> = selection.jobs.iter().map(|r| r.job.name()).collect();
    assert_eq!(
        names,
        [
            "Coordinator",
            "Implementer",
            "Tester",
            "Tests",
            "Correctness",
            "Regressions"
        ]
    );
    assert_eq!(selection.copied, ["Tests", "Regressions"]);
    assert_eq!(selection.jobs[5].main, selection.jobs[4].main);
    // A verifier no longer configured loses its row.
    verification.verifiers.retain(|v| v.focus != "Correctness");
    verification.levels[0]
        .verifiers
        .retain(|f| f != "Correctness");
    let mut selection = table();
    selection.fit(&verification);
    assert!(
        selection
            .row(&Job::Verifier {
                focus: "Correctness".into()
            })
            .is_none()
    );
    // With no reviewer row to copy, the tester's is used.
    assert_eq!(
        selection
            .row(&Job::Verifier {
                focus: "Regressions".into()
            })
            .unwrap()
            .main,
        table().jobs[2].main
    );
}

#[test]
fn a_role_and_focus_name_their_job() {
    let verification = VerificationSettings::default();
    let job = |role, focus| Job::for_role(role, focus, &verification);
    let verifier = |focus: &str| {
        Some(Job::Verifier {
            focus: focus.into(),
        })
    };
    assert_eq!(job(Role::Implementer, None), Some(Job::Implementer));
    assert_eq!(
        job(Role::Reviewer, Some(" regressions ")),
        verifier("Regressions")
    );
    // An ad-hoc reviewer is like the first configured reviewer.
    assert_eq!(
        job(Role::Reviewer, Some("Security")),
        verifier("Correctness")
    );
    assert_eq!(job(Role::Tester, Some("Tests")), verifier("Tests"));
    assert_eq!(job(Role::Tester, Some("Load")), Some(Job::Tester));
    assert_eq!(job(Role::Maintenance, None), None);
}

#[test]
fn sorting_uses_only_the_keepers_claude_choice() {
    let mut selection = table();
    assert_eq!(selection.sorting_model(), None);
    let keeper = |first: ModelProfile, then: Then| JobRow {
        job: Job::Keeper,
        main: Line { first, then },
        small: None,
    };
    selection.jobs.push(keeper(sol(), Then::Model(sonnet())));
    assert_eq!(selection.sorting_model(), Some(&sonnet()));
    check_sorting_model(selection.keeper()).unwrap();
    *selection.row_mut(&Job::Keeper).unwrap() = keeper(sol(), Then::Wait);
    assert_eq!(selection.sorting_model(), None);
    assert!(
        check_sorting_model(selection.keeper())
            .unwrap_err()
            .to_string()
            .starts_with("Sorting comments needs a Claude model")
    );
    assert!(
        check_sorting_model(None)
            .unwrap_err()
            .to_string()
            .contains("Choose a PR keeper model in Models first")
    );
}
