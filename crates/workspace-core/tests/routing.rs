use workspace_core::*;

#[test]
fn fixed_choice_and_bounded_orchestrator_selection() {
    let mut policy = default_policies().remove(0);
    let forbidden = ModelProfile {
        model: "unapproved-paid-model".into(),
        ..policy.default.clone()
    };
    assert_eq!(
        policy
            .select(Complexity::Complex, Some(&forbidden), None, None)
            .unwrap()
            .profile,
        policy.default
    );
    policy.mode = RoutingMode::Automatic;
    assert!(
        policy
            .select(Complexity::Complex, Some(&forbidden), None, None)
            .is_err()
    );
    let wrong_effort = ModelProfile {
        effort: "ultra".into(),
        ..policy.complex.clone()
    };
    assert!(
        policy
            .select(Complexity::Complex, Some(&wrong_effort), None, None)
            .is_err()
    );
    assert_eq!(
        policy
            .select(Complexity::Small, None, None, None)
            .unwrap()
            .profile,
        policy.small
    );
    assert_eq!(
        policy
            .select(Complexity::Complex, None, None, None)
            .unwrap()
            .profile,
        policy.complex
    );
}

#[test]
fn catalog_and_provider_mismatch_fail_without_fallback() {
    let policy = default_policies().remove(0);
    assert!(
        policy
            .select(Complexity::Small, None, Some(Provider::Codex), None)
            .is_err()
    );
    let catalog = [ModelCapability {
        provider: policy.default.provider,
        model: policy.default.model.clone(),
        efforts: vec!["low".into()],
    }];
    assert!(
        policy
            .select(Complexity::Standard, None, None, Some(&catalog))
            .is_err()
    );
    assert!(
        !policy
            .select(Complexity::Standard, None, None, None)
            .unwrap()
            .catalog_verified
    );
}

#[test]
fn capacity_is_per_project_and_released_on_failure() {
    let mut limits = TurnLimits::new(3).unwrap();
    limits.acquire("a", "one", 1).unwrap();
    assert!(limits.acquire("b", "one", 1).is_err());
    limits.acquire("b", "two", 2).unwrap();
    assert!(limits.acquire("b", "two", 2).is_err());
    limits.acquire("c", "two", 2).unwrap();
    assert!(limits.acquire("d", "three", 1).is_err());
    limits.release("b");
    limits.acquire("d", "three", 1).unwrap();
    assert_eq!(limits.active(), 3);
}
