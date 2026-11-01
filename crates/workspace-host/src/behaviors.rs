//! Behaviors (OpenSpec change `settings-cascade`): the catalog, versioned with the stock skills,
//! where each layer keeps its choices, and how an agent's instructions compose from them. The
//! Workspace's choices are `settings/behaviors.json` in the shared folder, synced like the other
//! shared settings; a Space's, and its repositories', are in its `space.yaml`. Only the human's
//! commands write any of them; no agent tool reaches them.
use crate::*;
use workspace_core::cascade::{
    self, Addition, Behavior, Choice, Choices, EffectiveSettings, Layer, LayerValues,
    RepositorySettings, ResolvedBehavior, Scope,
};

/// Review depth: the Behavior `verify_ticket` enforces.
pub(crate) const REVIEW_DEPTH: &str = "review_depth";
pub(crate) const FULL_REVIEW: &str = "full";
/// Publishing: the Behavior `publish_ticket` enforces.
pub(crate) const PUBLISHING: &str = "publishing";
pub(crate) const ALWAYS_ASK: &str = "always_ask";

/// Every Behavior this release knows, by role. A choice's sentence is part of its skill's stock
/// text: changing one bumps that skill's version, and a passage a Behavior controls is said
/// here and not in the stock text.
pub fn catalog() -> Vec<Behavior> {
    use Layer::{Repository, Space, Workspace};
    let all = vec![Workspace, Space, Repository];
    let no_repository = vec![Workspace, Space];
    let choice = |value: &str, label: &str, sentence: &str| Choice {
        value: value.into(),
        label: label.into(),
        sentence: sentence.into(),
    };
    let toggle = |on: &str, off: &str| [choice("on", "On", on), choice("off", "Off", off)];
    let behavior =
        |id: &str, name: &str, skill, layers: &Vec<Layer>, choices: Vec<Choice>| Behavior {
            id: id.into(),
            name: name.into(),
            skill,
            layers: layers.clone(),
            choices,
            tightens: false,
        };
    vec![
        behavior(
            "mockup_before_ui_spec",
            "Mockup before a UI spec",
            Skill::Coordinating,
            &all,
            toggle(
                "For UI-facing changes, produce a static HTML mockup in the human's own terms and get it approved before writing a spec.",
                "",
            )
            .into(),
        ),
        behavior(
            "fetch_before_landing_order",
            "Fetch before naming a landing order",
            Skill::Coordinating,
            &all,
            toggle(
                "Before recommending a landing order or claiming a branch is current, fetch origin and report how far main has moved.",
                "",
            )
            .into(),
        ),
        behavior(
            "confirm_relayed_answer",
            "Confirm a relayed answer moved the child",
            Skill::Coordinating,
            &no_repository,
            toggle(
                "After relaying an answer to a blocked child, confirm its status changed before telling the human work is continuing.",
                "",
            )
            .into(),
        ),
        behavior(
            "status_table",
            "Status table",
            Skill::Coordinating,
            &no_repository,
            vec![
                choice(
                    "on_finish",
                    "On finish or when asked",
                    "End with the status table only when you tell the human a ticket or the project is finished, accepted, published or ready for them, or when they ask for status. The table pinned above your chat already shows each ticket's live status and PR, so a routine update does not restate it.",
                ),
                choice(
                    "every_update",
                    "With every update",
                    "End every update to the human with the status table, routine ones included.",
                ),
            ],
        ),
        Behavior {
            tightens: true,
            ..behavior(
                REVIEW_DEPTH,
                "Review depth",
                Skill::Coordinating,
                &all,
                vec![
                    choice(
                        "proportional",
                        "Proportional to risk",
                        "Spend review in proportion to risk. A comment, doc or format-only fix needs at most a re-check level, never a new full review.",
                    ),
                    choice(
                        FULL_REVIEW,
                        "Full review for every ticket",
                        "Give every ticket a full review: verify_ticket refuses re-check levels for this work.",
                    ),
                ],
            )
        },
        Behavior {
            tightens: true,
            ..behavior(
                PUBLISHING,
                "Publishing",
                Skill::Coordinating,
                &all,
                vec![
                    choice(
                        "allowed_owners",
                        "As allowed owners say",
                        "",
                    ),
                    choice(
                        ALWAYS_ASK,
                        "Always ask first",
                        "Every publish_ticket waits for the human's approval in their Inbox, even in an allowed owner's repository, so tell the human when one is waiting.",
                    ),
                ],
            )
        },
        behavior(
            "wip_commits",
            "Commit work in progress at milestones",
            Skill::Implementing,
            &all,
            toggle(
                "For multi-step work (rebases, migrations, large refactors), commit WIP at each milestone so a stopped or interrupted turn loses nothing; squash before reporting ready_for_testing.",
                "Commit once the work is ready for testing, not at each milestone.",
            )
            .into(),
        ),
        behavior(
            "optional_findings",
            "Optional findings per round",
            Skill::Reviewing,
            &all,
            ["5", "3", "10"]
                .map(|count| {
                    let words = match count {
                        "3" => "three",
                        "10" => "ten",
                        _ => "five",
                    };
                    choice(
                        count,
                        count,
                        &format!("Report at most {words} non_blocking or pre_existing findings per round, the most consequential first; fold findings of the same kind into one."),
                    )
                })
                .into(),
        ),
        behavior(
            "answer_length",
            "Answer length",
            Skill::Assisting,
            &no_repository,
            vec![
                choice(
                    "brief",
                    "Brief",
                    "Answer the question asked, as briefly as it allows: a one-line question gets a short answer.",
                ),
                choice(
                    "thorough",
                    "Thorough",
                    "Answer the question asked thoroughly: give your reasoning, what you weighed and the details the human would otherwise ask for next.",
                ),
            ],
        ),
        behavior(
            "drafts",
            "Drafts",
            Skill::Assisting,
            &no_repository,
            vec![
                choice(
                    "markdown_and_html",
                    "Markdown, HTML for visuals",
                    "Prefer Markdown for text and a single self-contained HTML file for anything visual.",
                ),
                choice(
                    "markdown_only",
                    "Markdown only",
                    "Write every draft in Markdown, visuals included, such as a table or a Mermaid diagram.",
                ),
            ],
        ),
    ]
}

/// The sentences of `skill`'s Behaviors at their stock choices.
pub fn stock_sentences(skill: Skill) -> String {
    Layers::default().sentences(skill)
}

/// The sentences of `skill`'s Behaviors as the Workspace of the data folder `home` sets them,
/// for an agent outside every Space.
pub fn workspace_sentences(home: &Path, skill: Skill) -> String {
    let layers = Layers {
        workspace: shared::SharedStore::of(home).behaviors(),
        ..Layers::default()
    };
    layers.sentences(skill)
}

/// The Workspace's choices as `settings/behaviors.json` holds them.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct WorkspaceBehaviors {
    #[serde(default)]
    pub(crate) behaviors: Choices,
}

/// Refuses Workspace choices this release would refuse to save: a choice a Behavior does not
/// offer. Ids this release does not know are a newer release's and are kept.
pub(crate) fn check_workspace(choices: &Choices) -> Result<()> {
    for behavior in catalog() {
        if let Some(value) = choices.get(&behavior.id) {
            let stock = cascade::Setting::stock(&behavior);
            cascade::check(&behavior, Layer::Workspace, &stock, value)
                .with_context(|| format!("{} can't be saved", behavior.name))?;
        }
    }
    Ok(())
}

/// `chosen`, with the choices `saved` holds for Behaviors this release does not know.
fn keep_unknown(saved: &Choices, mut chosen: Choices) -> Choices {
    let known: Vec<String> = catalog().into_iter().map(|b| b.id).collect();
    for (id, value) in saved {
        if !known.contains(id) {
            chosen.entry(id.clone()).or_insert_with(|| value.clone());
        }
    }
    chosen
}

/// What each layer sets, for one agent, project or ticket. A coordinator, thread or job has no
/// single repository and stops at its Space.
#[derive(Clone, Debug, Default)]
pub(crate) struct Layers {
    pub(crate) workspace: Choices,
    pub(crate) space: Option<Space>,
    /// The ticket's repository as `owner/name`, which resolves within `space`.
    pub(crate) repository: Option<String>,
}
impl Layers {
    fn repository_settings(&self) -> Option<&RepositorySettings> {
        let repository = self.repository.as_deref()?;
        self.space.as_ref()?.repository_settings(repository)
    }
    fn set(&self, id: &str) -> Vec<(Layer, String)> {
        let space = self.space.as_ref().map(|space| &space.behaviors);
        let repository = self.repository_settings().map(|r| &r.behaviors);
        [
            (Layer::Workspace, Some(&self.workspace)),
            (Layer::Space, space),
            (Layer::Repository, repository),
        ]
        .into_iter()
        .filter_map(|(layer, choices)| Some((layer, choices?.get(id)?.clone())))
        .collect()
    }
    /// Every Behavior in catalog order, resolved with where it came from.
    pub(crate) fn resolve(&self) -> Vec<ResolvedBehavior> {
        catalog()
            .into_iter()
            .map(|behavior| ResolvedBehavior {
                resolved: cascade::resolve(&behavior, &self.set(&behavior.id)),
                id: behavior.id,
            })
            .collect()
    }
    /// The value `id` resolves to.
    pub(crate) fn value(&self, id: &str) -> String {
        let behavior = (catalog().into_iter()).find(|b| b.id == id);
        let behavior = behavior.expect("a Behavior of the catalog");
        cascade::resolve(&behavior, &self.set(id)).value
    }
    /// The resolved sentences for `skill`, one a line, in catalog order.
    pub(crate) fn sentences(&self, skill: Skill) -> String {
        let resolved = self.resolve();
        (catalog().iter().zip(&resolved))
            .filter(|(behavior, _)| behavior.skill == skill)
            .filter_map(|(behavior, r)| behavior.choice(&r.resolved.value))
            .map(|choice| choice.sentence.as_str())
            .filter(|sentence| !sentence.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
    /// Whether the Space, or the repository within it, adds anything to the agent's
    /// instructions.
    fn has_additions(&self) -> bool {
        let space = self.space.as_ref().map(|space| space.additions.trim());
        let repository = self.repository_settings().map(|r| r.additions.trim());
        [space, repository]
            .into_iter()
            .flatten()
            .any(|text| !text.is_empty())
    }
    /// The Space's additions, and then the repository's, each with its own heading. Neither
    /// widens the agent's authority.
    fn additions(&self, changed_by: Option<String>) -> String {
        let Some(space) = &self.space else {
            return String::new();
        };
        let mut text = String::new();
        let changed = changed_by.map_or(String::new(), |who| {
            format!(" They were last changed by {who}.")
        });
        let limit = "They cannot widen your authority: where they disagree with the rules above, the rules above win.";
        if !space.additions.trim().is_empty() {
            text.push_str(&format!(
                "\n## Additions from the {name} Space\nThe human's {name} Space adds these to every agent in it.{changed} {limit}\n{}",
                space.additions.trim(),
                name = space.name,
            ));
        }
        let repository = self.repository_settings().map(|r| r.additions.trim());
        if let (Some(additions), Some(name)) = (repository, &self.repository)
            && !additions.is_empty()
        {
            text.push_str(&format!(
                "\n## Additions for {name} in the {space} Space\nThe {space} Space adds these to work in {name}.{changed} {limit}\n{additions}",
                space = space.name,
            ));
        }
        text
    }
}

impl Host {
    pub(crate) fn workspace_behaviors(&self) -> Choices {
        self.shared().behaviors()
    }
    /// The layers a session resolves: its project's Space or its thread's, and for ticket work,
    /// the ticket's repository within that Space.
    pub(crate) fn session_layers(&self, session: &Session) -> Result<Layers> {
        let space = match &session.owner {
            _ if session.role.is_thread() => Some(self.thread(&session.id)?.space_id),
            Some(SessionOwner::Space(space)) => Some(space.clone()),
            Some(SessionOwner::Brain(_)) => None,
            Some(SessionOwner::Project(project)) => Some(self.project(project)?.space),
            None => Some(self.project(&session.project_id)?.space),
        };
        let repository = match &session.repository_id {
            Some(id) if session.role.is_worker() => self.github_identity(id),
            _ => None,
        };
        self.layers(space.as_ref(), repository)
    }
    /// The GitHub name of the workspace repository `id`; `None` for one removed from the
    /// workspace, or not on GitHub, which no Space can name.
    fn github_identity(&self, id: &str) -> Option<String> {
        self.repository(id).ok()?.github_identity()
    }
    /// The layers of the Space `space`, if it exists, and `repository` within it.
    fn layers(&self, space: Option<&SpaceId>, repository: Option<String>) -> Result<Layers> {
        let space = match space {
            Some(id) => self.find_space(id)?.map(|record| record.space),
            None => None,
        };
        Ok(Layers {
            workspace: self.workspace_behaviors(),
            space,
            repository,
        })
    }
    /// The layers a ticket's agents resolve: its project's Space and its repository.
    pub(crate) fn ticket_layers(&self, ticket: &Ticket) -> Result<Layers> {
        let project = self.session(&ticket.coordinator_id)?.project_id;
        let space = self.project(&project)?.space;
        let repository = self.github_identity(&ticket.repository_id);
        self.layers(Some(&space), repository)
    }
    /// What a session's turns are sent: the core, its skill's stock text or the human's
    /// rewrite, the resolved Behaviors, then the additions of the Workspace, its Space and, for
    /// ticket work, its repository, in that order. A thread's say which issue it was opened on
    /// before its Space's additions.
    pub fn session_instructions(&self, session: &Session) -> Result<String> {
        let layers = self.session_layers(session)?;
        let mut text = self.composed(session.role, &layers);
        if session.role.is_thread() {
            text.push_str(&self.attached_section(session)?);
        }
        // Who changed them comes from Git, asked only when there are additions to name.
        let changed_by = (layers.space.as_ref())
            .filter(|_| layers.has_additions())
            .and_then(|space| self.space_changed_by(space));
        text.push_str(&layers.additions(changed_by));
        Ok(text)
    }
    /// A role's instructions up to its Workspace additions, with Behaviors resolved over
    /// `layers`.
    fn composed(&self, role: Role, layers: &Layers) -> String {
        let behaviors = Skill::for_role(role).map_or(String::new(), |s| layers.sentences(s));
        self.skills().instructions(role, &behaviors)
    }
    /// Whether the Publishing Behavior makes every publish of `ticket` ask the human.
    pub(crate) fn always_asks_before_publishing(&self, ticket: &Ticket) -> Result<bool> {
        Ok(self.ticket_layers(ticket)?.value(PUBLISHING) == ALWAYS_ASK)
    }
    /// Whether the Review depth Behavior gives `ticket` only full reviews.
    pub(crate) fn wants_full_reviews(&self, ticket: &Ticket) -> Result<bool> {
        Ok(self.ticket_layers(ticket)?.value(REVIEW_DEPTH) == FULL_REVIEW)
    }

    /// What `scope` sets itself, without what it inherits.
    pub fn layer_values(&self, scope: &Scope) -> Result<LayerValues> {
        Ok(match scope {
            Scope::Workspace => LayerValues {
                behaviors: self.workspace_behaviors(),
                additions: String::new(),
            },
            Scope::Space { space_id } => {
                let space = self.space(space_id)?.space;
                LayerValues {
                    behaviors: space.behaviors,
                    additions: space.additions,
                }
            }
            Scope::Repository {
                space_id,
                repository,
            } => {
                let space = self.space(space_id)?.space;
                let settings = space.repository_settings(repository).cloned();
                let settings = settings.unwrap_or_default();
                LayerValues {
                    behaviors: settings.behaviors,
                    additions: settings.additions,
                }
            }
        })
    }
    /// Saves what `scope` sets, replacing what it set before. Each choice is refused as the
    /// resolver would ignore it: a layer the Behavior does not allow, or a Space or repository
    /// loosening a safety Behavior it inherits on this Mac. Ids this release does not know stay.
    pub fn set_layer_values(&mut self, scope: &Scope, values: LayerValues) -> Result<LayerValues> {
        let LayerValues {
            behaviors,
            additions,
        } = values;
        let behaviors = keep_unknown(&self.layer_values(scope)?.behaviors, behaviors);
        let (space_id, repository) = match scope {
            Scope::Workspace => {
                ensure!(
                    additions.is_empty(),
                    "The Workspace's additions are each skill's own rules; save them with the skill"
                );
                check_workspace(&behaviors)?;
                self.put_back_on_error(shared::BEHAVIORS, |host| {
                    host.shared().set_behaviors(&behaviors)
                })?;
                return self.layer_values(scope);
            }
            Scope::Space { space_id } => (space_id, None),
            Scope::Repository {
                space_id,
                repository,
            } => (space_id, Some(repository.clone())),
        };
        let mut space = self.space(space_id)?.space;
        if let Some(repository) = &repository {
            ensure!(
                (space.repositories.iter()).any(|listed| listed.eq_ignore_ascii_case(repository)),
                "{} does not list {repository}; add it to the Space first",
                space.name
            );
        }
        match repository {
            None => {
                space.behaviors = behaviors;
                space.additions = additions;
            }
            Some(repository) => {
                let key = (space.repository_settings.keys())
                    .find(|named| named.eq_ignore_ascii_case(&repository))
                    .cloned()
                    .unwrap_or(repository);
                let mut settings = space.repository_settings.remove(&key).unwrap_or_default();
                settings.behaviors = behaviors;
                settings.additions = additions;
                if !settings.is_empty() {
                    space.repository_settings.insert(key, settings);
                }
            }
        }
        self.update_space(space)?;
        self.layer_values(scope)
    }
    /// Refuses the Behavior choices a save of `space` changes from `current` as the resolver
    /// would ignore them: a layer the Behavior does not allow, or the Space or a repository
    /// loosening a safety Behavior it inherits on this Mac. A choice it leaves as it was, such
    /// as a looser one a teammate saved, stays readable and is ignored on read.
    pub(crate) fn check_space_behaviors(&self, current: &Space, space: &Space) -> Result<()> {
        let workspace = self.workspace_behaviors();
        let check = |layer, inherited: &Layers, saved: &Choices, choices: &Choices| {
            for behavior in catalog() {
                let value = choices.get(&behavior.id);
                if let Some(value) = value.filter(|v| saved.get(&behavior.id) != Some(*v)) {
                    let from = inherited.value(&behavior.id);
                    cascade::check(&behavior, layer, &from, value)
                        .with_context(|| format!("{} can't be saved", behavior.name))?;
                }
            }
            anyhow::Ok(())
        };
        let below_space = Layers {
            workspace: workspace.clone(),
            ..Layers::default()
        };
        check(
            Layer::Space,
            &below_space,
            &current.behaviors,
            &space.behaviors,
        )?;
        let below_repository = Layers {
            workspace,
            space: Some(space.clone()),
            repository: None,
        };
        for (repository, settings) in &space.repository_settings {
            let saved = current
                .repository_settings(repository)
                .map(|r| &r.behaviors);
            let saved = saved.cloned().unwrap_or_default();
            check(
                Layer::Repository,
                &below_repository,
                &saved,
                &settings.behaviors,
            )?;
        }
        Ok(())
    }
    /// What a project's agents get, or with `ticket_id` that ticket's agents, and where each
    /// value came from. A project stops at its Space; a ticket resolves its repository in it.
    pub fn effective_settings(
        &self,
        project_id: &str,
        ticket_id: Option<&str>,
    ) -> Result<EffectiveSettings> {
        let project = self.project(project_id)?;
        let layers = match ticket_id {
            Some(ticket_id) => {
                let ticket = self.ticket(ticket_id)?;
                let owner = self.session(&ticket.coordinator_id)?.project_id;
                ensure!(owner == project.id, "The ticket is not in this project");
                self.ticket_layers(&ticket)?
            }
            None => self.layers(Some(&project.space), None)?,
        };
        let mut additions: Vec<Addition> = Skill::ALL
            .into_iter()
            .map(|skill| (skill, self.skills().state(skill)))
            .filter(|(_, state)| state.rewrite.is_none() && !state.rules.trim().is_empty())
            .map(|(skill, state)| Addition {
                layer: Layer::Workspace,
                skill: Some(skill),
                text: state.rules,
            })
            .collect();
        if let Some(space) = &layers.space
            && !space.additions.trim().is_empty()
        {
            additions.push(Addition {
                layer: Layer::Space,
                skill: None,
                text: space.additions.clone(),
            });
        }
        if let Some(settings) = layers.repository_settings()
            && !settings.additions.trim().is_empty()
        {
            additions.push(Addition {
                layer: Layer::Repository,
                skill: None,
                text: settings.additions.clone(),
            });
        }
        let (_, unusable, _) = self.limits()?;
        let table = self.space_table(Some(&project.space), &unusable)?;
        Ok(EffectiveSettings {
            models: table.rows,
            small_models: table.small_rows,
            reviews: self.space_reviews(&project.space)?,
            space_id: project.space,
            repository: layers.repository.clone(),
            behaviors: layers.resolve(),
            additions,
        })
    }
    /// The coordinator's Behaviors for `workspace_context`: each as its project's Space resolves
    /// it, and each repository of the project whose Space entry changes one of them.
    pub(crate) fn behaviors_context(&self, project_id: &str) -> Result<Value> {
        let project = self.project(project_id)?;
        let layers = self.layers(Some(&project.space), None)?;
        let resolved = layers.resolve();
        let mut repositories = serde_json::Map::new();
        for repository in self.project_repositories(project_id)? {
            let Some(identity) = repository.github_identity() else {
                continue;
            };
            let within = Layers {
                repository: Some(identity.clone()),
                ..layers.clone()
            };
            let differ: Vec<ResolvedBehavior> = (within.resolve().into_iter())
                .filter(|r| r.resolved.from == Layer::Repository || !r.resolved.notes.is_empty())
                .collect();
            if !differ.is_empty() {
                repositories.insert(identity, serde_json::to_value(differ)?);
            }
        }
        Ok(json!({"space": resolved, "repositories": repositories}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::tests::{Fixture, fixture};

    /// The fixture's repository.
    const APP: &str = "allowed-org/app";

    /// A new Space `name` listing the fixture's repository.
    fn space_listing_app(f: &mut Fixture, name: &str) -> SpaceId {
        let mut space = f.host.create_space(name, "", "", None).unwrap().space;
        space.repositories.push(APP.into());
        let id = space.id.clone();
        f.host.update_space(space).unwrap();
        id
    }
    /// The fixture's project, moved into a new Space `name` listing its repository.
    fn in_space(f: &mut Fixture, name: &str) -> SpaceId {
        let space = space_listing_app(f, name);
        let project = f.coordinator.project_id.clone();
        f.host.move_project_to_space(&project, &space).unwrap();
        space
    }
    fn space(id: &SpaceId) -> Scope {
        Scope::Space {
            space_id: id.clone(),
        }
    }
    fn app(id: &SpaceId) -> Scope {
        Scope::Repository {
            space_id: id.clone(),
            repository: APP.into(),
        }
    }
    fn set(
        f: &mut Fixture,
        scope: Scope,
        behaviors: &[(&str, &str)],
        additions: &str,
    ) -> Result<LayerValues> {
        let behaviors = (behaviors.iter())
            .map(|(id, value)| (id.to_string(), value.to_string()))
            .collect();
        let additions = additions.into();
        f.host.set_layer_values(
            &scope,
            LayerValues {
                behaviors,
                additions,
            },
        )
    }
    fn resolved(f: &Fixture, id: &str) -> ResolvedBehavior {
        let project = &f.coordinator.project_id;
        let settings = f.host.effective_settings(project, Some(&f.ticket)).unwrap();
        settings.behaviors.into_iter().find(|b| b.id == id).unwrap()
    }

    #[test]
    fn an_implementer_gets_its_skill_behaviors_then_each_layers_additions_in_order() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        let skills = f.host.skills();
        skills
            .save_rules(Skill::Implementing, "Workspace rule.")
            .unwrap();
        set(
            &mut f,
            space(&sage),
            &[("wip_commits", "off")],
            "Sage rule.",
        )
        .unwrap();
        set(&mut f, app(&sage), &[], "App rule.").unwrap();
        let implementer = f.host.session_instructions(&f.implementer).unwrap();
        let order = [
            "# Implementing — v3",
            "Commit once the work is ready for testing",
            "## Your own rules for Implementing",
            "Workspace rule.",
            "## Additions from the Sage Space",
            "Sage rule.",
            "## Additions for allowed-org/app in the Sage Space",
            "App rule.",
        ];
        let at: Vec<usize> = (order.iter())
            .map(|part| implementer.find(part).unwrap_or_else(|| panic!("{part}")))
            .collect();
        assert!(at.is_sorted(), "{at:?}");
        assert!(!implementer.contains("commit WIP at each milestone"));
        // A coordinator has no single repository and stops at its Space, whose additions now
        // reach it too.
        let coordinator = f.host.session_instructions(&f.coordinator).unwrap();
        assert!(coordinator.ends_with("Sage rule."), "{coordinator}");
        assert!(!coordinator.contains("App rule."));
    }

    #[test]
    fn at_stock_every_role_is_told_what_it_was_told_before_behaviors() {
        let coordinator = provider::skill(Role::ProjectOrchestrator);
        for said in [
            "Spend review in proportion to risk.",
            "produce a static HTML mockup",
            "fetch origin and report how far main has moved",
            "confirm its status changed",
            "End with the status table only when",
        ] {
            assert!(coordinator.contains(said), "{said}");
        }
        assert!(provider::skill(Role::Implementer).contains("commit WIP at each milestone"));
        assert!(provider::skill(Role::Reviewer).contains("Report at most five non_blocking"));
        let assistant = provider::skill(Role::Assistant);
        assert!(assistant.contains("Answer the question asked, as briefly as it allows"));
        assert!(assistant.contains("single self-contained HTML file"));
    }

    #[test]
    fn a_repository_in_two_spaces_resolves_by_its_projects_space() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        let vyttle = space_listing_app(&mut f, "Vyttle");
        set(&mut f, app(&sage), &[(PUBLISHING, ALWAYS_ASK)], "").unwrap();
        let in_sage = resolved(&f, PUBLISHING);
        assert_eq!(in_sage.resolved.value, ALWAYS_ASK);
        assert_eq!(in_sage.resolved.from, Layer::Repository);
        assert!(f.host.always_asks_before_publishing(&f.ticket()).unwrap());
        let project = f.coordinator.project_id.clone();
        f.host.move_project_to_space(&project, &vyttle).unwrap();
        assert_eq!(resolved(&f, PUBLISHING).resolved.from, Layer::Stock);
        assert!(!f.host.always_asks_before_publishing(&f.ticket()).unwrap());
    }

    #[test]
    fn a_layer_a_behavior_does_not_allow_or_a_repository_the_space_does_not_list_is_refused() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        let refused = set(&mut f, app(&sage), &[("answer_length", "thorough")], "").unwrap_err();
        assert!(
            format!("{refused:#}").contains("cannot be set for a Repository"),
            "{refused:#}"
        );
        let other = Scope::Repository {
            space_id: sage.clone(),
            repository: "allowed-org/other".into(),
        };
        let refused = set(&mut f, other, &[], "x").unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("does not list allowed-org/other")
        );
        assert!(
            f.host
                .layer_values(&app(&sage))
                .unwrap()
                .behaviors
                .is_empty()
        );
    }

    #[test]
    fn a_space_may_not_loosen_a_safety_behavior_and_a_looser_value_read_is_ignored() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        set(&mut f, Scope::Workspace, &[(REVIEW_DEPTH, FULL_REVIEW)], "").unwrap();
        let loosen = [(REVIEW_DEPTH, "proportional")];
        let refused = set(&mut f, space(&sage), &loosen, "").unwrap_err();
        assert!(
            format!("{refused:#}").contains("a Space may only make Review depth stricter"),
            "{refused:#}"
        );
        set(&mut f, space(&sage), &[(PUBLISHING, ALWAYS_ASK)], "").unwrap();
        let refused = set(&mut f, app(&sage), &[(PUBLISHING, "allowed_owners")], "");
        assert!(format!("{:#}", refused.unwrap_err()).contains("a Repository may only make"));
        // Nor through the Space page's save.
        let mut edited = f.host.space(&sage).unwrap().space;
        let looser = (REVIEW_DEPTH.to_owned(), "proportional".to_owned());
        edited.behaviors.extend([looser]);
        let refused = f.host.update_space(edited.clone()).unwrap_err();
        assert!(format!("{refused:#}").contains("a Space may only make Review depth stricter"));
        // A teammate's Mac wrote the looser value; this Mac's stricter Workspace stands, and an
        // edit that leaves it as it was still saves.
        f.host
            .db
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![sage.as_str(), encode(&edited).unwrap()],
            )
            .unwrap();
        edited.description = "Sage Haven work".into();
        f.host.update_space(edited).unwrap();
        let depth = resolved(&f, REVIEW_DEPTH).resolved;
        assert_eq!(
            (depth.value.as_str(), depth.from),
            (FULL_REVIEW, Layer::Workspace)
        );
        assert!(
            depth.notes[0].contains("The Space value is ignored"),
            "{:?}",
            depth.notes
        );
        // The Workspace may loosen what it set itself.
        set(
            &mut f,
            Scope::Workspace,
            &[(REVIEW_DEPTH, "proportional")],
            "",
        )
        .unwrap();
    }

    #[test]
    fn a_looser_repository_value_a_teammate_wrote_is_ignored_with_a_note() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        set(&mut f, space(&sage), &[(PUBLISHING, ALWAYS_ASK)], "").unwrap();
        let mut written = f.host.space(&sage).unwrap().space;
        let looser = RepositorySettings {
            behaviors: [(PUBLISHING.into(), "allowed_owners".into())].into(),
            ..RepositorySettings::default()
        };
        written.repository_settings.insert(APP.into(), looser);
        f.host
            .db
            .execute(
                "UPDATE spaces SET data=?2 WHERE id=?1",
                params![sage.as_str(), encode(&written).unwrap()],
            )
            .unwrap();
        let publishing = resolved(&f, PUBLISHING).resolved;
        assert_eq!(
            (publishing.value.as_str(), publishing.from),
            (ALWAYS_ASK, Layer::Space)
        );
        assert_eq!(publishing.notes.len(), 1);
        assert!(
            publishing.notes[0].starts_with(
                "The Repository value is ignored: a Repository may only make Publishing stricter"
            ),
            "{:?}",
            publishing.notes
        );
        assert!(f.host.always_asks_before_publishing(&f.ticket()).unwrap());
    }

    #[test]
    fn always_ask_first_makes_an_allowed_owners_publish_wait_for_the_human() {
        let mut f = fixture();
        f.allow("allowed-org");
        let sage = in_space(&mut f, "Sage");
        set(&mut f, space(&sage), &[(PUBLISHING, ALWAYS_ASK)], "").unwrap();
        let (status, job) = f.publish(&f.implementer.id.clone()).unwrap();
        assert!(job.is_none());
        assert_eq!(status["status"], "waiting_for_human");
        assert_eq!(status["reason"], ALWAYS_ASK_REASON);
        let items = f.open_items();
        assert_eq!(items.len(), 1);
        assert!(
            items[0].prompt.contains(
                "allowed-org is allowed, but its Publishing Behavior is “Always ask first”"
            ),
            "{}",
            items[0].prompt
        );
        assert!(f.ticket().publishing.unwrap().always_ask);
    }

    #[test]
    fn full_review_for_every_ticket_refuses_a_re_check() {
        let mut f = fixture();
        let mut review = f.host.settings().verification;
        review.levels.push(ReviewLevel {
            name: "Light".into(),
            kind: LevelKind::Recheck,
            verifiers: vec![review.verifiers[0].focus.clone()],
            use_when: "after a small fix.".into(),
        });
        f.host.set_verification(review).unwrap();
        let mut ticket = f.ticket();
        ticket.state = TicketState::ReadyForTesting;
        f.host.save_ticket(&ticket).unwrap();
        set(&mut f, Scope::Workspace, &[(REVIEW_DEPTH, FULL_REVIEW)], "").unwrap();
        let refused = f.host.verify_ticket(&f.ticket, "Light", false).unwrap_err();
        assert!(
            refused.to_string().contains("Light is a re-check, and this ticket's Review depth Behavior is “Full review for every ticket”"),
            "{refused}"
        );
        assert!(f.ticket().running_cycle().is_none());
    }

    #[test]
    fn the_workspace_file_keeps_fields_and_behaviors_this_release_does_not_know() {
        let mut f = fixture();
        let file = f.host.shared_folder().join(shared::BEHAVIORS);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let newer = r#"{"behaviors": {"future_behavior": "sometimes"}, "future_field": 7}"#;
        fs::write(&file, newer).unwrap();
        set(
            &mut f,
            Scope::Workspace,
            &[("status_table", "every_update")],
            "",
        )
        .unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["future_field"], 7);
        assert_eq!(saved["behaviors"]["future_behavior"], "sometimes");
        assert_eq!(saved["behaviors"]["status_table"], "every_update");
        // Sync refuses a choice no Behavior offers, as a save does.
        let bad = br#"{"behaviors": {"status_table": "hourly"}}"#;
        assert!(shared::check(shared::BEHAVIORS, bad).is_err());
        assert!(shared::check(shared::BEHAVIORS, newer.as_bytes()).is_ok());
    }

    #[test]
    fn the_coordinators_context_names_where_each_behavior_came_from() {
        let mut f = fixture();
        let sage = in_space(&mut f, "Sage");
        set(&mut f, app(&sage), &[(REVIEW_DEPTH, FULL_REVIEW)], "").unwrap();
        let context = f.host.agent_context(&f.coordinator.id).unwrap();
        let behaviors = &context["behaviors"];
        let depth = |list: &Value| {
            let list = list.as_array().unwrap();
            list.iter()
                .find(|b| b["id"] == REVIEW_DEPTH)
                .cloned()
                .unwrap()
        };
        assert_eq!(depth(&behaviors["space"])["from"], "stock");
        let app = depth(&behaviors["repositories"][APP]);
        assert_eq!(
            (&app["value"], &app["from"]),
            (&json!(FULL_REVIEW), &json!("repository"))
        );
        assert_eq!(app["overrode"], json!(["stock"]));
    }

    #[test]
    fn every_behavior_has_choices_with_unique_values_and_a_stock_sentence_said_once() {
        let catalog = catalog();
        let mut ids: Vec<&str> = catalog.iter().map(|b| b.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), catalog.len());
        for behavior in &catalog {
            assert!(behavior.choices.len() >= 2, "{}", behavior.name);
            assert!(
                !behavior.layers.contains(&Layer::Stock),
                "{}",
                behavior.name
            );
            let stock = skills::stock(behavior.skill);
            for choice in &behavior.choices {
                let same = behavior.choices.iter().filter(|c| c.value == choice.value);
                assert_eq!(same.count(), 1, "{}", behavior.name);
                assert!(
                    choice.sentence.is_empty() || !stock.contains(&choice.sentence),
                    "{}: {:?} is still in the stock text",
                    behavior.name,
                    choice.label
                );
            }
        }
    }
}
