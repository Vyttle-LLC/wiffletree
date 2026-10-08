use crate::*;
use std::collections::BTreeSet;

impl Host {
    pub fn set_role_defaults(&mut self, defaults: &[RoleDefault]) -> Result<()> {
        let roles = [
            Role::ProjectOrchestrator,
            Role::Implementer,
            Role::Tester,
            Role::Reviewer,
        ];
        ensure!(
            defaults.len() == roles.len(),
            "Provide the four workspace role defaults"
        );
        let saved = self.policies()?;
        let mut policies = Vec::new();
        for role in roles {
            let entries: Vec<_> = defaults.iter().filter(|entry| entry.role == role).collect();
            ensure!(
                entries.len() == 1,
                "Provide exactly one default for {}",
                role.label()
            );
            let entry = entries[0];
            let primary = entry
                .profiles
                .iter()
                .find(|p| p.provider == entry.default_provider)
                .context("Default provider has no configured profiles")?;
            let allowed: BTreeSet<_> = entry
                .profiles
                .iter()
                .flat_map(|p| [p.big.clone(), p.small.clone()])
                .collect();
            for role in if role == Role::ProjectOrchestrator {
                vec![role, Role::TaskOrchestrator]
            } else {
                vec![role]
            } {
                let policy = RolePolicy {
                    role,
                    mode: RoutingMode::Automatic,
                    default: primary.big.clone(),
                    allowed: allowed.iter().cloned().collect(),
                    small: primary.small.clone(),
                    standard: primary.big.clone(),
                    complex: primary.big.clone(),
                    provider_profiles: entry.profiles.clone(),
                    turn_budget_minutes: saved
                        .iter()
                        .find(|p| p.role == role)
                        .context("Role policy missing")?
                        .turn_budget_minutes,
                };
                policy.validate()?;
                policies.push(policy);
            }
        }
        let sessions = self.sessions()?;
        let mut runtimes = self.runtimes()?;
        let tx = self.db.transaction()?;
        for policy in policies {
            let previous = saved
                .iter()
                .find(|p| p.role == policy.role)
                .context("Role policy missing")?;
            for session in sessions.iter().filter(|s| s.role == policy.role) {
                if runtimes
                    .iter()
                    .any(|r| r.session_id == session.id && r.profile.is_some())
                {
                    continue;
                }
                let mut runtime = runtimes
                    .iter()
                    .find(|r| r.session_id == session.id)
                    .cloned()
                    .unwrap_or(SessionRuntime {
                        session_id: session.id.clone(),
                        ..Default::default()
                    });
                runtime.profile = Some(
                    previous
                        .select(Complexity::Standard, None, Some(session.provider), None)?
                        .profile,
                );
                tx.execute("INSERT INTO runtimes VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET data=excluded.data", params![session.id, encode(&runtime)?])?;
                runtimes.push(runtime);
            }
            tx.execute(
                "UPDATE policies SET data=?2 WHERE role=?1",
                params![tag(&policy.role)?, encode(&policy)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn migrate_opus_defaults(&mut self) -> Result<()> {
        self.db
            .execute_batch("CREATE TABLE IF NOT EXISTS policy_migrations (id TEXT PRIMARY KEY)")?;
        let migrated = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM policy_migrations WHERE id='opus-defaults')",
            [],
            |r| r.get::<_, bool>(0),
        )?;
        if migrated {
            return Ok(());
        }
        let sessions = self.sessions()?;
        let runtimes = self.runtimes()?;
        let tx = self.db.transaction()?;
        for policy in default_policies()
            .into_iter()
            .filter(|p| p.default.provider == Provider::Claude)
        {
            let sol = ModelProfile {
                provider: Provider::Codex,
                model: "gpt-6.1-sol".into(),
                effort: "medium".into(),
            };
            let small = ModelProfile {
                effort: "low".into(),
                ..sol.clone()
            };
            let complex = ModelProfile {
                provider: Provider::Codex,
                model: "gpt-6-astra".into(),
                effort: "high".into(),
            };
            let legacy = RolePolicy {
                role: policy.role,
                mode: RoutingMode::Fixed,
                default: sol.clone(),
                allowed: vec![small.clone(), sol.clone(), complex.clone()],
                small,
                standard: sol.clone(),
                complex,
                provider_profiles: Vec::new(),
                turn_budget_minutes: None,
            };
            let saved: RolePolicy = tx.query_row(
                "SELECT data FROM policies WHERE role=?1",
                [tag(&policy.role)?],
                |r| decode(r, 0),
            )?;
            if saved != legacy {
                continue;
            }
            // Freeze existing agents before changing defaults for future agents.
            for session in sessions
                .iter()
                .filter(|s| s.role == policy.role && s.provider == Provider::Codex)
            {
                let mut runtime = runtimes
                    .iter()
                    .find(|r| r.session_id == session.id)
                    .cloned()
                    .unwrap_or(SessionRuntime {
                        session_id: session.id.clone(),
                        ..Default::default()
                    });
                if runtime.profile.is_none() {
                    runtime.profile = Some(sol.clone());
                    tx.execute("INSERT INTO runtimes VALUES (?1,?2) ON CONFLICT(session_id) DO UPDATE SET data=excluded.data", params![session.id, encode(&runtime)?])?;
                }
            }
            tx.execute(
                "UPDATE policies SET data=?2 WHERE role=?1",
                params![tag(&policy.role)?, encode(&policy)?],
            )?;
        }
        tx.execute("INSERT INTO policy_migrations VALUES ('opus-defaults')", [])?;
        tx.commit()?;
        Ok(())
    }
}
