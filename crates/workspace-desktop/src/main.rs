mod activity;
mod assets;
mod attachment_chips;
mod automation;
mod bridge;
mod context_view;
mod conversation;
mod conversation_header;
mod creation;
mod crew_panel;
mod data_dir;
mod events_view;
mod file_links;
mod files_view;
mod history_sheet;
mod inbox;
mod inspector;
mod leftovers;
mod login_env;
mod memory_view;
mod models;
mod models_view;
mod palette;
mod preferences;
mod processes_view;
mod progress;
mod project_name;
mod quotas;
mod render_counts;
mod repositories_view;
mod settings;
mod shared;
mod sidebar;
mod skills_view;
mod team_view;
#[cfg(test)]
mod test_support;
mod transcript;
mod tree;
mod ui;
mod update;
mod usage_view;
mod verifiers;
mod welcome;
mod wording;
mod workspace_notes;
use bridge::Bridge;
use creation::{Creation, CreationForm};
use gpui::{prelude::*, *};
use gpui_component::{
    Root, Sizable, Theme, ThemeMode, TitleBar, WindowExt,
    button::Button,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    notification::Notification,
    scroll::ScrollableElement,
};
use palette::{Palette, palette};
use preferences::{Appearance, Preferences};
use serde_json::Value;
use shared::{Amendment, HumanWrote, Intent, Requests, Shared, toast, toast_error};
use ui::{button, edge, font};
// GPUI has an `Inspector` too; ours wins over its glob.
use inspector::Inspector;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    rc::Rc,
};
// GPUI exports an accessibility `Role` too; ours wins over both globs.
use workspace_core::Role;
use workspace_core::ask::Ask;
use workspace_core::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Overview,
    Team,
    Memory,
    Events,
    Attention,
    Git,
    Files,
    Processes,
    /// Opened from the coordinator's progress strip, not from the rail. It lists the whole
    /// project, so it stays open whichever of the project's agents is selected.
    Crew,
}

/// What the open Memory, Events or Git panel loaded, decoded once when its reply arrives.
enum PanelData {
    Loading,
    Memory(Vec<WorkLog>),
    Events(Vec<Activity>),
    Git(Vec<Commit>),
}

/// A request sent again once, rather than once per ask, when asked for while it is answered.
#[derive(Default)]
struct Coalesced {
    sent: bool,
    again: bool,
}
impl Coalesced {
    /// Whether to send the request now; otherwise it goes again once the one sent is answered.
    fn send(&mut self) -> bool {
        if self.sent {
            self.again = true;
            return false;
        }
        self.sent = true;
        true
    }
    /// The request was answered; whether to send it again.
    fn answered(&mut self) -> bool {
        self.sent = false;
        std::mem::take(&mut self.again)
    }
}

/// What is shown, as far as reading the archive goes.
struct ArchiveDemand<'a> {
    show_archived: bool,
    panel: Option<Panel>,
    page: Page,
    selected: Option<&'a str>,
    messages: &'a [Message],
}

/// The archive is out of date and something shown needs it: Show archived, the Tickets, Crew or
/// Events panel, the Repositories page or a publish request, or a selected session or message sender
/// that the snapshot does not list, which only the archive holds.
fn wants_archive(index: &WorkspaceIndex, shown: &ArchiveDemand) -> bool {
    let snapshot = index.snapshot();
    let live: BTreeSet<&str> = snapshot.sessions.iter().map(|s| s.id.as_str()).collect();
    let archived = |id: &str| !live.contains(id);
    index.archive_is_stale()
        && (shown.show_archived
            || matches!(shown.panel, Some(Panel::Team | Panel::Crew | Panel::Events))
            || shown.page == Page::Repositories
            || snapshot.attention.iter().any(Attention::is_publish)
            || shown.selected.is_some_and(archived)
            || shown
                .messages
                .iter()
                .any(|m| m.sender.as_deref().is_some_and(archived)))
}

/// What fills the main area: one session's conversation, or a page about the whole workspace.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Conversation,
    Usage,
    Models,
    Skills,
    Repositories,
}

struct Workspace {
    bridge: Bridge,
    shared: Shared,
    /// The snapshot request sent and not yet answered, so that a burst of changes asks for one
    /// more, not one per change.
    snapshot_request: Coalesced,
    quota_request: Coalesced,
    /// An archive request is on its way.
    archive_pending: bool,
    load_error: Option<String>,
    models: Entity<models_view::ModelsPage>,
    skills: Entity<skills_view::SkillsPage>,
    conversation: Entity<conversation::Conversation>,
    creation: Option<Entity<CreationForm>>,
    repositories: Entity<repositories_view::RepositoriesPage>,
    usage: Entity<usage_view::UsagePage>,
    sidebar: Entity<sidebar::Sidebar>,
    inspector: Entity<Inspector>,
    inbox: Entity<inbox::Inbox>,
    contexts: Entity<context_view::Contexts>,
    last_repository_refresh: Option<std::time::Instant>,
    progress_fetch: activity::StepFetch,
}
impl Workspace {
    fn new(bridge: Bridge, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let verifiers = cx.new(|cx| verifiers::VerifierEditor::new(bridge.clone(), window, cx));
        let models = cx
            .new(|cx| models_view::ModelsPage::new(bridge.clone(), verifiers.clone(), window, cx));
        Self::adopt(&models, window, cx);
        // The start screen draws what the Models page detected.
        cx.observe(&models, |_, _, cx| cx.notify()).detach();
        let skills = cx
            .new(|cx| skills_view::SkillsPage::new(bridge.clone(), verifiers.clone(), window, cx));
        Self::adopt(&skills, window, cx);
        let inbox = cx.new(|cx| inbox::Inbox::new(bridge.clone(), window, cx));
        let contexts = cx.new(|_| context_view::Contexts::new(bridge.clone()));
        let conversation = cx.new(|cx| {
            conversation::Conversation::new(
                bridge.clone(),
                inbox.clone(),
                contexts.clone(),
                window,
                cx,
            )
        });
        Self::adopt(&conversation, window, cx);
        let inspector = cx.new(|cx| {
            Inspector::new(
                bridge.clone(),
                inbox.clone(),
                contexts.clone(),
                skills.clone(),
                window,
                cx,
            )
        });
        Self::adopt(&inspector, window, cx);
        let repositories = cx.new(|cx| {
            repositories_view::RepositoriesPage::new(bridge.clone(), verifiers.clone(), window, cx)
        });
        Self::adopt(&repositories, window, cx);
        let sidebar = cx.new(|cx| sidebar::Sidebar::new(bridge.clone(), window, cx));
        Self::adopt(&sidebar, window, cx);
        let usage = cx.new(|_| usage_view::UsagePage::new(bridge.clone()));
        Self::adopt(&usage, window, cx);
        let mut view = Self {
            bridge,
            shared: Shared::default(),
            snapshot_request: Coalesced::default(),
            quota_request: Coalesced::default(),
            archive_pending: false,
            load_error: None,
            models,
            skills,
            conversation,
            creation: None,
            repositories,
            usage,
            sidebar,
            inspector,
            inbox,
            contexts,
            last_repository_refresh: None,
            progress_fetch: Default::default(),
        };
        if let Some(changes) = view.bridge.changes() {
            cx.spawn_in(window, async move |this, cx| {
                while changes.recv().await.is_ok() {
                    if cx
                        .update(|window, cx| {
                            this.update(cx, |view, cx| {
                                view.refresh(window, cx);
                            })
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        view.watch_steps(window, cx);
        view.watch_progress(window, cx);
        view.watch_usage(window, cx);
        view.refresh(window, cx);
        view.refresh_quota(window, cx);
        view.refresh_repositories(false, window, cx);
        // New clones show up when you come back to the app; there is no file watcher.
        cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.refresh_repositories(false, window, cx);
            }
        })
        .detach();
        // Each minute's redraw of the whole window also keeps the times shown in minutes
        // current in an idle window, such as the PR keeper's countdown and age on PR lines.
        cx.spawn_in(window, async move |this, cx| {
            let mut minute = 0u8;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(60))
                    .await;
                minute = (minute + 1) % 5;
                if cx
                    .update(|window, cx| {
                        this.update(cx, |view, cx| {
                            if minute == 0 {
                                view.refresh_quota(window, cx);
                            } else {
                                view.read_quotas(window, cx);
                            }
                            window.refresh();
                        })
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }
    /// Routes a view's intents to the shell.
    fn adopt<V: EventEmitter<Intent>>(
        view: &Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(view, window, |shell, _, intent, window, cx| {
            shell.handle(intent, window, cx)
        })
        .detach();
    }
    fn handle(&mut self, intent: &Intent, window: &mut Window, cx: &mut Context<Self>) {
        match intent {
            Intent::Select(id) => self.select(id.clone(), window, cx),
            Intent::OpenPage(page) => self.open_page(*page, window, cx),
            Intent::ShowPanel(panel) => self.show_panel(*panel, window, cx),
            Intent::TogglePanel(panel) => self.toggle_panel(*panel, window, cx),
            Intent::ClosePanel => self.shared.panel = None,
            Intent::ShowArchived(shown) => {
                self.shared.show_archived = *shown;
                self.sync_archive(window, cx);
            }
            // Only the archive may know who sent them.
            Intent::MessagesShown => return self.sync_archive(window, cx),
            Intent::RefreshQuotas => self.refresh_quota(window, cx),
            Intent::Archived { archived, affected } => {
                self.archived(*archived, affected, window, cx)
            }
            Intent::OpenSkill(skill) => {
                self.open_page(Page::Skills, window, cx);
                self.skills.update(cx, |skills, cx| {
                    skills.open_skill(skills_view::SkillView::Skill(*skill), window, cx)
                });
            }
            Intent::Catalog(catalog) => self.shared.catalog = catalog.clone(),
            Intent::Amend(amendment) => self.amend(amendment),
            Intent::Create { kind, ticket } => {
                self.open_creation(*kind, ticket.as_deref(), window, cx)
            }
        }
        self.share(window, cx);
        cx.notify();
    }
    /// Hands every view the shared state as it is now.
    fn share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        self.usage
            .update(cx, |usage, cx| usage.share(shared.clone(), window, cx));
        self.repositories.update(cx, |repositories, cx| {
            repositories.share(shared.clone(), cx)
        });
        self.models
            .update(cx, |models, cx| models.share(shared.clone(), window, cx));
        self.skills
            .update(cx, |skills, cx| skills.share(shared.clone(), window, cx));
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.share(shared.clone(), window, cx));
        self.inbox
            .update(cx, |inbox, cx| inbox.share(shared.clone(), window, cx));
        self.contexts.update(cx, |contexts, cx| {
            contexts.share(shared.clone(), window, cx)
        });
        self.inspector.update(cx, |inspector, cx| {
            inspector.share(shared.clone(), window, cx)
        });
        self.conversation.update(cx, |conversation, cx| {
            conversation.share(shared, window, cx)
        });
    }
    /// Says what was archived or restored, and steps out of a selection that just went out of
    /// sight to its owner, or to any session still listed.
    fn archived(
        &mut self,
        archived: bool,
        affected: &[Session],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = affected.first().map_or("Session", |s| s.name.as_str());
        toast(
            if archived {
                format!("Archived {name}. Nothing was deleted.")
            } else {
                format!("Restored {name}")
            },
            window,
            cx,
        );
        let hidden = archived
            && !self.shared.show_archived
            && affected
                .iter()
                .any(|s| Some(&s.id) == self.shared.selected.as_ref());
        if hidden {
            let owner = affected[0].parent_id.clone().or_else(|| {
                (self.shared.index.iter())
                    .flat_map(|index| index.sessions())
                    .find(|s| !s.archived && affected.iter().all(|a| a.id != s.id))
                    .map(|s| s.id.clone())
            });
            match owner {
                Some(id) => self.select(id, window, cx),
                None => self.shared.selected = None,
            }
        }
    }
    /// Starts a project, or opens Models where a first setup is saved.
    fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.new_project(window, cx));
    }
    fn amend(&mut self, amendment: &Amendment) {
        self.amend_snapshot(|snapshot| match amendment {
            Amendment::Project(project) => {
                if let Some(saved) = snapshot.projects.iter_mut().find(|p| p.id == project.id) {
                    *saved = project.clone();
                }
                for session in &mut snapshot.sessions {
                    if session.project_id == project.id && session.role == Role::ProjectOrchestrator
                    {
                        session.name = project.name.clone();
                    }
                }
            }
            Amendment::Projects(projects) => snapshot.projects = projects.clone(),
            Amendment::Skill(saved) => {
                if let Some(state) =
                    (snapshot.skills.iter_mut()).find(|state| state.skill == saved.skill)
                {
                    *state = (**saved).clone();
                }
            }
        });
    }
    /// Shows a failure where the user is looking: in its form when one owns it, otherwise as a toast.
    fn fail(
        &mut self,
        command: &Command,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shared.index.is_none() {
            self.load_error = Some(error);
        } else if !self.form_failed(command, &error, cx) {
            toast_error(error, window, cx);
        }
        cx.notify();
    }
    /// Checks the root folders for new repositories, at most once a minute unless `now`.
    fn refresh_repositories(&mut self, now: bool, window: &mut Window, cx: &mut Context<Self>) {
        let recent = self
            .last_repository_refresh
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60));
        if now || !recent {
            self.last_repository_refresh = Some(std::time::Instant::now());
            let ask = ask::refresh_repositories();
            self.ask(ask, window, cx, |view, refresh, window, cx| {
                // A background check; an unreadable root is not worth interrupting anyone for.
                match refresh {
                    Ok(refresh) if !refresh.added.is_empty() => {
                        let names: Vec<_> = refresh.added.iter().map(|r| r.name.as_str()).collect();
                        let found = format!("Found new repositories: {}", names.join(", "));
                        toast(found, window, cx);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("Repository refresh: {error}"),
                }
                // Also picks up folders that went missing since the last snapshot.
                view.refresh(window, cx);
            });
        }
    }
    /// Step changes go to the transcript, for its activity card, and to the Files panel.
    fn watch_steps(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changes) = self.bridge.step_changes() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while changes.recv().await.is_ok() {
                let fetched = cx.update(|window, cx| {
                    this.update(cx, |view, cx| {
                        let transcript = view.conversation.read(cx).transcript.clone();
                        transcript.update(cx, |transcript, cx| transcript.fetch_steps(window, cx));
                        view.inspector
                            .update(cx, |inspector, cx| inspector.fetch_files(window, cx));
                    })
                });
                if !matches!(fetched, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }
    /// Reads the stored usage again whenever a provider reports some. A new reading redraws
    /// the quota strip, and the Usage page reloads its report with it.
    fn watch_usage(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changes) = self.bridge.usage_changes() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while changes.recv().await.is_ok() {
                let read = cx
                    .update(|window, cx| this.update(cx, |view, cx| view.read_quotas(window, cx)));
                if !matches!(read, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }
    /// Asks for the stored quota readings, or, while a read is on its way, for one more after it.
    fn read_quotas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quota_request.send() {
            self.ask(ask::quotas(), window, cx, |view, quotas, window, cx| {
                if view.quota_request.answered() {
                    view.read_quotas(window, cx);
                }
                view.quotas_read(Command::Quotas, quotas, window, cx);
            });
        }
    }
    /// Reads Codex's account limits again, one reading at a time.
    fn refresh_quota(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_quotas(window, cx);
        if self.shared.quota_pending {
            return;
        }
        self.shared.quota_pending = true;
        self.ask(
            ask::refresh_codex_quota(),
            window,
            cx,
            |view, quotas, window, cx| {
                view.shared.quota_pending = false;
                view.quotas_read(Command::RefreshCodexQuota, quotas, window, cx);
            },
        );
    }
    fn quotas_read(
        &mut self,
        command: Command,
        quotas: Result<Vec<QuotaReading>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match quotas {
            // The same readings again would only redraw what draws them.
            Ok(quotas) if *self.shared.quotas != quotas => self.shared.quotas = Rc::new(quotas),
            Ok(_) => {}
            Err(error) => self.fail(&command, error, window, cx),
        }
        self.share(window, cx);
    }
    /// Handles a reply to a creation form's command.
    fn accept_creation(
        &mut self,
        command: Command,
        result: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = match result {
            Ok(v) => v,
            Err(e) => return self.fail(&command, e, window, cx),
        };
        if matches!(command, Command::ImportRepositories { .. }) {
            match serde_json::from_value::<RepositoryImport>(value) {
                Ok(result) => {
                    if !result.imported.is_empty() {
                        toast(
                            format!("Added {} repositories", result.imported.len()),
                            window,
                            cx,
                        );
                    }
                    if let Some(form) = &self.creation {
                        form.update(cx, |form, cx| form.import_finished(&result, cx));
                    }
                    if result.failures.is_empty() {
                        self.creation = None;
                        window.close_dialog(cx);
                    }
                }
                Err(error) => self.fail(&command, error.to_string(), window, cx),
            }
            return;
        }
        if self.creation.take().is_some() {
            window.close_dialog(cx);
        }
        match command {
            Command::CreateSession { .. } | Command::AssignTicket { .. } => {
                if let Ok(session) = serde_json::from_value::<Session>(value) {
                    let id = session.id.clone();
                    self.amend_snapshot(|snapshot| snapshot.sessions.push(session));
                    self.select(id, window, cx);
                }
            }
            Command::CreateTicket { coordinator_id, .. } => {
                self.select(coordinator_id, window, cx);
                self.shared.panel = Some(Panel::Team);
            }
            _ => {}
        }
    }
    /// Asks for the latest snapshot, or, while one is on its way, for one more after it.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot_request.send() {
            self.ask(ask::snapshot(), window, cx, |view, snapshot, window, cx| {
                if view.snapshot_request.answered() {
                    view.refresh(window, cx);
                }
                match snapshot {
                    Ok(snapshot) => view.accept_snapshot(snapshot, window, cx),
                    Err(error) => view.fail(&Command::Snapshot, error, window, cx),
                }
            });
        }
    }
    /// Reads the archive again when what is shown needs it and the one held is out of date.
    fn sync_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let transcript = self.conversation.read(cx).transcript.read(cx);
        let wanted = self.shared.index.as_ref().is_some_and(|index| {
            let shown = ArchiveDemand {
                show_archived: self.shared.show_archived,
                panel: self.shared.panel,
                page: self.shared.page,
                selected: self.shared.selected.as_deref(),
                messages: transcript.messages(),
            };
            wants_archive(index, &shown)
        });
        if wanted && !self.archive_pending {
            self.archive_pending = true;
            self.ask(ask::archive(), window, cx, |view, archive, window, cx| {
                view.archive_pending = false;
                match archive {
                    Ok(archive) => view.accept_archive(archive, window, cx),
                    Err(error) => view.fail(&Command::Archive, error, window, cx),
                }
            });
        }
    }
    fn accept_archive(&mut self, archive: Archive, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = &self.shared.index else {
            return;
        };
        let snapshot = index.snapshot().clone();
        self.shared.index = Some(Rc::new(WorkspaceIndex::new(snapshot, Rc::new(archive))));
        // A selection the archive does not hold either is gone.
        if self.shared.selected_session().is_none() {
            let first = (self.shared.index.as_ref())
                .and_then(|index| index.sessions().find(|s| !s.archived).map(|s| s.id.clone()));
            match first {
                Some(id) => self.select(id, window, cx),
                None => self.shared.selected = None,
            }
        }
        self.share(window, cx);
        // A snapshot that arrived meanwhile may have moved the stamp again.
        self.sync_archive(window, cx);
    }
    /// Shows a change the host confirmed before the snapshot that has it arrives.
    fn amend_snapshot(&mut self, change: impl FnOnce(&mut Snapshot)) {
        if let Some(index) = &self.shared.index {
            let mut snapshot = index.snapshot().clone();
            change(&mut snapshot);
            let archive = index.archive().clone();
            self.shared.index = Some(Rc::new(WorkspaceIndex::new(snapshot, archive)));
        }
    }
    fn accept_snapshot(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let index = WorkspaceIndex::new(snapshot, self.shared.archive());
        // A selected session the held archive is out of date for may be in the next one, which
        // `accept_archive` checks.
        let gone = |id: &String| index.session(id).is_none() && !index.archive_is_stale();
        if self.shared.selected.as_ref().is_none_or(gone) {
            self.shared.selected = index.sessions().find(|s| !s.archived).map(|s| s.id.clone());
        }
        settings::share_sync(index.snapshot(), cx);
        self.shared.index = Some(Rc::new(index));
        self.load_error = None;
        self.share(window, cx);
        self.sync_archive(window, cx);
    }
    fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.shared.selected = Some(id);
        self.shared.selections += 1;
        self.shared.page = Page::Conversation;
        if let Some(role) = self.shared.selected_session().map(|s| s.role) {
            self.shared.panel = self.shared.panel.map(|panel| panel.kept_for(role));
        }
        self.share(window, cx);
        self.sync_archive(window, cx);
        cx.notify();
    }
    fn toggle_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.shared.panel != Some(panel);
        self.shared.panel = open.then_some(panel);
        self.share(window, cx);
        self.sync_archive(window, cx);
        cx.notify();
    }
    fn show_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        self.shared.panel = Some(panel);
        self.share(window, cx);
        self.sync_archive(window, cx);
        cx.notify();
    }
    /// Opens a workspace page in place of the conversation.
    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.shared.page = page;
        match page {
            Page::Usage => self.read_quotas(window, cx),
            Page::Repositories => self.refresh_repositories(true, window, cx),
            Page::Skills => self.skills.update(cx, |skills, cx| {
                skills.open_skill(skills_view::SkillView::List, window, cx)
            }),
            _ => {}
        }
        self.share(window, cx);
        self.sync_archive(window, cx);
        cx.notify();
    }
    /// Returns true when an open form displays the failure itself.
    fn form_failed(&mut self, command: &Command, error: &str, cx: &mut Context<Self>) -> bool {
        if matches!(
            command,
            Command::CreateSession { .. }
                | Command::CreateTicket { .. }
                | Command::AssignTicket { .. }
                | Command::AttachRepository { .. }
                | Command::ImportRepositories { .. }
                | Command::SetProjectRepositories { .. }
        ) && let Some(form) = &self.creation
        {
            form.update(cx, |form, cx| {
                form.pending = false;
                form.error = error.into();
                cx.notify();
            });
            return true;
        }
        false
    }
    /// Opens a creation dialog. `ticket` preselects the ticket when assigning an agent.
    fn open_creation(
        &mut self,
        kind: Creation,
        ticket: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = &self.shared.index else {
            return;
        };
        let project = self.shared.project_id();
        let has_repository = project
            .as_ref()
            .is_some_and(|id| !index.snapshot().project_repositories(id).is_empty());
        // A ticket needs a repository, so start with the step that is actually missing.
        let kind = if kind == Creation::Ticket && !has_repository {
            window.push_notification(
                Notification::info("Add a repository first. Each ticket works in one."),
                cx,
            );
            Creation::Import
        } else {
            kind
        };
        // The workspace Repositories page adds to the workspace, not to the selected project.
        let context = (self.shared.page != Page::Repositories)
            .then(|| self.shared.selected_session())
            .flatten();
        let form = cx.new(|cx| {
            CreationForm::new(
                kind,
                index,
                context,
                ticket,
                self.bridge.clone(),
                window,
                cx,
            )
            .with_catalog(&self.shared.catalog)
        });
        self.creation = Some(form.clone());
        let focused_form = form.clone();
        let weak = cx.weak_entity();
        let cancel = weak.clone();
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let weak = weak.clone();
            let cancel = cancel.clone();
            dialog
                .title(kind.title())
                .width(px(560.).min(window.viewport_size().width - px(48.)))
                .max_h(window.viewport_size().height - px(100.))
                .confirm()
                .ok_text(kind.action())
                .child(form.clone())
                .on_ok(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.submit_creation(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel
                        .update(cx, |view, cx| {
                            if view.creation.as_ref().is_some_and(|f| f.read(cx).pending) {
                                return false;
                            }
                            view.creation = None;
                            true
                        })
                        .unwrap_or(true)
                })
        });
        focused_form.update(cx, |form, cx| form.focus(window, cx));
    }
    fn submit_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.creation.clone() else {
            return;
        };
        if form.read(cx).pending || form.update(cx, |form, cx| form.add_typed_folder(window, cx)) {
            return;
        }
        match form.read(cx).command(cx) {
            Ok(command) => {
                form.update(cx, |form, cx| {
                    form.pending = true;
                    form.error.clear();
                    cx.notify();
                });
                self.request(command, window, cx);
            }
            Err(error) => form.update(cx, |form, cx| {
                form.error = error;
                cx.notify();
            }),
        }
    }
    fn colors(&self, cx: &App) -> Palette {
        palette(is_dark(cx))
    }
}

impl Requests for Workspace {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.accept_creation(command, reply, window, cx);
        self.share(window, cx);
    }
}

/// Whether this client draws dark, following macOS when the choice is System.
fn is_dark(cx: &App) -> bool {
    match cx.global::<Preferences>().appearance {
        Appearance::Light => false,
        Appearance::Dark => true,
        Appearance::System => matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

/// Applies to every open window and is remembered for the next launch.
fn set_appearance(appearance: Appearance, cx: &mut App) {
    let preferences = cx.global_mut::<Preferences>();
    preferences.appearance = appearance;
    if let Err(error) = preferences.save() {
        eprintln!("Saving preferences: {error}");
    }
    apply_theme(cx);
}

/// A view choice of this client, remembered for the next launch; the host's order is untouched.
fn set_group_projects(grouped: bool, cx: &mut App) {
    let preferences = cx.global_mut::<Preferences>();
    preferences.group_projects = grouped;
    if let Err(error) = preferences.save() {
        eprintln!("Saving preferences: {error}");
    }
}

fn apply_theme(cx: &mut App) {
    let dark = is_dark(cx);
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let p = palette(dark);
    // `update` carries the edits into the component tokens and the Markdown style.
    Theme::update(cx, |theme| {
        theme.font_family = ".SystemUIFont".into();
        theme.font_size = px(14.);
        theme.colors.background = p.base;
        theme.colors.foreground = p.text;
        theme.colors.border = p.edge.opacity(edge::HALF);
        theme.colors.popover = p.surface;
        theme.colors.popover_foreground = p.text;
        theme.colors.list_active = p.overlay;
        theme.colors.list_hover = p.overlay;
        theme.colors.list_active_border = p.focus;
        theme.colors.secondary_hover = p.overlay;
        theme.colors.secondary_active = p.overlay;
        theme.colors.drag_border = p.focus;
        theme.colors.primary = p.accent;
        theme.colors.primary_foreground = p.on_accent;
        theme.colors.button_primary = p.accent;
        theme.colors.button_primary_foreground = p.on_accent;
        theme.colors.secondary = p.overlay;
        theme.colors.secondary_foreground = p.text;
        // Plain buttons, Secondary before gpui-component 0.6, keep the same grey.
        theme.colors.button = p.overlay;
        theme.colors.button_hover = p.overlay;
        theme.colors.button_active = p.overlay;
        theme.colors.button_foreground = p.text;
        // Menu and select highlights, and the background of inline code in rendered Markdown.
        // A tint of the text so it shows on the base, panels and message bubbles alike.
        theme.colors.accent = p.text.opacity(0.12);
        theme.colors.accent_foreground = p.text;
        theme.colors.link = p.blue;
        theme.colors.table_head_foreground = p.text;
        theme.colors.selection = p.focus.opacity(0.3);
        theme.colors.muted = p.surface;
        theme.colors.muted_foreground = p.subtle;
        theme.colors.input = p.edge;
        theme.colors.ring = p.focus;
        theme.colors.title_bar = p.surface;
        theme.colors.title_bar_border = p.edge.opacity(edge::LIGHT);
        theme.colors.danger = p.red;
        theme.colors.success = p.green;
        theme.colors.warning = p.yellow;
        theme.colors.info = p.focus;
    });
}

actions!(
    workspace,
    [
        Quit,
        SendMessage,
        NewProject,
        About,
        OpenSettings,
        CheckForUpdates,
        OpenModels,
        OpenRepositories
    ]
);

/// The release version, or the crate version marked as a local build.
fn version_label() -> String {
    match update::VERSION {
        Some(version) => format!("v{version}"),
        None => format!("v{} dev", env!("CARGO_PKG_VERSION")),
    }
}

/// Opens the standard macOS About panel, which reads the bundle's name, version and copyright.
// objc's macros test a `cargo-clippy` feature this crate does not declare.
#[allow(unexpected_cfgs)]
fn show_about() {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    // SAFETY: called on the main thread; NSApp is the shared application, and a nil options
    // dictionary asks for the bundle's defaults.
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, orderFrontStandardAboutPanel: std::ptr::null_mut::<Object>()];
    }
}

fn main() -> anyhow::Result<()> {
    // SAFETY: nothing else has started yet, so no other thread can read the environment.
    unsafe { login_env::adopt() };
    let mut args = std::env::args().skip(1);
    let mut home = None;
    let mut repository = None;
    let mut automation = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                home = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("Missing data directory"))?,
                ))
            }
            "--demo-repository" => {
                repository = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("Missing demo repository"))?,
                )
            }
            "--automation-socket" => {
                automation =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("Missing automation socket path")
                    })?))
            }
            _ => anyhow::bail!(
                "Usage: workspace-desktop --data-dir <directory> [--demo-repository <repository-root>] [--automation-socket <path>]"
            ),
        }
    }
    let home = home.unwrap_or_else(|| {
        data_dir::default_directory(&PathBuf::from(
            std::env::var_os("HOME").expect("macOS home directory"),
        ))
    });
    let preferences = Preferences::load(&home);
    let bridge = Bridge::start(home, repository);
    let steps = automation.as_deref().map(automation::listen).transpose()?;
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.set_global(preferences);
            apply_theme(cx);
            // Under the System appearance, macOS can switch modes while the app runs. The component
            // theme only changes when told, so every window re-applies it or keeps stale colours.
            cx.observe_new(|_: &mut Root, window, _| {
                if let Some(window) = window {
                    window
                        .observe_window_appearance(|_, cx| apply_theme(cx))
                        .detach();
                }
            })
            .detach();
            cx.bind_keys([
                KeyBinding::new("enter", SendMessage, Some("ChatComposer > Input")),
                KeyBinding::new("cmd-enter", SendMessage, Some("ChatComposer > Input")),
                KeyBinding::new("cmd-n", NewProject, None),
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("cmd-q", Quit, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            // Quitting exits the process without dropping the host, so running turns are
            // stopped here, before their provider processes could outlive the app.
            let host = bridge.clone();
            cx.on_app_quit(move |_| {
                host.shutdown();
                async {}
            })
            .detach();
            cx.on_action(|_: &About, _| show_about());
            // Settings needs no main window, so it opens even after that one is closed.
            // Deferred so ⌘, pressed inside Settings can still focus that window.
            let settings_bridge = bridge.clone();
            cx.on_action(move |_: &OpenSettings, cx| {
                let bridge = settings_bridge.clone();
                cx.defer(move |cx| settings::Settings::open(bridge, cx));
            });
            cx.set_menus(vec![
                Menu::new("Wiffletree").items([
                    MenuItem::action("About Wiffletree", About),
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::action("Check for Updates…", CheckForUpdates),
                    MenuItem::separator(),
                    MenuItem::action("Quit Wiffletree", Quit),
                ]),
                Menu::new("File").items([MenuItem::action("New Project", NewProject)]),
            ]);
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(960.), px(600.))),
                        titlebar: Some(TitleBar::title_bar_options()),
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_window_title("Wiffletree");
                        let view = cx.new(|cx| Workspace::new(bridge, window, cx));
                        // Registered on the app so ⌘N and the menu work whatever has focus.
                        // Deferred because a shortcut arrives while the window is mid-update.
                        let workspace = view.downgrade();
                        let handle = window.window_handle();
                        let new_project = workspace.clone();
                        cx.on_action(move |_: &NewProject, cx| {
                            let workspace = new_project.clone();
                            cx.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    workspace.update(cx, |view, cx| view.new_project(window, cx))
                                });
                            });
                        });
                        let check_for_updates = workspace.clone();
                        cx.on_action(move |_: &CheckForUpdates, cx| {
                            let workspace = check_for_updates.clone();
                            cx.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    workspace.update(cx, |view, cx| {
                                        view.sidebar.update(cx, |sidebar, cx| {
                                            sidebar.check_for_updates(true, window, cx)
                                        })
                                    })
                                });
                            });
                        });
                        // Settings opens these pages in the main window, which comes forward.
                        let open = move |page: Page, cx: &mut App| {
                            let workspace = workspace.clone();
                            cx.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    window.activate_window();
                                    workspace.update(cx, |view, cx| {
                                        view.handle(&Intent::OpenPage(page), window, cx)
                                    })
                                });
                            });
                        };
                        let open_models = open.clone();
                        cx.on_action(move |_: &OpenModels, cx| open_models(Page::Models, cx));
                        cx.on_action(move |_: &OpenRepositories, cx| open(Page::Repositories, cx));
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .expect("Open native workspace");
            if let Some(steps) = steps {
                // The untyped handle leaves the root view free for the events a step dispatches.
                let window: AnyWindowHandle = window.into();
                let _ = automation::NativeWindow::stay_on_stage();
                cx.spawn(async move |cx| {
                    while let Ok((step, reply)) = steps.recv().await {
                        let outcome = window
                            .update(cx, |_, window, cx| step.perform(window, cx))
                            .unwrap_or_else(|e| Err(e.to_string()));
                        let _ = reply.send(outcome).await;
                    }
                })
                .detach();
            } else {
                cx.activate(true);
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArchiveDemand, Coalesced, Page, Panel, wants_archive};
    use serde_json::json;
    use std::rc::Rc;
    use workspace_core::*;

    fn session(id: &str, archived: bool) -> Session {
        serde_json::from_value(json!({
            "id":id,"project_id":"p","parent_id":null,"repository_id":null,"name":id,
            "role":"project_orchestrator","provider":"claude","status":"ready",
            "archived":archived
        }))
        .unwrap()
    }

    /// A live coordinator and an archived one only the archive holds, read at stamp 1; the
    /// snapshot's stamp is `stamp`.
    fn index(stamp: u64) -> WorkspaceIndex {
        let snapshot = Snapshot {
            sessions: vec![session("live", false)],
            archive_stamp: stamp,
            ..Default::default()
        };
        let archive = Archive {
            sessions: vec![session("old", true)],
            session_order: vec!["old".into(), "live".into()],
            stamp: 1,
            ..Default::default()
        };
        WorkspaceIndex::new(snapshot, Rc::new(archive))
    }

    fn shown(selected: &str) -> ArchiveDemand<'_> {
        ArchiveDemand {
            show_archived: false,
            panel: None,
            page: Page::Conversation,
            selected: Some(selected),
            messages: &[],
        }
    }

    #[test]
    fn the_archive_is_read_again_only_when_it_moved_and_something_shown_needs_it() {
        let moved = index(2);
        assert!(!wants_archive(&moved, &shown("live")), "the default view");
        let demands = [
            ArchiveDemand {
                show_archived: true,
                ..shown("live")
            },
            ArchiveDemand {
                panel: Some(Panel::Team),
                ..shown("live")
            },
            ArchiveDemand {
                panel: Some(Panel::Crew),
                ..shown("live")
            },
            // It names the agents behind each event.
            ArchiveDemand {
                panel: Some(Panel::Events),
                ..shown("live")
            },
            ArchiveDemand {
                page: Page::Repositories,
                ..shown("live")
            },
            // An archived session already in the held archive, whose status may have moved.
            shown("old"),
        ];
        for demand in &demands {
            assert!(wants_archive(&moved, demand));
            assert!(
                !wants_archive(&index(1), demand),
                "the archive held is current"
            );
        }
    }

    #[test]
    fn a_message_from_an_archived_sender_needs_the_archive_once_it_moved() {
        let message: Message = serde_json::from_value(json!({
            "sequence":1,"id":"m","project_id":"p","sender":"old","recipient":"live",
            "body":"Done","receipt":"completed","created_at":0
        }))
        .unwrap();
        let messages = [message];
        let shown = ArchiveDemand {
            messages: &messages,
            ..shown("live")
        };
        assert!(wants_archive(&index(2), &shown));
        assert!(!wants_archive(&index(1), &shown));
    }

    #[test]
    fn a_pending_publish_request_needs_the_archive_once_it_moved() {
        // Its card is drawn from its ticket, which the archive holds once accepted.
        let publish = Attention {
            id: "a".into(),
            project_id: "p".into(),
            session_id: "live".into(),
            host: "local".into(),
            operation_id: format!("{PUBLISH_PREFIX}t:push"),
            prompt: "Push?".into(),
            options: vec![],
            answer: None,
        };
        let with_publish = |stamp| {
            let index = index(stamp);
            let snapshot = Snapshot {
                attention: vec![publish.clone()],
                ..index.snapshot().clone()
            };
            WorkspaceIndex::new(snapshot, index.archive().clone())
        };
        assert!(wants_archive(&with_publish(2), &shown("live")));
        assert!(!wants_archive(&with_publish(1), &shown("live")));
        let question = Attention {
            operation_id: "question:q".into(),
            ..publish
        };
        let index = index(2);
        let snapshot = Snapshot {
            attention: vec![question],
            ..index.snapshot().clone()
        };
        let asked = WorkspaceIndex::new(snapshot, index.archive().clone());
        assert!(!wants_archive(&asked, &shown("live")), "a question is not");
    }

    #[test]
    fn asks_while_a_request_is_answered_send_it_once_more_after() {
        let mut request = Coalesced::default();
        assert!(request.send());
        assert!(!request.send() && !request.send(), "one is on its way");
        assert!(request.answered(), "the asks during it go as one more");
        assert!(request.send());
        assert!(!request.answered(), "nothing was asked meanwhile");
        assert!(request.send());
    }
}
