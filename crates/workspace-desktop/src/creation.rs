use super::bridge::Bridge;
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, IconName, IndexPath, Selectable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    input::{Input, InputState},
    select::{Select, SelectItem, SelectState},
    spinner::Spinner,
};
use std::collections::BTreeSet;
use workspace_core::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Creation {
    Coordinator,
    Ticket,
    Agent,
    Repository,
    Import,
    ProjectRepositories,
}

impl Creation {
    pub fn title(self) -> &'static str {
        match self {
            Self::Coordinator => "Add repository team",
            Self::Ticket => "New ticket",
            Self::Agent => "Assign ticket agent",
            Self::Repository => "Attach repository",
            Self::Import => "Add repositories",
            Self::ProjectRepositories => "Project repositories",
        }
    }
    pub fn action(self) -> &'static str {
        match self {
            Self::Coordinator => "Create team",
            Self::Ticket => "Create ticket",
            Self::Agent => "Assign agent",
            Self::Repository => "Attach repository",
            Self::Import => "Add selected",
            Self::ProjectRepositories => "Save",
        }
    }
}

#[derive(Clone)]
struct Choice {
    id: String,
    label: String,
}
impl SelectItem for Choice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.label.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }
}

pub struct CreationForm {
    kind: Creation,
    project_id: Option<String>,
    project_name: String,
    root_id: Option<String>,
    /// The title, path or folder, depending on the kind.
    name: Entity<InputState>,
    /// The ticket brief or the agent's instruction.
    detail: Entity<InputState>,
    base: Entity<InputState>,
    choices: Entity<SelectState<Vec<Choice>>>,
    options: Vec<Choice>,
    role: Role,
    policies: Vec<RolePolicy>,
    pub pending: bool,
    pub error: String,
    bridge: Bridge,
    discovery: Option<RepositoryDiscovery>,
    selected_repositories: BTreeSet<String>,
    folders: BTreeSet<String>,
    scanning: bool,
    workspace_repositories: Vec<Repository>,
    /// The project's choice: `None` uses every workspace repository.
    chosen_repositories: Option<BTreeSet<String>>,
}

fn default_instruction(role: Role) -> &'static str {
    match role {
        Role::Tester => "Independently verify this ticket against its acceptance criteria.",
        Role::Reviewer => "Review this ticket’s changes and report the most important findings.",
        _ => "Implement this ticket to its acceptance criteria.",
    }
}

impl CreationForm {
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind == Creation::ProjectRepositories {
            return;
        }
        let first = if self.kind == Creation::Agent {
            &self.detail
        } else {
            &self.name
        };
        first.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn new(
        kind: Creation,
        snapshot: &Snapshot,
        selected: Option<&Session>,
        ticket: Option<&str>,
        bridge: Bridge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project_id = selected.map(|s| s.project_id.clone());
        let project = snapshot
            .projects
            .iter()
            .find(|p| Some(&p.id) == project_id.as_ref());
        let project_name = project.map(|p| p.name.clone()).unwrap_or_default();
        let chosen_repositories = project
            .and_then(|p| p.repositories.as_ref())
            .map(|ids| ids.iter().cloned().collect());
        let root_id = snapshot
            .sessions
            .iter()
            .find(|s| {
                Some(&s.project_id) == project_id.as_ref() && s.role == Role::ProjectOrchestrator
            })
            .map(|s| s.id.clone());
        let owner = selected.and_then(|s| {
            if s.role == Role::TaskOrchestrator {
                Some(s.id.as_str())
            } else {
                s.parent_id.as_deref()
            }
        });
        let choices: Vec<Choice> = if kind == Creation::Agent {
            snapshot
                .tickets
                .iter()
                .filter(|t| Some(t.coordinator_id.as_str()) == owner && t.state != "accepted")
                .map(|t| Choice {
                    id: t.id.clone(),
                    label: t.title.clone(),
                })
                .collect()
        } else if kind == Creation::Ticket {
            snapshot
                .sessions
                .iter()
                .filter(|s| {
                    Some(&s.project_id) == project_id.as_ref() && s.role == Role::TaskOrchestrator
                })
                .map(|s| Choice {
                    id: s.id.clone(),
                    label: s.name.clone(),
                })
                .collect()
        } else {
            project_id
                .as_deref()
                .map(|id| snapshot.project_repositories(id))
                .unwrap_or_default()
                .into_iter()
                .map(|r| Choice {
                    id: r.id.clone(),
                    label: r.name.clone(),
                })
                .collect()
        };
        let current = match kind {
            Creation::Agent => ticket,
            Creation::Ticket => owner,
            _ => selected.and_then(|s| s.repository_id.as_deref()),
        };
        let index = choices
            .iter()
            .position(|c| Some(c.id.as_str()) == current)
            .or_else(|| (!choices.is_empty()).then_some(0));
        let options = choices.clone();
        let choices = cx.new(|cx| {
            SelectState::new(choices, index.map(IndexPath::new), window, cx).searchable(true)
        });
        let name = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match kind {
                Creation::Coordinator => "Defaults to the repository name",
                Creation::Ticket => "e.g. Build notification preferences",
                Creation::Repository => "/Users/you/dev/repository",
                Creation::Agent | Creation::Import | Creation::ProjectRepositories => {
                    "e.g. ~/dev/* or ~/dev/company/app"
                }
            })
        });
        let detail = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(4, 10)
                .placeholder(if kind == Creation::Ticket {
                    "What to build, acceptance criteria, dependencies and how to test it"
                } else {
                    "Optional. Leave empty to work the ticket as briefed."
                })
        });
        let base = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("HEAD")
                .placeholder("e.g. origin/main or HEAD")
        });
        Self {
            kind,
            project_id,
            project_name,
            root_id,
            name,
            detail,
            base,
            choices,
            options,
            role: Role::Implementer,
            policies: snapshot.policies.clone(),
            pending: false,
            error: String::new(),
            bridge,
            discovery: None,
            selected_repositories: BTreeSet::new(),
            folders: BTreeSet::new(),
            scanning: false,
            workspace_repositories: snapshot.repositories.clone(),
            chosen_repositories,
        }
    }

    fn default_provider(&self, role: Role) -> Result<Provider, String> {
        self.policies
            .iter()
            .find(|p| p.role == role)
            .map(|p| p.default.provider)
            .ok_or_else(|| "Role policy unavailable".into())
    }

    pub fn command(&self, cx: &App) -> Result<Command, String> {
        if self.kind == Creation::Import {
            if self.scanning {
                return Err("Wait for the folder scan to finish".into());
            }
            if self.selected_repositories.is_empty() {
                return Err("Add a repository or parent folder, then select repositories".into());
            }
            // Unticked repositories stay out when their folder is checked again later.
            let dismissed = self
                .discovery
                .iter()
                .flat_map(|d| &d.repositories)
                .filter(|r| !r.added && !self.selected_repositories.contains(&r.path))
                .map(|r| r.path.clone())
                .collect();
            return Ok(Command::ImportRepositories {
                project_id: self.project_id.clone(),
                paths: self.selected_repositories.iter().cloned().collect(),
                roots: self.folders.iter().cloned().collect(),
                dismissed,
            });
        }
        let name = self.name.read(cx).value().trim().to_string();
        if self.kind == Creation::Repository {
            if name.is_empty() {
                return Err("Enter or choose the repository folder".into());
            }
            let base = self.base.read(cx).value().trim().to_string();
            if base.is_empty() {
                return Err("Enter the repository's base ref".into());
            }
            return Ok(Command::AttachRepository {
                project_id: self.project_id.clone(),
                path: name,
                base,
            });
        }
        let project_id = self.project_id.clone().ok_or("Select a project first")?;
        if self.kind == Creation::ProjectRepositories {
            return Ok(Command::SetProjectRepositories {
                project_id,
                repository_ids: self.chosen_repositories.as_ref().map(|chosen| {
                    // Keep the workspace order rather than the set's.
                    self.workspace_repositories
                        .iter()
                        .filter(|r| chosen.contains(&r.id))
                        .map(|r| r.id.clone())
                        .collect()
                }),
            });
        }
        let selected = self.choices.read(cx).selected_value();
        let choice = self
            .options
            .iter()
            .find(|option| Some(&option.id) == selected)
            .cloned()
            .ok_or(match self.kind {
                Creation::Coordinator => "Attach a repository to this project first",
                Creation::Ticket => "Add a repository team to this project first",
                _ => "Create a ticket in this repository team first",
            })?;
        let detail = self.detail.read(cx).value().trim().to_string();
        match self.kind {
            Creation::Ticket => {
                if name.is_empty() {
                    return Err("Give the ticket a title".into());
                }
                if detail.is_empty() {
                    return Err("Describe the ticket and its acceptance criteria".into());
                }
                Ok(Command::CreateTicket {
                    coordinator_id: choice.id,
                    title: name,
                    brief: detail,
                })
            }
            Creation::Agent => Ok(Command::AssignTicket {
                ticket_id: choice.id,
                role: self.role,
                provider: self.default_provider(self.role)?,
                instruction: if detail.is_empty() {
                    default_instruction(self.role).into()
                } else {
                    detail
                },
            }),
            _ => Ok(Command::CreateSession {
                project_id,
                parent_id: self
                    .root_id
                    .clone()
                    .ok_or("Project coordinator unavailable")?,
                repository_id: Some(choice.id),
                name: if name.is_empty() { choice.label } else { name },
                role: Role::TaskOrchestrator,
                provider: self.default_provider(Role::TaskOrchestrator)?,
            }),
        }
    }

    fn field(label: &'static str, input: impl IntoElement) -> Div {
        div()
            .w_full()
            .min_w_0()
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(label),
            )
            .child(input)
    }

    /// Adds the typed folder and rescans every folder, keeping earlier unticked repositories unticked.
    fn scan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let unticked: BTreeSet<String> = self
            .discovery
            .iter()
            .flat_map(|d| &d.repositories)
            .filter(|r| !r.added && !self.selected_repositories.contains(&r.path))
            .map(|r| r.path.clone())
            .collect();
        self.add_folder(window, cx);
        if self.folders.is_empty() {
            self.error = "Enter a repository or parent folder".into();
            cx.notify();
            return;
        }
        let receiver = match self.bridge.request(Command::DiscoverRepositories {
            folders: self.folders.iter().cloned().collect(),
        }) {
            Ok(receiver) => receiver,
            Err(error) => {
                self.error = error;
                cx.notify();
                return;
            }
        };
        self.scanning = true;
        self.discovery = None;
        self.selected_repositories.clear();
        self.error.clear();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or_else(|_| Err("Host disconnected".into()))
                .and_then(|v| {
                    serde_json::from_value::<RepositoryDiscovery>(v).map_err(|e| e.to_string())
                });
            let _ = this.update(cx, |form, cx| {
                form.scanning = false;
                match result {
                    Ok(discovery) => {
                        form.selected_repositories = discovery
                            .repositories
                            .iter()
                            .filter(|r| !r.added && !unticked.contains(&r.path))
                            .map(|r| r.path.clone())
                            .collect();
                        form.discovery = Some(discovery);
                    }
                    Err(error) => form.error = error,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Enter in the folder field means Add rather than import; true when it started a scan.
    pub fn add_typed_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.kind != Creation::Import || self.name.read(cx).value().trim().is_empty() {
            return false;
        }
        self.scan(window, cx);
        true
    }

    fn add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.name.read(cx).value().trim().to_owned();
        if path.is_empty() {
            return;
        }
        self.folders.insert(path);
        self.name
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Choose repositories or parent folders".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = cx.update(|window, cx| {
                    this.update(cx, |form, cx| {
                        if form.pending || form.scanning {
                            return;
                        }
                        form.folders
                            .extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
                        form.scan(window, cx);
                    })
                });
            }
        })
        .detach();
    }

    fn choose_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the repository folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.first()
            {
                let path = path.to_string_lossy().into_owned();
                let _ = cx.update(|window, cx| {
                    this.update(cx, |form, cx| {
                        form.name
                            .update(cx, |input, cx| input.set_value(path, window, cx));
                    })
                });
            }
        })
        .detach();
    }

    pub fn import_finished(&mut self, result: &RepositoryImport, cx: &mut Context<Self>) {
        self.pending = false;
        for repo in &result.imported {
            self.selected_repositories.remove(&repo.path);
            if let Some(discovery) = &mut self.discovery {
                for candidate in &mut discovery.repositories {
                    if candidate.path == repo.path {
                        candidate.added = true;
                    }
                }
            }
        }
        self.error = result.failures.join("\n");
        cx.notify();
    }

    fn note(text: impl Into<SharedString>, cx: &App) -> Div {
        div()
            .text_size(px(12.))
            .line_height(relative(1.5))
            .text_color(cx.theme().muted_foreground)
            .child(text.into())
    }

    fn role_default_note(&self, role: Role, cx: &App) -> Option<Div> {
        let policy = self.policies.iter().find(|p| p.role == role)?;
        Some(Self::note(
            format!(
                "Starts on {:?} · {} · {}. Change defaults in Models.",
                policy.default.provider, policy.default.model, policy.default.effort
            ),
            cx,
        ))
    }

    fn coordinator_fields(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::field(
                "Repository",
                Select::new(&self.choices)
                    .w_full()
                    .placeholder("Select a repository")
                    .disabled(self.pending),
            ))
            .child(Self::field(
                "Team name",
                Input::new(&self.name).w_full().disabled(self.pending),
            ))
            .child(Self::note(
                "One coordinator plans and accepts every ticket for this repository within the project.",
                cx,
            ))
            .children(self.role_default_note(Role::TaskOrchestrator, cx))
    }

    fn ticket_fields(&self) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::field(
                "Title",
                Input::new(&self.name).w_full().disabled(self.pending),
            ))
            .child(Self::field(
                "Repository team",
                Select::new(&self.choices)
                    .w_full()
                    .placeholder("Select a team")
                    .disabled(self.pending),
            ))
            .child(Self::field(
                "Brief and acceptance criteria",
                Input::new(&self.detail).w_full().disabled(self.pending),
            ))
    }

    fn agent_fields(&self, cx: &mut Context<Self>) -> Div {
        let mut roles = div().flex().flex_wrap().gap_2();
        for role in [Role::Implementer, Role::Tester, Role::Reviewer] {
            let selected = self.role == role;
            roles = roles.child(
                super::button(SharedString::from(format!("create-role-{role:?}")))
                    .label(role.label())
                    .selected(selected)
                    .when(selected, |b| b.primary())
                    .disabled(self.pending)
                    .on_click(cx.listener(move |form, _, _, cx| {
                        form.role = role;
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::field(
                "Ticket",
                Select::new(&self.choices)
                    .w_full()
                    .placeholder("Select a ticket")
                    .disabled(self.pending),
            ))
            .child(Self::field("Role", roles))
            .child(Self::field(
                "Instruction",
                Input::new(&self.detail).w_full().disabled(self.pending),
            ))
            .children(self.role_default_note(self.role, cx))
    }

    fn repository_fields(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::field(
                "Repository folder",
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.name).w_full().disabled(self.pending)),
                    )
                    .child(
                        super::button("choose-repository")
                            .label("Choose…")
                            .disabled(self.pending)
                            .on_click(
                                cx.listener(|form, _, window, cx| {
                                    form.choose_repository(window, cx)
                                }),
                            ),
                    ),
            ))
            .child(Self::field(
                "Base ref",
                Input::new(&self.base).w_full().disabled(self.pending),
            ))
            .child(Self::note(
                "Tickets branch from this ref into their own worktrees. Your checkout is never modified.",
                cx,
            ))
    }

    fn project_repository_fields(&self, cx: &mut Context<Self>) -> Div {
        let uses_all = self.chosen_repositories.is_none();
        let mut body = div().flex().flex_col().gap_4().child(
            Checkbox::new("use-all-repositories")
                .label("Use every workspace repository, including ones added later")
                .checked(uses_all)
                .disabled(self.pending)
                .on_click(cx.listener(|form, checked: &bool, _, cx| {
                    // Unticking starts from everything, so nothing silently disappears.
                    form.chosen_repositories = (!*checked).then(|| {
                        form.workspace_repositories
                            .iter()
                            .map(|r| r.id.clone())
                            .collect()
                    });
                    cx.notify();
                })),
        );
        if self.workspace_repositories.is_empty() {
            return body.child(Self::note(
                "The workspace has no repositories yet. Use Add repositories to bring some in.",
                cx,
            ));
        }
        let mut list = div().flex().flex_col();
        for repo in &self.workspace_repositories {
            let id = repo.id.clone();
            let chosen = self
                .chosen_repositories
                .as_ref()
                .is_none_or(|chosen| chosen.contains(&repo.id));
            list = list.child(
                div()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Checkbox::new(SharedString::from(format!("use-{id}")))
                            .checked(chosen)
                            .disabled(self.pending || uses_all)
                            .on_click(cx.listener(move |form, checked: &bool, _, cx| {
                                if let Some(chosen) = &mut form.chosen_repositories {
                                    if *checked {
                                        chosen.insert(id.clone());
                                    } else {
                                        chosen.remove(&id);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(repo.name.clone()),
                            )
                            .child(Self::note(repo.path.clone(), cx).text_ellipsis()),
                    )
                    .child(Self::note(format!("Base {}", repo.base), cx)),
            );
        }
        body = body.child(list);
        body
    }

    fn import_fields(&self, cx: &mut Context<Self>) -> Div {
        let busy = self.pending || self.scanning;
        let mut folders = div().flex().flex_wrap().gap_2();
        for path in &self.folders {
            let remove = path.clone();
            folders = folders.child(
                div()
                    .max_w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_size(px(12.))
                    .child(div().min_w_0().text_ellipsis().child(path.clone()))
                    .child(
                        super::button(SharedString::from(format!("remove-folder-{path}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip("Remove folder")
                            .disabled(busy)
                            .on_click(cx.listener(move |form, _, window, cx| {
                                form.folders.remove(&remove);
                                if form.folders.is_empty() {
                                    form.discovery = None;
                                    form.selected_repositories.clear();
                                    cx.notify();
                                } else {
                                    form.scan(window, cx);
                                }
                            })),
                    ),
            );
        }
        let mut body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Self::field(
                "Repositories or parent folders",
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.name).w_full().disabled(busy)),
                    )
                    .child(
                        super::button("add-folder")
                            .label("Add")
                            .loading(self.scanning)
                            .disabled(busy)
                            .on_click(cx.listener(|form, _, window, cx| form.scan(window, cx))),
                    )
                    .child(
                        super::button("browse-folders")
                            .label("Choose…")
                            .disabled(busy)
                            .on_click(cx.listener(|form, _, window, cx| form.browse(window, cx))),
                    ),
            ))
            .when(!self.folders.is_empty(), |d| d.child(folders))
            .child(Self::note(
                "A repository is added as is. Any other folder is scanned one level deep and checked again for new repositories when you come back to Wiffletree, so add each level you keep repositories in, such as ~/dev/* and ~/dev/company/*. Paths may start with ~/.",
                cx,
            ));
        let Some(discovery) = &self.discovery else {
            return body;
        };
        if discovery.repositories.is_empty() {
            body = body.child(Self::note(
                "No committed Git repositories found in these folders.",
                cx,
            ));
        } else {
            let available = discovery.repositories.iter().filter(|r| !r.added).count();
            let all = self.selected_repositories.len() == available && available > 0;
            body = body.child(
                div().flex().items_center().justify_between().child(
                    Checkbox::new("select-all-repos")
                        .label(format!(
                            "{} of {available} selected",
                            self.selected_repositories.len()
                        ))
                        .checked(all)
                        .disabled(self.pending || available == 0)
                        .on_click(cx.listener(|form, checked: &bool, _, cx| {
                            form.selected_repositories = form
                                .discovery
                                .iter()
                                .flat_map(|d| &d.repositories)
                                .filter(|r| *checked && !r.added)
                                .map(|r| r.path.clone())
                                .collect();
                            cx.notify();
                        })),
                ),
            );
        }
        let mut list = div().flex().flex_col();
        for repo in &discovery.repositories {
            let path = repo.path.clone();
            let selected = self.selected_repositories.contains(&path);
            list = list.child(
                div()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Checkbox::new(SharedString::from(format!("repo-{path}")))
                            .checked(selected || repo.added)
                            .disabled(self.pending || repo.added)
                            .on_click(cx.listener(move |form, checked: &bool, _, cx| {
                                if *checked {
                                    form.selected_repositories.insert(path.clone());
                                } else {
                                    form.selected_repositories.remove(&path);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(repo.name.clone()),
                            )
                            .child(Self::note(repo.path.clone(), cx).text_ellipsis()),
                    )
                    .child(Self::note(
                        if repo.added {
                            "In the workspace".to_owned()
                        } else {
                            format!("Base {}", repo.base)
                        },
                        cx,
                    )),
            );
        }
        body = body.child(list);
        for warning in &discovery.warnings {
            body = body.child(Self::note(warning.clone(), cx).text_color(cx.theme().warning));
        }
        body
    }
}

impl Render for CreationForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = match self.kind {
            Creation::Coordinator => self.coordinator_fields(cx),
            Creation::Ticket => self.ticket_fields(),
            Creation::Agent => self.agent_fields(cx),
            Creation::Repository => self.repository_fields(cx),
            Creation::Import => self.import_fields(cx),
            Creation::ProjectRepositories => self.project_repository_fields(cx),
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .text_size(px(13.))
            .child(Self::note(
                if self.project_id.is_some() {
                    format!("In {}", self.project_name)
                } else {
                    "For the whole workspace. Projects that use every repository pick these up."
                        .into()
                },
                cx,
            ))
            .child(fields)
            .when(!self.error.is_empty(), |d| {
                d.child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(cx.theme().danger.opacity(0.08))
                        .text_color(cx.theme().danger)
                        .child(self.error.clone()),
                )
            })
            .when(self.pending, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child(Self::note("Saving…", cx)),
                )
            })
    }
}
