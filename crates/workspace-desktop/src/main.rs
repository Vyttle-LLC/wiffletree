mod activity;
mod agents_panel;
mod assets;
mod attachment_chips;
mod automation;
mod background;
mod brain_browser_view;
mod brain_markdown;
mod brain_search;
mod brains_view;
mod bridge;
mod browse;
mod connect_sheet;
mod context_view;
mod conversation;
mod conversation_header;
mod creation;
mod events_view;
mod file_links;
mod files_view;
mod folders;
mod history_sheet;
mod hosts;
mod inbox;
mod inspector;
mod leftovers;
mod live_work;
mod memory_view;
mod menu_bar;
mod models;
mod models_view;
mod native_quit;
mod new_project;
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
mod spaces_view;
mod staging;
mod team_view;
#[cfg(test)]
mod test_support;
mod ticket_table;
mod transcript;
mod tree;
mod ui;
mod update;
mod uploads;
mod usage_view;
mod verifiers;
mod welcome;
mod wording;
mod workspace_notes;
use bridge::{Bridge, ChangedSteps};
use creation::{Creation, CreationForm};
use gpui::{prelude::*, *};
use gpui_component::{
    Root, Sizable, Theme, ThemeMode, TitleBar, WindowExt,
    button::Button,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    notification::Notification,
    scroll::ScrollableElement,
};
use hosts::{HostChoice, HostView, Hosts};
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
    /// Opened from the conversation header or the coordinator's agents card, not from the
    /// rail. It lists the whole project, so it stays open whichever of the project's agents is
    /// selected.
    Agents,
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
    /// A coordinator's ticket table lists its done tickets.
    done_tickets: bool,
}

/// The archive is out of date and something shown needs it: Show archived, the Tickets, Agents or
/// Events panel, the Repositories page, a ticket table's done list or a publish or merge request,
/// or a selected session or message sender
/// that the snapshot does not list, which only the archive holds.
fn wants_archive(index: &WorkspaceIndex, shown: &ArchiveDemand) -> bool {
    let snapshot = index.snapshot();
    let live: BTreeSet<&str> = snapshot.sessions.iter().map(|s| s.id.as_str()).collect();
    let archived = |id: &str| !live.contains(id);
    index.archive_is_stale()
        && (shown.show_archived
            || matches!(
                shown.panel,
                Some(Panel::Team | Panel::Agents | Panel::Events)
            )
            || shown.page == Page::Repositories
            || shown.done_tickets
            || (snapshot.attention.iter()).any(|a| a.is_publish() || a.is_merge())
            || shown.selected.is_some_and(archived)
            || shown
                .messages
                .iter()
                .any(|m| m.sender.as_deref().is_some_and(archived)))
}

/// Whether changed steps include the selected session's, which its transcript shows.
fn shows_steps_of_selected(shared: &Shared, changed: &ChangedSteps) -> bool {
    shared
        .selected
        .as_deref()
        .is_some_and(|id| changed.includes(id))
}

/// Whether changed steps include files the Files panel lists for the selected session: its
/// own, or for a project coordinator, its agents' too.
fn shows_files_of(shared: &Shared, changed: &ChangedSteps) -> bool {
    let Some(selected) = shared.selected_session() else {
        return false;
    };
    let coordinator = selected.role == Role::ProjectOrchestrator;
    let mut agents = (shared.index.iter())
        .flat_map(|index| index.sessions())
        .filter(|s| coordinator && s.parent_id.as_ref() == Some(&selected.id));
    changed.includes(&selected.id) || agents.any(|s| changed.includes(&s.id))
}

/// Replaces the readings held only when a read brings different ones: the same readings again
/// would only redraw what draws them.
fn keep_quotas(held: &mut Rc<Vec<QuotaReading>>, read: Vec<QuotaReading>) {
    if **held != read {
        *held = Rc::new(read);
    }
}

/// What fills the main area: one session's conversation, or a page about the whole workspace.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Conversation,
    Usage,
    Models,
    Skills,
    Repositories,
    Spaces,
    Brains,
    /// One brain's Status, Wiki, Logs and Conflicts, opened from Brains or a Memory panel.
    BrainBrowser,
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
    /// The host refused the app as it last connected.
    refused: bool,
    /// The human chose to let a refusing host's agents finish before it restarts.
    waiting_for_host: bool,
    models: Entity<models_view::ModelsPage>,
    skills: Entity<skills_view::SkillsPage>,
    conversation: Entity<conversation::Conversation>,
    creation: Option<Entity<CreationForm>>,
    repositories: Entity<repositories_view::RepositoriesPage>,
    spaces: Entity<spaces_view::SpacesPage>,
    brains: Entity<brains_view::BrainsPage>,
    brain_browser: Entity<brain_browser_view::BrainBrowser>,
    brain_search: Entity<brain_search::BrainSearch>,
    usage: Entity<usage_view::UsagePage>,
    sidebar: Entity<sidebar::Sidebar>,
    inspector: Entity<Inspector>,
    inbox: Entity<inbox::Inbox>,
    contexts: Entity<context_view::Contexts>,
    last_repository_refresh: Option<std::time::Instant>,
    progress_fetch: activity::StepFetch,
    /// Holds focus when nothing else does, such as on a page without a text field, so keys
    /// still pass through the workspace's key context.
    focus: FocusHandle,
}
impl Workspace {
    fn new(bridge: Bridge, host: HostView, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let spaces = cx.new(|cx| spaces_view::SpacesPage::new(bridge.clone(), window, cx));
        Self::adopt(&spaces, window, cx);
        let brains = cx.new(|cx| brains_view::BrainsPage::new(bridge.clone(), window, cx));
        Self::adopt(&brains, window, cx);
        let brain_browser = cx.new(|_| brain_browser_view::BrainBrowser::new(bridge.clone()));
        Self::adopt(&brain_browser, window, cx);
        let brain_search = cx.new(|cx| brain_search::BrainSearch::new(bridge.clone(), window, cx));
        Self::adopt(&brain_search, window, cx);
        // The shell draws the overlay while it is open.
        cx.observe(&brain_search, |_, _, cx| cx.notify()).detach();
        let sidebar = cx.new(|cx| sidebar::Sidebar::new(bridge.clone(), window, cx));
        Self::adopt(&sidebar, window, cx);
        let usage = cx.new(|_| usage_view::UsagePage::new(bridge.clone()));
        Self::adopt(&usage, window, cx);
        let mut view = Self {
            bridge,
            shared: Shared {
                host,
                ..Shared::default()
            },
            snapshot_request: Coalesced::default(),
            quota_request: Coalesced::default(),
            archive_pending: false,
            load_error: None,
            refused: false,
            waiting_for_host: false,
            models,
            skills,
            conversation,
            creation: None,
            repositories,
            spaces,
            brains,
            brain_browser,
            brain_search,
            usage,
            sidebar,
            inspector,
            inbox,
            contexts,
            last_repository_refresh: None,
            progress_fetch: Default::default(),
            focus: cx.focus_handle(),
        };
        cx.on_focus_lost(window, |view, window, cx| view.focus.focus(window, cx))
            .detach();
        if let Some(changes) = view.bridge.changes() {
            cx.spawn_in(window, async move |this, cx| {
                while changes.recv().await.is_ok() {
                    if cx
                        .update(|window, cx| {
                            this.update(cx, |view, cx| {
                                // A refusing host answers nothing, so its screen is drawn now.
                                let refused = view.bridge.refusal().is_some();
                                if refused != std::mem::replace(&mut view.refused, refused) {
                                    view.waiting_for_host &= refused;
                                    cx.notify();
                                }
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
        view.watch_link(window, cx);
        view.watch_steps(window, cx);
        view.watch_progress(window, cx);
        view.watch_usage(window, cx);
        render_counts::watch_stalls(cx);
        view.count_notifies(cx);
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
    /// Counts the notifies of the views that redraw on their own, for `render_counts`.
    fn count_notifies(&self, cx: &mut Context<Self>) {
        use render_counts::count_notifies;
        count_notifies(&cx.entity(), "Workspace", cx);
        count_notifies(&self.sidebar, "Sidebar", cx);
        count_notifies(&self.conversation, "Conversation", cx);
        let transcript = self.conversation.read(cx).transcript.clone();
        count_notifies(&transcript, "Transcript", cx);
        count_notifies(&self.inspector, "Inspector", cx);
        count_notifies(&self.inbox, "Inbox", cx);
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
            Intent::MessagesShown | Intent::DoneTicketsShown => {
                return self.sync_archive(window, cx);
            }
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
            Intent::OpenSpace(space) => {
                self.open_page(Page::Spaces, window, cx);
                self.spaces.update(cx, |spaces, cx| {
                    spaces.open_space(space.clone(), window, cx)
                });
            }
            Intent::OpenBrain(opening) => {
                self.open_page(Page::BrainBrowser, window, cx);
                self.brain_browser
                    .update(cx, |browser, cx| browser.open(opening.clone(), window, cx));
            }
            Intent::OpenBatch(batch) => {
                self.open_page(Page::Brains, window, cx);
                (self.brains).update(cx, |brains, cx| brains.open_batch(batch.clone(), cx));
            }
            Intent::SearchBrain {
                repository,
                project,
            } => self.search_brain(Some((repository.clone(), project.clone())), window, cx),
            Intent::Catalog(catalog) => self.shared.catalog = catalog.clone(),
            Intent::Detected(detection) => self.shared.detection = detection.clone(),
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
        render_counts::timed("share", || self.share_now(window, cx));
    }
    fn share_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        self.usage
            .update(cx, |usage, cx| usage.share(shared.clone(), window, cx));
        self.repositories.update(cx, |repositories, cx| {
            repositories.share(shared.clone(), window, cx)
        });
        self.spaces
            .update(cx, |spaces, cx| spaces.share(shared.clone(), window, cx));
        self.brains
            .update(cx, |brains, cx| brains.share(shared.clone(), window, cx));
        self.brain_browser
            .update(cx, |browser, cx| browser.share(shared.clone(), window, cx));
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
    /// On a remote host, follows the connection each second: the view freezes while the host does
    /// not answer, and the banner's age counts up meanwhile. The connection's own wake belongs to
    /// `hosts::follow`, which redraws the switcher.
    fn watch_link(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shared.host.remote.is_none() {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let ticked = cx.update(|window, cx| {
                    this.update(cx, |view, cx| {
                        let host = cx.global::<Hosts>().view_of(&view.bridge);
                        if host != view.shared.host {
                            view.shared.host = host;
                            view.share(window, cx);
                            cx.notify();
                        } else if view.shared.host.frozen(ui::now()).is_some() {
                            cx.notify();
                        }
                    })
                });
                if !matches!(ticked, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }
    /// Step changes go to the transcript, for its live work, and to the Files panel, each
    /// only when they concern a session it shows.
    fn watch_steps(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(changes) = self.bridge.step_changes() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            while changes.recv().await.is_ok() {
                let fetched = cx.update(|window, cx| {
                    this.update(cx, |view, cx| {
                        let changed = view.bridge.take_changed_steps();
                        if shows_steps_of_selected(&view.shared, &changed) {
                            let transcript = view.conversation.read(cx).transcript.clone();
                            transcript
                                .update(cx, |transcript, cx| transcript.fetch_steps(window, cx));
                        }
                        if shows_files_of(&view.shared, &changed) {
                            view.inspector
                                .update(cx, |inspector, cx| inspector.fetch_files(window, cx));
                        }
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
            Ok(quotas) => keep_quotas(&mut self.shared.quotas, quotas),
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
        let conversation = self.conversation.read(cx);
        let transcript = conversation.transcript.read(cx);
        let coordinator =
            (self.shared.selected_session()).is_some_and(|s| s.role == Role::ProjectOrchestrator);
        let wanted = self.shared.index.as_ref().is_some_and(|index| {
            let shown = ArchiveDemand {
                show_archived: self.shared.show_archived,
                panel: self.shared.panel,
                page: self.shared.page,
                selected: self.shared.selected.as_deref(),
                messages: transcript.messages(),
                done_tickets: coordinator && conversation.done_tickets_open,
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
        let archive = self.shared.archive();
        let index = render_counts::timed("index", || WorkspaceIndex::new(snapshot, archive));
        // A selected session the held archive is out of date for may be in the next one, which
        // `accept_archive` checks.
        let gone = |id: &String| index.session(id).is_none() && !index.archive_is_stale();
        if self.shared.selected.as_ref().is_none_or(gone) {
            self.shared.selected = index.sessions().find(|s| !s.archived).map(|s| s.id.clone());
        }
        let host = self.bridge.remote().map(|r| r.id.clone());
        settings::share_sync(index.snapshot(), host, cx);
        if self.shared.host.remote.is_none() {
            cx.global_mut::<Hosts>().this_mac_counts = hosts::counts(index.snapshot());
        }
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
    /// Opens the search overlay over `brain`'s repository and scope, or else over the brain
    /// browser's brain on that page, or the selected project's Space's brain.
    fn search_brain(
        &mut self,
        brain: Option<(String, Option<String>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let brain = brain.or_else(|| {
            (self.shared.page == Page::BrainBrowser)
                .then(|| self.brain_browser.read(cx).searched())
                .flatten()
        });
        let selected = brain_search::searched_for(&self.shared);
        let (searched, scope) = match brain {
            None => {
                let scope = match &selected {
                    brain_search::Searched::Brain { project, .. } => {
                        project.as_ref().map(|(id, _)| id.clone())
                    }
                    brain_search::Searched::NoBrain { .. } => None,
                };
                (selected, scope)
            }
            Some((repository, scope)) => {
                // The project chip is the scope's project, or the selected one in this brain.
                let project = self.shared.snapshot().and_then(|snapshot| {
                    let projects = brain_browser_view::projects_of_brain(snapshot, &repository);
                    let wanted = scope.clone().or_else(|| self.shared.project_id());
                    (projects.into_iter())
                        .find(|p| Some(&p.id) == wanted.as_ref())
                        .map(|p| (p.id.clone(), p.name.clone()))
                });
                let searched = brain_search::Searched::Brain {
                    repository,
                    project,
                };
                (searched, scope)
            }
        };
        self.brain_search
            .update(cx, |search, cx| search.open(searched, scope, window, cx));
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
        let verification = self.models.read(cx).verification(cx);
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
            .with_models(&self.shared, verification)
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
    change_preferences(cx, |preferences| preferences.group_projects = grouped);
}

/// Shows a working agent's activity as one line, or as the full card.
fn set_minimize_activity(minimized: bool, cx: &mut App) {
    change_preferences(cx, |preferences| preferences.minimize_activity = minimized);
}

/// Shows the coordinator's pinned ticket table as its title line only, or in full.
fn set_collapse_ticket_table(collapsed: bool, cx: &mut App) {
    change_preferences(cx, |preferences| {
        preferences.collapse_ticket_table = collapsed
    });
}

/// Finds Wiffletree on other Macs on this network while `on`; turning it off stops browsing and
/// clears what it found. Paired hosts still reconnect by their stored addresses either way.
fn set_find_on_network(on: bool, cx: &mut App) {
    change_preferences(cx, |preferences| preferences.find_on_network = on);
    cx.global::<Hosts>().browsed.browse_while(on);
    cx.refresh_windows();
}

fn change_preferences(cx: &mut App, change: impl FnOnce(&mut Preferences)) {
    let preferences = cx.global_mut::<Preferences>();
    change(preferences);
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

/// The open main window and its workspace, which the app's actions act on.
#[derive(Default)]
struct MainWindow(Option<(AnyWindowHandle, WeakEntity<Workspace>)>);
impl Global for MainWindow {}

/// The main window's content: the selected host's workspace, built anew when another host is
/// selected. Switching hosts stops nothing on any of them.
struct HostWindow {
    workspace: Entity<Workspace>,
    /// Each host's workspace once shown, kept with its view, drafts and sends in flight, so
    /// switching back finds it as it was and only one workspace waits on each host's wakes.
    kept: std::collections::HashMap<HostChoice, Entity<Workspace>>,
}
#[derive(Default)]
struct MainShell(Option<WeakEntity<HostWindow>>);
impl Global for MainShell {}
impl Render for HostWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.workspace.clone()
    }
}
impl HostWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = Self::workspace(window, cx);
        let selected = cx.global::<Hosts>().selected.clone();
        Self {
            kept: [(selected, workspace.clone())].into(),
            workspace,
        }
    }
    /// Shows the selected host's workspace, built anew when it was never shown or `fresh`, as
    /// after pairing again.
    fn show(&mut self, fresh: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = cx.global::<Hosts>().selected.clone();
        let kept = self.kept.get(&selected).filter(|_| !fresh).cloned();
        self.workspace = match kept {
            Some(workspace) => {
                cx.set_global(MainWindow(Some((
                    window.window_handle(),
                    workspace.downgrade(),
                ))));
                workspace.update(cx, |view, cx| view.refresh(window, cx));
                workspace
            }
            None => {
                let workspace = Self::workspace(window, cx);
                self.kept.insert(selected, workspace.clone());
                workspace
            }
        };
        cx.notify();
    }
    /// The selected host's workspace, which the app's actions then act on.
    fn workspace(window: &mut Window, cx: &mut App) -> Entity<Workspace> {
        let hosts = cx.global::<Hosts>();
        let (bridge, host) = (hosts.bridge(), hosts.view());
        let workspace = cx.new(|cx| Workspace::new(bridge, host, window, cx));
        cx.set_global(MainWindow(Some((
            window.window_handle(),
            workspace.downgrade(),
        ))));
        workspace
    }
}

/// Forgets the paired host `id` on this device: it leaves the switcher and the client settings,
/// and the main window shows This Mac if it showed that host. Nothing changes at the host.
fn forget_host(id: &str, cx: &mut App) {
    if cx.global::<Hosts>().selected == HostChoice::Remote(id.into())
        && let Some((handle, _)) = cx.default_global::<MainWindow>().0.clone()
    {
        let _ = handle.update(cx, |_, window, cx| {
            select_host(HostChoice::ThisMac, window, cx)
        });
    }
    cx.global_mut::<Hosts>().remove(id);
    if let Some(shell) = (cx.default_global::<MainShell>().0.clone()).and_then(|s| s.upgrade()) {
        shell.update(cx, |shell, _| {
            shell.kept.remove(&HostChoice::Remote(id.into()))
        });
    }
    let preferences = cx.global_mut::<Preferences>();
    preferences.paired_hosts.retain(|h| h.id != id);
    if let Err(error) = preferences.save() {
        eprintln!("Saving preferences: {error}");
    }
    cx.refresh_windows();
}

/// Shows `choice`'s workspace in the main window.
fn select_host(choice: HostChoice, window: &mut Window, cx: &mut App) {
    if cx.global::<Hosts>().selected != choice {
        show_host(choice, false, window, cx);
    }
}

/// Shows `choice`'s workspace, built anew when `fresh`, as after its connection was replaced.
fn show_host(choice: HostChoice, fresh: bool, window: &mut Window, cx: &mut App) {
    cx.global_mut::<Hosts>().select(choice);
    if let Some(shell) = (cx.default_global::<MainShell>().0.clone()).and_then(|s| s.upgrade()) {
        shell.update(cx, |shell, cx| shell.show(fresh, window, cx));
    }
    settings::Settings::host_selected(cx);
}

/// Runs `with` on the main window's workspace, if it is open. Deferred because a shortcut
/// arrives while the window is mid-update.
fn with_workspace(
    cx: &mut App,
    with: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    let Some((handle, workspace)) = cx.default_global::<MainWindow>().0.clone() else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            workspace.update(cx, |view, cx| with(view, window, cx))
        });
    });
}

/// The actions that act on the main window. Registered on the app so ⌘N and the menu work
/// whatever has focus, and once, whichever main window is open.
fn register_window_actions(cx: &mut App) {
    cx.on_action(|_: &NewProject, cx| {
        with_workspace(cx, |view, window, cx| view.new_project(window, cx))
    });
    cx.on_action(|_: &brain_search::OpenBrainSearch, cx| {
        with_workspace(cx, |view, window, cx| view.search_brain(None, window, cx))
    });
    cx.on_action(|_: &CheckForUpdates, cx| {
        with_workspace(cx, |view, window, cx| {
            view.sidebar.update(cx, |sidebar, cx| {
                sidebar.check_for_updates(true, window, cx)
            })
        })
    });
    // Settings opens these pages in the main window, which comes forward.
    let open = |page: Page, cx: &mut App| {
        with_workspace(cx, move |view, window, cx| {
            window.activate_window();
            view.handle(&Intent::OpenPage(page), window, cx)
        })
    };
    cx.on_action(move |_: &OpenModels, cx| open(Page::Models, cx));
    cx.on_action(move |_: &OpenRepositories, cx| open(Page::Repositories, cx));
}

/// Opens the main window on the selected host.
fn open_main_window(cx: &mut App) -> WindowHandle<Root> {
    let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(960.), px(600.))),
            titlebar: Some(TitleBar::title_bar_options()),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title("Wiffletree");
            let view = cx.new(|cx| HostWindow::new(window, cx));
            cx.set_global(MainShell(Some(view.downgrade())));
            cx.new(|cx| Root::new(view, window, cx))
        },
    )
    .expect("Open native workspace")
}

fn main() -> anyhow::Result<()> {
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
    let default_folder = home.is_none();
    let home = home.unwrap_or_else(|| {
        workspace_core::data_dir::default_directory(&PathBuf::from(
            std::env::var_os("HOME").expect("macOS home directory"),
        ))
    });
    let preferences = Preferences::load(&home);
    let staging = staging::Staging::open(&home);
    let bridge = Bridge::start(&home, default_folder, repository);
    let steps = automation.as_deref().map(automation::listen).transpose()?;
    let application = gpui_platform::application().with_assets(assets::Assets);
    application.on_reopen(background::reopened);
    application.run(move |cx| {
        gpui_component::init(cx);
        let browsed = browse::hosts();
        browsed.browse_while(preferences.find_on_network);
        cx.set_global(Hosts::new(
            bridge.clone(),
            &preferences.paired_hosts,
            browsed,
        ));
        cx.set_global(preferences);
        cx.set_global(attachment_chips::HostImages::new());
        cx.set_global(staging);
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
        cx.bind_keys(brain_search::bindings());
        cx.on_action(|_: &About, _| show_about());
        // Settings needs no main window, so it opens even after that one is closed.
        // Deferred so ⌘, pressed inside Settings can still focus that window.
        cx.on_action(move |_: &OpenSettings, cx| {
            cx.defer(settings::Settings::open);
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
        register_window_actions(cx);
        hosts::watch(cx);
        background::install(
            bridge,
            |cx| {
                open_main_window(cx);
            },
            cx,
        );
        let window = open_main_window(cx);
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
    use super::{
        ArchiveDemand, ChangedSteps, Coalesced, Page, Panel, Shared, keep_quotas, shows_files_of,
        shows_steps_of_selected, wants_archive,
    };
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
            done_tickets: false,
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
                panel: Some(Panel::Agents),
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
            // The ticket table's done list names tickets only the archive holds.
            ArchiveDemand {
                done_tickets: true,
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
    fn the_same_quota_readings_again_keep_the_ones_held() {
        let reading = |used| QuotaReading {
            provider: Provider::Codex,
            account: None,
            observed_at: 1,
            source: "Codex app-server".into(),
            windows: vec![QuotaWindow {
                key: "weekly".into(),
                label: "Weekly".into(),
                used_percent: Some(used),
                observed_at: 1,
                duration_mins: Some(10080),
                resets_at: None,
                status: None,
            }],
            error: None,
        };
        let mut held = Rc::new(vec![reading(40.)]);
        let before = held.clone();
        keep_quotas(&mut held, vec![reading(40.)]);
        assert!(
            Rc::ptr_eq(&held, &before),
            "nothing that draws them redraws"
        );
        keep_quotas(&mut held, vec![reading(41.)]);
        assert!(!Rc::ptr_eq(&held, &before));
        assert_eq!(*held, vec![reading(41.)]);
    }

    /// A coordinator with one agent and another project's coordinator, with `selected` selected.
    fn selecting(selected: &str) -> Shared {
        let mut agent = session("agent", false);
        (agent.parent_id, agent.role) = (Some("live".into()), Role::Implementer);
        let snapshot = Snapshot {
            sessions: vec![session("live", false), agent, session("other", false)],
            ..Default::default()
        };
        Shared {
            index: Some(Rc::new(WorkspaceIndex::new(snapshot, Rc::default()))),
            selected: Some(selected.into()),
            ..Default::default()
        }
    }

    fn changed(sessions: &[&str]) -> ChangedSteps {
        ChangedSteps {
            sessions: sessions.iter().map(|s| s.to_string()).collect(),
            all: false,
        }
    }

    #[test]
    fn steps_are_fetched_only_for_the_sessions_shown() {
        let coordinator = selecting("live");
        assert!(!shows_steps_of_selected(
            &coordinator,
            &changed(&["agent", "other"])
        ));
        assert!(shows_steps_of_selected(&coordinator, &changed(&["live"])));
        // The Files panel rolls up a coordinator's agents.
        assert!(shows_files_of(&coordinator, &changed(&["agent"])));
        assert!(!shows_files_of(&coordinator, &changed(&["other"])));
        let agent = selecting("agent");
        assert!(!shows_files_of(&agent, &changed(&["live"])));
        assert!(shows_files_of(&agent, &changed(&["agent"])));
    }

    #[test]
    fn after_a_resync_every_shown_session_s_steps_are_fetched() {
        let resynced = ChangedSteps {
            all: true,
            ..Default::default()
        };
        let shared = selecting("live");
        assert!(shows_steps_of_selected(&shared, &resynced));
        assert!(shows_files_of(&shared, &resynced));
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
