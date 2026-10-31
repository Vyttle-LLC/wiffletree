//! Host settings: this machine's in `settings.json`, the shared ones through `SharedStore`, and
//! the readable folder layout they choose for new work.
use crate::*;

pub(crate) const FILE: &str = "settings.json";
/// The fields `settings.json` held before the shared folder. They stay there, unread and unwritten.
const MOVED_FIELDS: [&str; 4] = [
    "verification",
    "checkin_child_turns",
    "checkin_human_turns",
    "keeper",
];
const SLUG_LIMIT: usize = 48;

/// `~/wiffletree`, or `~/wiffletree-beta` for a beta build so the two never share folders.
fn default_workspaces_dir() -> String {
    let name = if option_env!("WIFFLETREE_CHANNEL") == Some("beta") {
        "wiffletree-beta"
    } else {
        "wiffletree"
    };
    std::env::home_dir()
        .unwrap_or_default()
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// Expands a leading `~` to the home directory.
pub(crate) fn expand_home(value: &str) -> Result<PathBuf> {
    Ok(if value == "~" || value.starts_with("~/") {
        std::env::home_dir()
            .context("Home directory unavailable")?
            .join(value.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(value)
    })
}

/// A readable, lowercase, filesystem-safe name, or `fallback` when nothing readable remains.
pub(crate) fn slug(text: &str, fallback: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(SLUG_LIMIT);
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        fallback.into()
    } else {
        slug.into()
    }
}

/// `base`, or the first of `base-2`, `base-3`… that is not taken.
fn unused(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| match n {
            1 => base.to_owned(),
            n => format!("{base}-{n}"),
        })
        .find(|candidate| !taken(candidate))
        .expect("an unused suffix exists")
}

/// The slug that names a project's folders: its home folder's name, or the slug a project from
/// before workspace folders was given for its first ticket.
fn project_slug(project: &Project) -> Option<String> {
    project
        .home
        .as_deref()
        .and_then(|home| Path::new(home).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .or_else(|| project.slug.clone())
}

/// The operating system's reason alone, without the path of a probe file the user never chose.
fn reason(error: &std::io::Error) -> std::io::Error {
    error
        .raw_os_error()
        .map(std::io::Error::from_raw_os_error)
        .unwrap_or_else(|| error.kind().into())
}

/// A keeper trigger's switch.
type Switch = fn(&mut KeeperSettings) -> &mut bool;

/// The owners allowed in the settings saved in `home`, read off the host thread; none when the
/// file is missing or invalid.
pub(crate) fn allowed_owners(home: &Path) -> Vec<String> {
    fs::read(home.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HostSettings>(&bytes).ok())
        .map(|settings| settings.allowed_owners)
        .unwrap_or_default()
}

impl Host {
    /// The saved settings: each shared one from its file, the rest from `settings.json`. A file
    /// that is missing, unreadable or invalid gives its defaults. A keeper trigger this machine
    /// cannot run is off, and stays on in the file.
    pub fn settings(&self) -> HostSettings {
        let local = fs::read(self.home.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| HostSettings::new(default_workspaces_dir()));
        let shared = self.shared();
        let (checkin_child_turns, checkin_human_turns) = shared.check_ins();
        let (verification, mut keeper) = (shared.review(), shared.keeper());
        for (switch, _) in self.held(&keeper, &verification) {
            *switch(&mut keeper) = false;
        }
        HostSettings {
            verification,
            checkin_child_turns,
            checkin_human_turns,
            keeper,
            ..local
        }
    }
    /// The triggers on in `keeper` that this machine's PR keeper model and review settings
    /// cannot run, as turned on from another machine, each with why.
    fn held(
        &self,
        keeper: &KeeperSettings,
        verification: &VerificationSettings,
    ) -> Vec<(Switch, String)> {
        let saved = self.saved_selection().ok().flatten();
        let model = saved.as_ref().and_then(ModelSelection::keeper);
        let switches: [Switch; 3] = [|k| &mut k.comments, |k| &mut k.conflicts, |k| &mut k.checks];
        let alone = |switch: Switch| {
            let mut alone = KeeperSettings {
                comments: false,
                conflicts: false,
                checks: false,
                ..keeper.clone()
            };
            *switch(&mut alone) = *switch(&mut keeper.clone());
            alone
        };
        let needs = |alone: KeeperSettings| match alone.comments {
            true => check_sorting_model(model),
            false => check_tries(&alone, model, verification),
        };
        (switches.into_iter())
            .filter_map(|switch| Some((switch, format!("{:#}", needs(alone(switch)).err()?))))
            .collect()
    }
    /// What the Repositories page says about each trigger held off on this machine.
    pub(crate) fn keeper_held(&self) -> Vec<String> {
        let shared = self.shared();
        let held = self.held(&shared.keeper(), &shared.review());
        let line = |(_, why)| format!("On from another machine, off on this Mac: {why}");
        held.into_iter().map(line).collect()
    }
    /// Saves who verifies tickets. An invalid setting is refused and the saved one stays.
    pub fn set_verification(&mut self, verification: VerificationSettings) -> Result<HostSettings> {
        verification.validate()?;
        let keeper = self.settings().keeper;
        check_tries(&keeper, self.model_selection()?.keeper(), &verification)?;
        self.shared().set_review(&verification)?;
        Ok(self.settings())
    }
    /// Saves a model selection with the verifiers sent with it, or neither: both are checked
    /// together first, and a failed selection write puts the review settings back as they were.
    pub fn set_setup(
        &mut self,
        selection: &ModelSelection,
        verification: Option<VerificationSettings>,
    ) -> Result<ModelSelection> {
        selection.validate()?;
        let Some(verification) = verification else {
            return self.set_model_selection(selection);
        };
        verification.validate()?;
        policies::check_keeper_needs(&self.settings().keeper, selection, &verification)?;
        self.put_back_on_error(shared::REVIEW, |host| {
            host.shared().set_review(&verification)?;
            host.set_model_selection(selection)
        })
    }
    /// Saves "Builds at once" on this machine. Waiting builds read it as they next check.
    pub fn set_builds_at_once(&mut self, builds: usize) -> Result<HostSettings> {
        ensure!(
            (1..=MAX_BUILDS_AT_ONCE).contains(&builds),
            "Builds at once must be 1–{MAX_BUILDS_AT_ONCE}"
        );
        let settings = HostSettings {
            builds_at_once: builds,
            ..self.settings()
        };
        self.save_settings(&settings)?;
        Ok(settings)
    }
    /// Saves the check-in thresholds. An invalid pair is refused and the saved one stays.
    pub fn set_check_ins(&mut self, child_turns: u32, human_turns: u32) -> Result<HostSettings> {
        HostSettings::validate_check_ins(child_turns, human_turns)?;
        self.shared().set_check_ins(child_turns, human_turns)?;
        Ok(self.settings())
    }
    /// Saves the PR keeper's settings. An invalid setting is refused and the saved one stays.
    /// Turning comment sorting on starts every PR's comments afresh at the next pass. A trigger
    /// held off on this machine stays on in the file unless the save turns it on, which is
    /// checked as any other, and still needs the rebase trigger.
    pub fn set_keeper(&mut self, keeper: KeeperSettings) -> Result<HostSettings> {
        keeper.validate()?;
        let selection = self.model_selection()?;
        if keeper.comments {
            check_sorting_model(selection.keeper())?;
        }
        let settings = self.settings();
        check_tries(&keeper, selection.keeper(), &settings.verification)?;
        if keeper.comments && !settings.keeper.comments {
            self.restart_comments()?;
        }
        let mut saved = keeper;
        for (switch, _) in self.held(&self.shared().keeper(), &settings.verification) {
            *switch(&mut saved) = true;
        }
        (saved.check_rebase())
            .map_err(|e| anyhow::anyhow!("{e:#}, and is on from another machine that runs it"))?;
        self.shared().set_keeper(&saved)?;
        Ok(self.settings())
    }
    /// Saves or removes a repository's checks command. The PR keeper reads it when it runs.
    pub fn set_repository_checks(
        &mut self,
        repository_id: &str,
        checks: Option<RepositoryChecks>,
    ) -> Result<Repository> {
        if let Some(checks) = &checks {
            checks.validate()?;
        }
        let repository = Repository {
            checks,
            ..self.repository(repository_id)?
        };
        self.db.execute(
            "UPDATE repositories SET data=?2 WHERE id=?1",
            params![repository.id, encode(&repository)?],
        )?;
        Ok(repository)
    }
    pub fn set_workspaces_dir(&mut self, path: &str) -> Result<HostSettings> {
        text(path, 4096)?;
        let folder = expand_home(path.trim())?;
        ensure!(
            folder.is_absolute(),
            "Choose an absolute folder for workspaces"
        );
        fs::create_dir_all(&folder)
            .map_err(|e| anyhow::anyhow!("Cannot create {}: {}", folder.display(), reason(&e)))?;
        // A new, uniquely named file: never one that exists, which could be a symlink elsewhere.
        tempfile::NamedTempFile::new_in(&folder)
            .and_then(|probe| probe.close())
            .map_err(|e| anyhow::anyhow!("Cannot write to {}: {}", folder.display(), reason(&e)))?;
        let settings = HostSettings {
            workspaces_dir: folder.to_string_lossy().into_owned(),
            ..self.settings()
        };
        self.save_settings(&settings)?;
        Ok(settings)
    }
    /// Allows an owner, or a single fork as `owner/name`, to have PRs opened without asking, or
    /// stops allowing it. Entries compare without regard to case.
    pub fn set_owner_allowed(&mut self, entry: &str, allowed: bool) -> Result<HostSettings> {
        let entry = entry.trim();
        let mut parts = entry.split('/');
        let named = |part: Option<&str>| {
            part.is_some_and(|p| {
                !p.is_empty()
                    && p.len() <= 100
                    && p.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            })
        };
        ensure!(
            named(parts.next())
                && parts.next().is_none_or(|name| named(Some(name)))
                && parts.next().is_none(),
            "Name a GitHub owner, or a fork as owner/name"
        );
        let mut settings = self.settings();
        settings
            .allowed_owners
            .retain(|e| !e.eq_ignore_ascii_case(entry));
        if allowed {
            settings.allowed_owners.push(entry.into());
        }
        self.save_settings(&settings)?;
        Ok(settings)
    }
    /// Writes this machine's settings atomically and keeps fields this version does not know.
    pub(crate) fn save_settings(&self, settings: &HostSettings) -> Result<()> {
        let path = self.home.join(FILE);
        let mut saved = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if let Value::Object(mut fields) = serde_json::to_value(settings)? {
            fields.retain(|field, _| !MOVED_FIELDS.contains(&field.as_str()));
            saved.as_object_mut().unwrap().extend(fields);
        }
        let mut staged = tempfile::NamedTempFile::new_in(&self.home)?;
        serde_json::to_writer_pretty(&mut staged, &saved)?;
        staged.persist(&path)?;
        Ok(())
    }
    /// Where the main coordinator runs. Projects from before workspace folders keep their
    /// original directory, because providers resume conversations by directory.
    pub fn project_directory(&self, project_id: &str) -> Result<PathBuf> {
        Ok(match self.project(project_id)?.home {
            Some(home) => PathBuf::from(home),
            None => self.home.join("projects").join(project_id),
        })
    }
    /// `<workspaces>/projects/<slug>` with a slug no other project or folder uses.
    pub(crate) fn new_project_home(&self, name: &str) -> Result<String> {
        let workspaces = PathBuf::from(self.settings().workspaces_dir);
        let slug = self.unused_project_slug(name, &workspaces)?;
        Ok(workspaces
            .join("projects")
            .join(slug)
            .to_string_lossy()
            .into_owned())
    }
    fn unused_project_slug(&self, name: &str, workspaces: &Path) -> Result<String> {
        let projects = self.projects()?;
        Ok(unused(&slug(name, "project"), |candidate| {
            projects
                .iter()
                .any(|p| project_slug(p).as_deref() == Some(candidate))
                || workspaces.join("projects").join(candidate).exists()
                || workspaces.join("tasks").join(candidate).exists()
        }))
    }
    /// The project's slug. A project from before workspace folders gets one on first use and
    /// keeps it, so renaming it never moves where its tickets go.
    fn fixed_project_slug(&self, project: &Project, workspaces: &Path) -> Result<String> {
        if let Some(slug) = project_slug(project) {
            return Ok(slug);
        }
        let slug = self.unused_project_slug(&project.name, workspaces)?;
        let mut project = project.clone();
        project.slug = Some(slug.clone());
        self.db.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![project.id, encode(&project)?],
        )?;
        Ok(slug)
    }
    /// `<workspaces>/tasks/<project>/<repository>/<ticket>` and its `wiffletree/<ticket>`
    /// branch, suffixing the ticket slug until neither is in use.
    pub(crate) fn new_ticket_place_observed(
        &self,
        project: &Project,
        repository: &Repository,
        title: &str,
        branches: &[String],
    ) -> Result<(PathBuf, String)> {
        let repository_path = Path::new(&repository.path);
        let workspaces = PathBuf::from(self.settings().workspaces_dir);
        let parent = workspaces
            .join("tasks")
            .join(self.fixed_project_slug(project, &workspaces)?)
            .join(slug(
                &repository_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                "repository",
            ));
        let mut tickets = self.tickets()?;
        tickets.extend(
            checkout::pending_creations(self)?
                .into_iter()
                .map(|c| c.ticket),
        );
        let slug = unused(&slug(title, "ticket"), |candidate| {
            let path = parent.join(candidate);
            let branch = format!("refs/heads/wiffletree/{candidate}");
            path.exists()
                || tickets.iter().any(|t| Path::new(&t.worktree) == path)
                || branches.iter().any(|r| {
                    r.strip_prefix(&branch)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                })
        });
        Ok((parent.join(&slug), format!("wiffletree/{slug}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_readable_safe_and_bounded() {
        assert_eq!(
            slug("  Fix: Inbound calls (v2)!  ", "ticket"),
            "fix-inbound-calls-v2"
        );
        assert_eq!(slug("Café ☕", "ticket"), "caf");
        assert_eq!(slug("../..", "ticket"), "ticket");
        assert_eq!(slug("", "project"), "project");
        let long = slug(&"word ".repeat(40), "ticket");
        assert!(long.len() <= SLUG_LIMIT && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn nothing_is_allowed_until_the_human_allows_it_and_the_list_survives_a_restart() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        assert!(host.settings().allowed_owners.is_empty());
        assert!(host.snapshot().unwrap().allowed_owners.is_empty());
        let allow = |host: &mut Host, entry: &str, allowed| {
            host.execute(Command::SetOwnerAllowed {
                entry: entry.into(),
                allowed,
            })
        };
        allow(&mut host, "Sage-Social", true).unwrap();
        allow(&mut host, "MHohlios/gpui-component", true).unwrap();
        // Allowing again, in any letter case, keeps one entry.
        allow(&mut host, "sage-social", true).unwrap();
        drop(host);
        let mut host = Host::open(home.path()).unwrap();
        assert_eq!(
            host.snapshot().unwrap().allowed_owners,
            ["MHohlios/gpui-component", "sage-social"]
        );
        allow(&mut host, "SAGE-SOCIAL", false).unwrap();
        assert_eq!(host.settings().allowed_owners, ["MHohlios/gpui-component"]);
        // Saving another setting keeps the list.
        host.set_check_ins(30, 100).unwrap();
        assert_eq!(host.settings().allowed_owners, ["MHohlios/gpui-component"]);
        for entry in ["", "a/b/c", "owner/", "has space", "--flag=x y"] {
            assert!(allow(&mut host, entry, true).is_err(), "{entry}");
        }
        assert_eq!(host.settings().allowed_owners, ["MHohlios/gpui-component"]);
    }

    #[test]
    fn the_keeper_is_off_until_the_human_saves_a_valid_setting_and_no_agent_tool_saves_one() {
        let home = tempfile::tempdir().unwrap();
        // Settings saved before the keeper existed.
        fs::write(
            home.path().join(FILE),
            r#"{"workspaces_dir": "/tmp/w", "allowed_owners": ["Sage-Social"]}"#,
        )
        .unwrap();
        let mut host = Host::open(home.path()).unwrap();
        assert_eq!(host.settings().keeper, KeeperSettings::default());
        assert!(!host.settings().keeper.rebase);
        assert_eq!(host.settings().keeper.quiet_minutes, 2);
        let on = KeeperSettings {
            rebase: true,
            quiet_minutes: 5,
            comments: false,
            ..KeeperSettings::default()
        };
        host.execute(Command::SetKeeper { keeper: on.clone() })
            .unwrap();
        for quiet_minutes in [0, 61] {
            let refused = KeeperSettings {
                rebase: true,
                quiet_minutes,
                comments: false,
                ..KeeperSettings::default()
            };
            assert!(host.set_keeper(refused).is_err(), "{quiet_minutes}");
        }
        assert_eq!(host.settings().keeper, on);
        assert_eq!(host.snapshot().unwrap().keeper, on);
        // Every workspace tool an agent has; none of them changes settings.
        let tools = mcp::tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"publish_ticket"), "{names:?}");
        assert!(
            names
                .iter()
                .all(|name| !["keeper", "setting", "check", "repositor"]
                    .iter()
                    .any(|word| name.contains(word))),
            "{names:?}"
        );
    }

    /// A host whose keeper may rebase, with a keeper model chosen and only the level "Full".
    fn rebasing() -> (tempfile::TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        let mut selection = host.model_selection().unwrap();
        selection.set_keeper(Some(crate::keeper::tests::keeper_line(Provider::Claude)));
        host.set_model_selection(&selection).unwrap();
        host.set_verification(VerificationSettings::default())
            .unwrap();
        let rebase = KeeperSettings {
            rebase: true,
            ..KeeperSettings::default()
        };
        host.set_keeper(rebase).unwrap();
        (home, host)
    }
    fn with_light(host: &mut Host) {
        let mut verification = host.settings().verification;
        verification.levels.push(ReviewLevel {
            name: "Light".into(),
            kind: LevelKind::Recheck,
            verifiers: vec!["Correctness".into()],
            use_when: "after a small fix.".into(),
        });
        host.set_verification(verification).unwrap();
    }
    fn conflicts(level: &str) -> KeeperSettings {
        KeeperSettings {
            rebase: true,
            conflicts: true,
            review_level: Some(level.into()),
            ..KeeperSettings::default()
        }
    }
    fn refusal(result: Result<impl std::fmt::Debug>) -> String {
        format!("{:#}", result.unwrap_err())
    }

    #[test]
    fn the_conflicts_trigger_needs_the_rebase_trigger_a_keeper_model_and_a_re_check_level() {
        let (_home, mut host) = rebasing();
        let saved = host.settings();
        let no_recheck = refusal(host.set_keeper(conflicts("Full")));
        assert!(
            no_recheck.contains("add one in Models → Reviews"),
            "{no_recheck}"
        );
        with_light(&mut host);
        let full = refusal(host.set_keeper(conflicts("Full")));
        assert!(full.contains("choose Light"), "{full}");
        let alone = KeeperSettings {
            rebase: false,
            ..conflicts("Light")
        };
        let alone = refusal(host.set_keeper(alone));
        assert!(
            alone.contains(&format!("needs \"{KEEPER_REBASE}\"")),
            "{alone}"
        );
        assert_eq!(host.settings().keeper, saved.keeper);
        host.set_keeper(conflicts("Light")).unwrap();
        // Turning the rebase trigger off alone is refused, and both stay on.
        let off = KeeperSettings {
            rebase: false,
            ..host.settings().keeper
        };
        assert!(host.set_keeper(off).is_err());
        assert_eq!(host.settings().keeper, conflicts("Light"));
        // Without a keeper model.
        let mut selection = host.model_selection().unwrap();
        selection.set_keeper(None);
        let dropped = refusal(host.set_model_selection(&selection));
        assert!(dropped.contains("needs a PR keeper model"), "{dropped}");
        assert!(host.model_selection().unwrap().keeper().is_some());
    }

    #[test]
    fn the_checks_trigger_needs_a_keeper_model_and_what_the_conflicts_trigger_needs() {
        let (_home, mut host) = rebasing();
        with_light(&mut host);
        let mut selection = host.model_selection().unwrap();
        selection.set_keeper(None);
        host.set_model_selection(&selection).unwrap();
        let checks = |rebase| KeeperSettings {
            rebase,
            checks: true,
            review_level: Some("Light".into()),
            ..KeeperSettings::default()
        };
        // No model.
        let no_model = refusal(host.set_keeper(checks(true)));
        assert!(
            no_model.contains(
                &format!("\"{KEEPER_TRIES}\" needs a PR keeper model. Choose a PR keeper model in Models first.")
            ),
            "{no_model}"
        );
        let (_home, mut host) = rebasing();
        with_light(&mut host);
        let alone = refusal(host.set_keeper(checks(false)));
        assert!(
            alone.contains(&format!("needs \"{KEEPER_REBASE}\"")),
            "{alone}"
        );
        let full = refusal(host.set_keeper(KeeperSettings {
            review_level: Some("Full".into()),
            ..checks(true)
        }));
        assert!(full.contains("choose Light"), "{full}");
        host.set_keeper(checks(true)).unwrap();
        assert!(
            host.set_keeper(checks(false)).is_err(),
            "the rebase trigger stays on"
        );
        let mut selection = host.model_selection().unwrap();
        selection.set_keeper(None);
        assert!(host.set_model_selection(&selection).is_err());
        assert_eq!(host.settings().keeper, checks(true));
    }

    #[test]
    fn the_keepers_level_cannot_be_removed_or_made_a_full_review_while_the_trigger_needs_it() {
        let (_home, mut host) = rebasing();
        with_light(&mut host);
        host.set_keeper(conflicts("Light")).unwrap();
        let saved = host.settings().verification;
        let mut removed = saved.clone();
        removed.levels.retain(|level| level.name != "Light");
        let refused = refusal(host.set_verification(removed));
        assert!(
            refused.contains(&format!("\"{KEEPER_TRIES}\"")),
            "{refused}"
        );
        let mut full = saved.clone();
        full.levels[1].kind = LevelKind::FullReview;
        assert!(host.set_verification(full).is_err());
        assert_eq!(host.settings().verification, saved);
    }

    #[test]
    fn a_repositorys_checks_command_is_saved_bounded_and_removed() {
        let (home, mut host) = rebasing();
        let repository = home.path().join("app");
        fs::create_dir_all(&repository).unwrap();
        let git = crate::test_support::git;
        git(&repository, &["init", "-q", "-b", "main"]);
        git(
            &repository,
            &["commit", "-q", "--allow-empty", "-m", "start"],
        );
        let added = host
            .add_repository(repository.to_str().unwrap(), "main")
            .unwrap();
        assert_eq!(added.checks, None);
        let checks = |command: &str, minutes| RepositoryChecks {
            command: command.into(),
            minutes,
        };
        let set = |host: &mut Host, checks| {
            host.execute(Command::SetRepositoryChecks {
                repository_id: added.id.clone(),
                checks,
            })
        };
        set(&mut host, Some(checks("cargo test", 20))).unwrap();
        for refused in [
            checks("cargo test", 0),
            checks("cargo test", 121),
            checks("  ", 20),
        ] {
            assert!(set(&mut host, Some(refused)).is_err());
        }
        let saved = host.repository(&added.id).unwrap().checks;
        assert_eq!(saved, Some(checks("cargo test", 20)));
        set(&mut host, None).unwrap();
        assert_eq!(host.repository(&added.id).unwrap().checks, None);
    }

    #[test]
    fn unused_suffixes_after_the_base() {
        let taken = ["notes", "notes-2"];
        assert_eq!(unused("notes", |c| taken.contains(&c)), "notes-3");
        assert_eq!(unused("other", |c| taken.contains(&c)), "other");
    }
    /// A host with no PR keeper model whose shared file turns on every trigger, as a pass
    /// would pull it from another machine.
    fn held_by_another_machine() -> (tempfile::TempDir, Host) {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        with_light(&mut host);
        let pulled = KeeperSettings {
            checks: true,
            comments: true,
            ..conflicts("Light")
        };
        shared::SharedStore::of(home.path())
            .set_keeper(&pulled)
            .unwrap();
        (home, host)
    }

    #[test]
    fn triggers_on_from_another_machine_are_held_off_here_until_a_keeper_model_is_chosen() {
        let (home, mut host) = held_by_another_machine();
        let file = home.path().join("shared/settings/pr-keeper.json");
        let saved = || serde_json::from_slice::<KeeperSettings>(&fs::read(&file).unwrap()).unwrap();
        let keeper = host.settings().keeper;
        assert!(keeper.rebase && !keeper.conflicts && !keeper.checks && !keeper.comments);
        let held = host.snapshot().unwrap().keeper_held;
        assert_eq!(held.len(), 3);
        for line in &held {
            assert!(
                line.starts_with("On from another machine, off on this Mac: ")
                    && line.contains("PR keeper model"),
                "{line}"
            );
        }
        // Every save that runs the keeper's checks counts the held triggers as off, and the
        // file keeps them on.
        let mut selection = host.model_selection().unwrap();
        host.set_model_selection(&selection).unwrap();
        let review = host.settings().verification;
        host.set_setup(&selection, Some(review.clone())).unwrap();
        host.set_verification(review).unwrap();
        let quiet = KeeperSettings {
            quiet_minutes: 5,
            ..host.settings().keeper
        };
        host.set_keeper(quiet).unwrap();
        let kept = saved();
        assert!(kept.conflicts && kept.checks && kept.comments && kept.quiet_minutes == 5);
        let on = KeeperSettings {
            checks: true,
            ..host.settings().keeper
        };
        let refused = refusal(host.set_keeper(on));
        assert!(refused.contains("needs a PR keeper model"), "{refused}");
        assert_eq!(saved(), kept);
        selection.set_keeper(Some(crate::keeper::tests::keeper_line(Provider::Claude)));
        host.set_model_selection(&selection).unwrap();
        let keeper = host.settings().keeper;
        assert!(keeper.conflicts && keeper.checks && keeper.comments);
        assert!(host.snapshot().unwrap().keeper_held.is_empty());
        selection.set_keeper(None);
        let removed = refusal(host.set_model_selection(&selection));
        assert!(removed.contains("PR keeper model"), "{removed}");
    }

    #[test]
    fn a_held_trigger_still_needs_the_rebase_trigger() {
        let (home, mut host) = held_by_another_machine();
        let file = home.path().join("shared/settings/pr-keeper.json");
        let before = fs::read(&file).unwrap();
        let rebase_off = KeeperSettings {
            rebase: false,
            ..host.settings().keeper
        };
        let refused = refusal(host.set_keeper(rebase_off));
        assert!(
            refused.contains(&format!(
                "needs \"{KEEPER_REBASE}\", and is on from another machine"
            )),
            "{refused}"
        );
        assert_eq!(fs::read(&file).unwrap(), before);
        // Nor is such a file pulled from another machine.
        let pulled = br#"{"rebase": false, "conflicts": true, "quiet_minutes": 2}"#;
        let refused = format!("{:#}", shared::check(shared::KEEPER, pulled).unwrap_err());
        assert!(refused.contains("needs \"Keep it rebased"), "{refused}");
    }
}
