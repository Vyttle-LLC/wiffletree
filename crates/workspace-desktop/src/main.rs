mod activity;
mod assets;
mod automation;
mod bridge;
mod context_view;
mod conversation;
mod creation;
mod data_dir;
mod history_sheet;
mod inspector;
mod login_env;
mod memory_view;
mod models;
mod models_view;
mod palette;
mod preferences;
mod project_name;
mod repositories_view;
mod settings;
mod sidebar;
mod team_view;
mod ui;
mod update;
mod usage_view;
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
use preferences::Appearance;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    rc::Rc,
};
// GPUI exports an accessibility `Role` too; ours wins over both globs.
use workspace_core::Role;
use workspace_core::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Overview,
    Team,
    Memory,
    Events,
    Attention,
    Git,
}

/// What fills the main area: one session's conversation, or a page about the whole workspace.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Conversation,
    Usage,
    Models,
    Repositories,
}

fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id).small()
}

struct Workspace {
    bridge: Bridge,
    snapshot: Option<Snapshot>,
    load_error: Option<String>,
    selected: Option<String>,
    messages: Rc<Vec<Message>>,
    list: ListState,
    drafts: BTreeMap<String, String>,
    cached_pages: BTreeMap<String, Rc<Vec<Message>>>,
    cache_order: VecDeque<String>,
    input: Entity<TextareaState>,
    sending: BTreeSet<String>,
    page: Option<i64>,
    memory_title: Entity<InputState>,
    memory_body: Entity<TextareaState>,
    memory_kind: LogKind,
    brain_path: Entity<InputState>,
    answer: Entity<TextareaState>,
    answering: Option<String>,
    role_defaults: Vec<RoleDefault>,
    saved_role_defaults: Vec<RoleDefault>,
    profile_tabs: [Provider; 4],
    model_catalog: Vec<workspace_host::runtime::ModelOption>,
    catalog_pending: usize,
    catalog_loaded: bool,
    catalog_notice: String,
    panel: Option<Panel>,
    panel_data: Value,
    view: Page,
    creation: Option<Entity<CreationForm>>,
    pending_project_selection: Option<String>,
    creating_project: bool,
    project_name: Entity<InputState>,
    project_edit: Option<project_name::NameEdit>,
    collapsed_projects: BTreeSet<String>,
    show_archived: bool,
    tree_scroll: ScrollHandle,
    quotas: Vec<QuotaReading>,
    quota_pending: bool,
    usage_report: Option<UsageReport>,
    usage_days: u16,
    usage_project: bool,
    usage_provider: Option<Provider>,
    usage_day: Option<String>,
    context: BTreeMap<String, Result<workspace_host::context::ContextUsage, String>>,
    context_pending: BTreeSet<String>,
    /// The turn each session's context was last measured after.
    context_seen: BTreeMap<String, Option<i64>>,
    last_repository_refresh: Option<std::time::Instant>,
    /// A downloaded release that installs on restart or quit.
    update: Option<update::Staged>,
    checking_update: bool,
    /// Hides the update card; the footer keeps a smaller restart button.
    update_card_dismissed: bool,
    /// The selected session's latest run, shown as the activity card while it works.
    activity: Option<activity::RunActivity>,
    step_fetch: activity::StepFetch,
    activity_clock: bool,
    preferences: preferences::Preferences,
    /// The open turn history, which follows step changes while it is shown.
    history: Option<WeakEntity<history_sheet::HistorySheet>>,
    /// Finished runs' durations and step counts, keyed by run.
    run_summaries: BTreeMap<String, RunSummary>,
}
impl Workspace {
    fn new(
        bridge: Bridge,
        preferences: preferences::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 9));
        // The send button follows the draft, so redraw as it changes.
        cx.subscribe(&input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let project_name = cx.new(|cx| InputState::new(window, cx).placeholder("Project name"));
        cx.subscribe_in(
            &project_name,
            window,
            |view, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    view.save_project_name(window, cx)
                }
                _ => {}
            },
        )
        .detach();
        let answer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("Your answer…")
        });
        let mut view = Self {
            bridge,
            snapshot: None,
            load_error: None,
            selected: None,
            messages: Rc::new(vec![]),
            list: ListState::new(0, ListAlignment::Bottom, px(300.)),
            drafts: BTreeMap::new(),
            cached_pages: BTreeMap::new(),
            cache_order: VecDeque::new(),
            input,
            sending: BTreeSet::new(),
            page: None,
            memory_title: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Title, e.g. Palette stays compatible")
            }),
            memory_body: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 8)
                    .placeholder("What was decided or observed, and why it matters")
            }),
            memory_kind: LogKind::Decision,
            brain_path: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Path to a Markdown brain folder")
            }),
            answer,
            answering: None,
            role_defaults: vec![],
            saved_role_defaults: vec![],
            profile_tabs: [Provider::Claude; 4],
            model_catalog: models::fallback(),
            catalog_pending: 0,
            catalog_loaded: false,
            catalog_notice: String::new(),
            panel: None,
            panel_data: Value::Null,
            view: Page::Conversation,
            creation: None,
            pending_project_selection: None,
            creating_project: false,
            project_name,
            project_edit: None,
            collapsed_projects: BTreeSet::new(),
            show_archived: false,
            tree_scroll: ScrollHandle::new(),
            quotas: vec![],
            quota_pending: false,
            usage_report: None,
            usage_days: 7,
            usage_project: false,
            usage_provider: None,
            usage_day: None,
            context: BTreeMap::new(),
            context_pending: BTreeSet::new(),
            context_seen: BTreeMap::new(),
            last_repository_refresh: None,
            update: None,
            checking_update: false,
            update_card_dismissed: false,
            activity: None,
            step_fetch: Default::default(),
            activity_clock: false,
            preferences,
            history: None,
            run_summaries: BTreeMap::new(),
        };
        if let Some(changes) = view.bridge.changes() {
            cx.spawn_in(window, async move |this, cx| {
                while changes.recv().await.is_ok() {
                    if cx
                        .update(|window, cx| {
                            this.update(cx, |view, cx| {
                                view.request(Command::Snapshot, window, cx);
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
        view.request(Command::Snapshot, window, cx);
        view.refresh_quota(window, cx);
        view.refresh_repositories(false, window, cx);
        // New clones show up when you come back to the app; there is no file watcher.
        cx.observe_window_activation(window, |view, window, cx| {
            if window.is_window_active() {
                view.refresh_repositories(false, window, cx);
            }
        })
        .detach();
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
                                view.request(Command::Quotas, window, cx);
                            }
                            cx.notify();
                        })
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view.watch_for_updates(window, cx);
        view
    }
    fn live_enabled(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| {
            self.project_id()
                .is_some_and(|id| s.live_projects.contains(&id))
        })
    }
    fn selected_session(&self) -> Option<&Session> {
        self.session(self.selected.as_deref()?)
    }
    fn session(&self, id: &str) -> Option<&Session> {
        self.snapshot.as_ref()?.sessions.iter().find(|s| s.id == id)
    }
    fn runtime(&self, session_id: &str) -> Option<&SessionRuntime> {
        self.snapshot
            .as_ref()?
            .runtimes
            .iter()
            .find(|runtime| runtime.session_id == session_id)
    }
    fn project_id(&self) -> Option<String> {
        self.selected_session().map(|s| s.project_id.clone())
    }
    fn project_attention(&self) -> Vec<&Attention> {
        let project = self.project_id();
        self.snapshot
            .iter()
            .flat_map(|s| &s.attention)
            .filter(|a| Some(&a.project_id) == project.as_ref())
            .collect()
    }
    fn toast(&self, message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(message), cx);
    }
    fn toast_error(&self, message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        struct RequestFailed;
        window.push_notification(
            Notification::error(message)
                .title("That didn’t work")
                .id::<RequestFailed>(),
            cx,
        );
    }
    /// Shows a failure where the user is looking: in its form when one owns it, otherwise as a toast.
    fn fail(
        &mut self,
        command: &Command,
        error: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.is_none() {
            self.load_error = Some(error);
        } else if !self.form_failed(command, &error, cx) {
            self.toast_error(error, window, cx);
        }
        cx.notify();
    }
    fn request(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = match self.bridge.request(command.clone()) {
            Ok(r) => r,
            Err(e) => return self.fail(&command, e, window, cx),
        };
        if let Command::Send { recipient, .. } = &command {
            self.sending.insert(recipient.clone());
        }
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver
                .recv()
                .await
                .unwrap_or_else(|_| Err("Host disconnected".into()));
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.accept(command, result, window, cx);
                    cx.notify();
                })
            });
        })
        .detach();
    }
    /// Checks the root folders for new repositories, at most once a minute unless `now`.
    fn refresh_repositories(&mut self, now: bool, window: &mut Window, cx: &mut Context<Self>) {
        let recent = self
            .last_repository_refresh
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60));
        if now || !recent {
            self.last_repository_refresh = Some(std::time::Instant::now());
            self.request(Command::RefreshRepositories, window, cx);
        }
    }
    fn accept(
        &mut self,
        command: Command,
        result: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(command, Command::RefreshRepositories) {
            // A background check; an unreadable root is not worth interrupting anyone for.
            match result.and_then(|v| {
                serde_json::from_value::<RepositoryRefresh>(v).map_err(|e| e.to_string())
            }) {
                Ok(refresh) if !refresh.added.is_empty() => {
                    let names: Vec<_> = refresh.added.iter().map(|r| r.name.as_str()).collect();
                    self.toast(
                        format!("Found new repositories: {}", names.join(", ")),
                        window,
                        cx,
                    );
                }
                Ok(_) => {}
                Err(error) => eprintln!("Repository refresh: {error}"),
            }
            // Also picks up folders that went missing since the last snapshot.
            self.request(Command::Snapshot, window, cx);
            return;
        }
        if matches!(&command, Command::RecordQuota { .. }) {
            self.quota_pending = false;
        }
        if let Command::Send { recipient, .. } = &command {
            self.sending.remove(recipient);
        }
        let value = match result {
            Ok(v) => v,
            Err(e) => return self.fail(&command, e, window, cx),
        };
        if matches!(command, Command::ImportRepositories { .. }) {
            match serde_json::from_value::<RepositoryImport>(value) {
                Ok(result) => {
                    if !result.imported.is_empty() {
                        self.toast(
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
            self.request(Command::Snapshot, window, cx);
            return;
        }
        if matches!(
            command,
            Command::CreateSession { .. }
                | Command::CreateTicket { .. }
                | Command::AssignTicket { .. }
                | Command::AttachRepository { .. }
                | Command::SetProjectRepositories { .. }
        ) && self.creation.take().is_some()
        {
            window.close_dialog(cx);
        }
        match command {
            Command::Snapshot => match serde_json::from_value::<Snapshot>(value) {
                Ok(snapshot) => self.accept_snapshot(snapshot, window, cx),
                Err(e) => self.fail(&Command::Snapshot, e.to_string(), window, cx),
            },
            Command::Messages {
                session_id, before, ..
            } => {
                if self.selected.as_deref() == Some(&session_id) && self.page == before {
                    match serde_json::from_value::<Vec<Message>>(value) {
                        Ok(rows) => {
                            self.replace_messages(rows);
                            self.load_run_summaries(window, cx);
                            if before.is_none() {
                                self.cached_pages
                                    .insert(session_id.clone(), self.messages.clone());
                                self.cache_order.retain(|id| id != &session_id);
                                self.cache_order.push_back(session_id);
                                while self.cache_order.len() > 4 {
                                    if let Some(old) = self.cache_order.pop_front() {
                                        self.cached_pages.remove(&old);
                                    }
                                }
                            }
                        }
                        Err(e) => self.toast_error(e.to_string(), window, cx),
                    }
                }
            }
            Command::Send {
                recipient, body, ..
            } => {
                if self.selected.as_deref() == Some(&recipient)
                    && self.input.read(cx).value().as_str() == body
                {
                    self.input
                        .update(cx, |input, cx| input.set_value("", window, cx));
                }
                if self.drafts.get(&recipient) == Some(&body) {
                    self.drafts.remove(&recipient);
                }
                if self.selected.as_deref() == Some(&recipient) {
                    self.page = None;
                    self.load_messages(window, cx);
                }
            }
            Command::RunSummaries { .. } => match serde_json::from_value(value) {
                Ok(summaries) => self.accept_run_summaries(summaries),
                Err(error) => self.toast_error(error.to_string(), window, cx),
            },
            Command::Logs { project_id, .. } => {
                if self.panel == Some(Panel::Memory) && self.project_id() == Some(project_id) {
                    self.panel_data = value;
                }
            }
            Command::Quotas | Command::RecordQuota { .. } => {
                match serde_json::from_value(value) {
                    Ok(quotas) => self.quotas = quotas,
                    Err(error) => self.toast_error(error.to_string(), window, cx),
                }
                if self.view == Page::Usage {
                    self.load_usage(window, cx);
                }
            }
            Command::Usage {
                days,
                project_id,
                provider,
                ..
            } => {
                if days == self.usage_days
                    && project_id == self.usage_project_id()
                    && provider == self.usage_provider
                {
                    match serde_json::from_value(value) {
                        Ok(report) => self.usage_report = Some(report),
                        Err(error) => self.toast_error(error.to_string(), window, cx),
                    }
                }
            }
            Command::Activity { project_id, .. } => {
                if self.panel == Some(Panel::Events) && self.project_id() == Some(project_id) {
                    self.panel_data = value;
                }
            }
            Command::GitHistory { repository_id, .. } => {
                if self.panel == Some(Panel::Git)
                    && self
                        .selected_session()
                        .and_then(|s| s.repository_id.clone())
                        == Some(repository_id)
                {
                    self.panel_data = value;
                }
            }
            Command::CreateSession { .. } | Command::AssignTicket { .. } => {
                if let Ok(session) = serde_json::from_value::<Session>(value) {
                    let id = session.id.clone();
                    if let Some(snapshot) = &mut self.snapshot {
                        snapshot.sessions.push(session);
                    }
                    self.select(id, window, cx);
                }
                self.request(Command::Snapshot, window, cx);
            }
            Command::CreateTicket { coordinator_id, .. } => {
                self.select(coordinator_id, window, cx);
                self.panel = Some(Panel::Team);
                self.request(Command::Snapshot, window, cx);
            }
            Command::CreateProject { .. } => {
                if let Ok(project) = serde_json::from_value::<Project>(value) {
                    self.pending_project_selection = Some(project.id);
                }
                self.request(Command::Snapshot, window, cx);
            }
            Command::RenameProject { project_id, .. } => {
                if let Ok(project) = serde_json::from_value::<Project>(value) {
                    if let Some(snapshot) = &mut self.snapshot {
                        if let Some(saved) =
                            snapshot.projects.iter_mut().find(|p| p.id == project_id)
                        {
                            *saved = project.clone();
                        }
                        for session in &mut snapshot.sessions {
                            if session.project_id == project_id
                                && session.role == Role::ProjectOrchestrator
                            {
                                session.name = project.name.clone();
                            }
                        }
                    }
                    if self
                        .project_edit
                        .as_ref()
                        .is_some_and(|e| e.project_id == project_id)
                    {
                        self.project_edit = None;
                    }
                }
                self.request(Command::Snapshot, window, cx);
            }
            Command::SetArchived { archived, .. } => {
                let affected: Vec<Session> = serde_json::from_value(value).unwrap_or_default();
                let name = affected.first().map_or("Session", |s| s.name.as_str());
                self.toast(
                    if archived {
                        format!("Archived {name}. Nothing was deleted.")
                    } else {
                        format!("Restored {name}")
                    },
                    window,
                    cx,
                );
                let hidden = archived
                    && !self.show_archived
                    && affected
                        .iter()
                        .any(|s| Some(&s.id) == self.selected.as_ref());
                if hidden {
                    // Step out to the owner that is still listed, or to any remaining session.
                    let owner = affected[0].parent_id.clone().or_else(|| {
                        self.snapshot
                            .iter()
                            .flat_map(|s| &s.sessions)
                            .find(|s| !s.archived && affected.iter().all(|a| a.id != s.id))
                            .map(|s| s.id.clone())
                    });
                    match owner {
                        Some(id) => self.select(id, window, cx),
                        None => self.selected = None,
                    }
                }
                self.request(Command::Snapshot, window, cx);
            }
            Command::ResolveAttention { .. } => {
                self.answering = None;
                self.answer
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.request(Command::Snapshot, window, cx);
            }
            Command::DismissAttention { id } => {
                if self.answering.as_ref() == Some(&id) {
                    self.answering = None;
                }
                self.request(Command::Snapshot, window, cx);
            }
            Command::SetRoleDefaults { defaults } => {
                self.saved_role_defaults = defaults;
                self.toast("Role defaults saved", window, cx);
                self.request(Command::Snapshot, window, cx);
            }
            Command::AppendLog { .. } => {
                self.memory_title
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.memory_body
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.toast("Saved to project memory", window, cx);
                self.load_panel(window, cx);
            }
            Command::AttachBrain { .. } => {
                self.brain_path
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.toast("Markdown brain attached", window, cx);
                self.request(Command::Snapshot, window, cx);
            }
            Command::ExportLogs { .. } => {
                let exported = value["exported"].as_u64().unwrap_or_default();
                self.toast(
                    if exported == 0 {
                        "Nothing new to export".to_owned()
                    } else {
                        format!("Exported {exported} memories to the brain")
                    },
                    window,
                    cx,
                );
            }
            _ => {
                self.page = None;
                self.request(Command::Snapshot, window, cx);
            }
        }
    }
    fn accept_snapshot(&mut self, snapshot: Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        let new_root = self.pending_project_selection.as_ref().and_then(|project| {
            snapshot
                .sessions
                .iter()
                .find(|s| &s.project_id == project && s.role == Role::ProjectOrchestrator)
                .map(|s| s.id.clone())
        });
        if !snapshot
            .sessions
            .iter()
            .any(|s| Some(&s.id) == self.selected.as_ref())
        {
            self.selected = snapshot
                .sessions
                .iter()
                .find(|s| !s.archived)
                .map(|s| s.id.clone());
        }
        self.sync_role_defaults(&snapshot.policies);
        self.snapshot = Some(snapshot);
        self.load_error = None;
        self.sync_composer(window, cx);
        if let Some(root) = new_root {
            let project_id = self.pending_project_selection.take().unwrap();
            self.creating_project = false;
            self.collapsed_projects.remove(&project_id);
            self.select(root, window, cx);
            self.tree_scroll.set_offset(point(px(0.), px(0.)));
            self.begin_project_rename(project_id, window, cx);
        } else {
            self.sync_transcript_rows();
            self.load_messages(window, cx);
            self.load_panel(window, cx);
        }
        // A turn that just started has no steps yet; ask before the first one arrives.
        if self.selected_working() && !self.activity.as_ref().is_some_and(|a| a.running) {
            self.fetch_steps(window, cx);
        }
        self.sync_activity_clock(window, cx);
        self.sync_context(window, cx);
    }
    /// Unsaved edits survive refreshes; an untouched editor follows the saved policy.
    fn sync_role_defaults(&mut self, policies: &[RolePolicy]) {
        let saved = models::role_defaults(policies);
        if self.role_defaults == self.saved_role_defaults {
            self.role_defaults = saved.clone();
        }
        self.saved_role_defaults = saved;
    }
    /// Transcript rows are the loaded messages plus a trailing activity row while the agent works.
    fn transcript_rows(&self) -> usize {
        self.messages.len() + usize::from(self.selected_working())
    }
    fn sync_transcript_rows(&mut self) {
        let rows = self.transcript_rows();
        let shown = self.list.item_count();
        if shown < self.messages.len() {
            self.list.reset(rows);
        } else if shown != rows {
            self.list
                .splice(self.messages.len()..shown, rows - self.messages.len());
        }
    }
    /// Splices only what a refresh changed, so a reader scrolled up keeps their place.
    fn replace_messages(&mut self, rows: Vec<Message>) {
        let shown = std::mem::replace(&mut self.messages, Rc::new(rows));
        let Some((dropped, unchanged)) = conversation::transcript_overlap(&shown, &self.messages)
        else {
            self.list.reset(self.transcript_rows());
            return;
        };
        if dropped > 0 {
            self.list.splice(0..dropped, 0);
        }
        let (shown_rows, rows) = (self.list.item_count(), self.transcript_rows());
        if unchanged < self.messages.len() || shown_rows != rows {
            self.list.splice(unchanged..shown_rows, rows - unchanged);
        }
    }
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match self.selected_session().map(|s| s.role) {
            Some(Role::ProjectOrchestrator) => "Describe the goal, or reply to your coordinator…",
            Some(Role::TaskOrchestrator) => "Message this repository coordinator…",
            Some(_) => "Give this agent a direct instruction…",
            None => "Create a project to start…",
        };
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx)
        });
    }
    fn load_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session_id) = self.selected.clone() {
            self.request(
                Command::Messages {
                    session_id,
                    before: self.page,
                    limit: MAX_PAGE,
                },
                window,
                cx,
            );
        }
    }
    fn select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(old) = &self.selected {
            self.drafts
                .insert(old.clone(), self.input.read(cx).value().to_string());
        }
        let draft = self.drafts.get(&id).cloned().unwrap_or_default();
        self.input
            .update(cx, |input, cx| input.set_value(draft, window, cx));
        self.messages = self
            .cached_pages
            .get(&id)
            .cloned()
            .unwrap_or_else(|| Rc::new(vec![]));
        self.selected = Some(id);
        self.view = Page::Conversation;
        self.page = None;
        self.list.reset(self.transcript_rows());
        self.panel_data = Value::Null;
        if let Some(role) = self.selected_session().map(|s| s.role)
            && self.panel.is_some_and(|panel| !panel.available(role))
        {
            self.panel = Some(Panel::Overview);
        }
        self.sync_composer(window, cx);
        self.sync_context(window, cx);
        self.load_messages(window, cx);
        self.load_panel(window, cx);
        self.activity = None;
        self.fetch_steps(window, cx);
        self.sync_activity_clock(window, cx);
        cx.notify();
    }
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().to_string();
        if body.trim().is_empty() {
            return;
        }
        if let Some(recipient) = self.selected.clone() {
            if self.sending.contains(&recipient) {
                return;
            }
            self.drafts.insert(recipient.clone(), body.clone());
            self.request(
                Command::Send {
                    id: new_id(),
                    sender: None,
                    recipient,
                    body,
                },
                window,
                cx,
            );
        }
    }
    fn set_live(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(project_id) = self.project_id() {
            self.request(
                Command::SetLive {
                    project_id,
                    enabled,
                },
                window,
                cx,
            );
        }
    }
    fn set_archived(
        &mut self,
        session_id: String,
        archived: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request(
            Command::SetArchived {
                session_id,
                archived,
            },
            window,
            cx,
        );
    }
    fn reconcile(&mut self, retry: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session_id) = self.selected.clone() {
            self.request(Command::ReconcileSession { session_id, retry }, window, cx);
        }
    }
    fn toggle_pause(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.selected_session() {
            let status = if session.status == Status::Paused {
                Status::Ready
            } else {
                Status::Paused
            };
            self.request(
                Command::SetStatus {
                    session_id: session.id.clone(),
                    status,
                },
                window,
                cx,
            );
        }
    }
    fn selected_profile(&self) -> Option<ModelProfile> {
        let session = self.selected_session()?;
        self.runtime(&session.id)
            .and_then(|runtime| runtime.profile.clone())
            .or_else(|| {
                self.snapshot
                    .as_ref()?
                    .policies
                    .iter()
                    .find(|policy| policy.role == session.role)
                    .map(|policy| policy.default.clone())
            })
    }
    fn toggle_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel == Some(panel) {
            self.panel = None;
            cx.notify();
        } else {
            self.show_panel(panel, window, cx);
        }
    }
    fn show_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Some(panel);
        self.panel_data = Value::Null;
        self.load_panel(window, cx);
        cx.notify();
    }
    /// Opens a workspace page in place of the conversation.
    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.view = page;
        match page {
            Page::Usage => {
                self.request(Command::Quotas, window, cx);
                self.load_usage(window, cx);
            }
            Page::Models if !self.catalog_loaded => self.refresh_models(window, cx),
            Page::Repositories => self.refresh_repositories(true, window, cx),
            _ => {}
        }
        cx.notify();
    }
    fn load_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project_id) = self.project_id() else {
            return;
        };
        match self.panel {
            Some(Panel::Memory) => self.request(
                Command::Logs {
                    project_id,
                    before: None,
                    limit: 30,
                },
                window,
                cx,
            ),
            Some(Panel::Events) => self.request(
                Command::Activity {
                    project_id,
                    before: None,
                    limit: 50,
                },
                window,
                cx,
            ),
            Some(Panel::Git) => {
                if let Some(repository_id) = self
                    .selected_session()
                    .and_then(|s| s.repository_id.clone())
                {
                    self.request(
                        Command::GitHistory {
                            repository_id,
                            skip: 0,
                            limit: 30,
                        },
                        window,
                        cx,
                    )
                }
            }
            _ => {}
        }
    }
    fn refresh_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.catalog_pending > 0 {
            return;
        }
        self.catalog_pending = 2;
        self.catalog_loaded = true;
        self.catalog_notice.clear();
        let (sender, receiver) = async_channel::bounded(2);
        for provider in models::PROVIDERS {
            let sender = sender.clone();
            std::thread::spawn(move || {
                let result = workspace_host::runtime::models(provider).map_err(|e| e.to_string());
                let _ = sender.send_blocking((provider, result));
            });
        }
        drop(sender);
        cx.spawn_in(window, async move |this, cx| {
            while let Ok((provider, result)) = receiver.recv().await {
                let updated = cx.update(|_, cx| {
                    this.update(cx, |view, cx| {
                        view.catalog_pending -= 1;
                        match result {
                            Ok(models) if !models.is_empty() => {
                                view.model_catalog
                                    .retain(|model| model.provider != provider);
                                view.model_catalog.extend(models);
                            }
                            _ => {
                                if !view.catalog_notice.is_empty() {
                                    view.catalog_notice.push(' ');
                                }
                                view.catalog_notice.push_str(&format!(
                                    "{provider:?} list unavailable; showing built-in and saved models."
                                ));
                            }
                        }
                        cx.notify();
                    })
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
    fn save_role_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(
            Command::SetRoleDefaults {
                defaults: self.role_defaults.clone(),
            },
            window,
            cx,
        );
    }
    /// Checks for a newer release now and hourly, and installs a staged one when the app quits.
    fn watch_for_updates(&self, window: &mut Window, cx: &mut Context<Self>) {
        if update::VERSION.is_none() || update::installed_bundle().is_none() {
            return;
        }
        cx.on_app_quit(|view, _| {
            if let Some(staged) = view.update.take() {
                let _ = staged.install_after_exit(false);
            }
            async {}
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let checked = cx.update(|window, cx| {
                    this.update(cx, |view, cx| view.check_for_updates(false, window, cx))
                });
                if checked.is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(60 * 60))
                    .await;
            }
        })
        .detach();
    }
    /// Downloads and stages a newer release. A `manual` check also reports when there is none.
    fn check_for_updates(&mut self, manual: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.update.is_some() {
            if manual {
                self.update_card_dismissed = false;
                cx.notify();
            }
            return;
        }
        let (Some(current), Some(installed)) = (update::VERSION, update::installed_bundle()) else {
            if manual {
                window.push_notification(
                    Notification::info(
                        "This build can't update itself. Install a release to get updates.",
                    ),
                    cx,
                );
            }
            return;
        };
        if self.checking_update {
            return;
        }
        self.checking_update = true;
        cx.notify();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(update::check(current, &installed));
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(result) = receiver.recv().await else {
                return;
            };
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.checking_update = false;
                    match result {
                        Ok(Some(staged)) => {
                            view.update = Some(staged);
                            view.update_card_dismissed = false;
                        }
                        Ok(None) if manual => view.toast(
                            format!("Wiffletree {current} is the latest version"),
                            window,
                            cx,
                        ),
                        Ok(None) => {}
                        Err(error) if manual => window.push_notification(
                            Notification::error(format!("{error:#}"))
                                .title("Couldn’t check for updates"),
                            cx,
                        ),
                        Err(error) => eprintln!("Update check failed: {error:#}"),
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    }
    /// Restarts into the staged release, confirming first if agents would be interrupted.
    fn restart_to_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let working = self
            .snapshot
            .iter()
            .flat_map(|s| &s.sessions)
            .any(|s| s.status == Status::Working);
        if !working {
            return self.install_update(window, cx);
        }
        let weak = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .title("Restart to update?")
                .confirm()
                .ok_text("Restart")
                .child("Agents are working. Restarting interrupts their turns; you can retry the held input afterwards.")
                .on_ok(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| view.install_update(window, cx));
                    true
                })
        });
    }
    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(staged) = self.update.take() else {
            return;
        };
        match staged.install_after_exit(true) {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.update = Some(staged);
                self.toast_error(format!("{error:#}"), window, cx);
            }
        }
    }
    /// Returns true when an open form displays the failure itself.
    fn form_failed(&mut self, command: &Command, error: &str, cx: &mut Context<Self>) -> bool {
        if matches!(command, Command::CreateProject { .. }) {
            self.creating_project = false;
        }
        if let Command::RenameProject { project_id, .. } = command
            && let Some(edit) = &mut self.project_edit
            && edit.project_id == *project_id
        {
            edit.saving = false;
            edit.error = error.into();
            return true;
        }
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
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let project = self.project_id();
        let has_repository = project
            .as_ref()
            .is_some_and(|id| !snapshot.project_repositories(id).is_empty());
        // A team needs a repository, so start with the step that is actually missing.
        let kind = if kind == Creation::Coordinator && !has_repository {
            window.push_notification(
                Notification::info("Add a repository first. Each team works in one."),
                cx,
            );
            Creation::Import
        } else {
            kind
        };
        // The workspace Repositories page adds to the workspace, not to the selected project.
        let context = (self.view != Page::Repositories)
            .then(|| self.selected_session())
            .flatten();
        let form = cx.new(|cx| {
            CreationForm::new(
                kind,
                snapshot,
                context,
                ticket,
                self.bridge.clone(),
                window,
                cx,
            )
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
    fn capture_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected.clone() else {
            return;
        };
        let title = self.memory_title.read(cx).value().trim().to_string();
        let changed = self.memory_body.read(cx).value().trim().to_string();
        if title.is_empty() || changed.is_empty() {
            self.toast_error("Add a title and a description first.", window, cx);
            return;
        }
        self.request(
            Command::AppendLog {
                input: LogInput {
                    session_id,
                    milestone: new_id(),
                    kind: self.memory_kind.clone(),
                    title,
                    reference: "—".into(),
                    changed,
                    why: "Captured directly in native prototype".into(),
                    state: "Source report; evidence not independently verified".into(),
                },
            },
            window,
            cx,
        );
    }
    fn colors(&self, cx: &App) -> Palette {
        palette(self.is_dark(cx))
    }
    fn is_dark(&self, cx: &App) -> bool {
        match self.preferences.appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => matches!(
                cx.window_appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }
    /// Applies to every open window and is remembered for the next launch.
    fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.preferences.appearance = appearance;
        if let Err(error) = self.preferences.save() {
            eprintln!("Saving preferences: {error}");
        }
        self.apply_theme(cx);
        cx.notify();
    }
    fn apply_theme(&self, cx: &mut App) {
        let dark = self.is_dark(cx);
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
            theme.colors.border = p.edge.opacity(0.5);
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
            theme.colors.link = p.focus;
            theme.colors.table_head_foreground = p.text;
            theme.colors.selection = p.focus.opacity(0.3);
            theme.colors.muted = p.surface;
            theme.colors.muted_foreground = p.subtle;
            theme.colors.input = p.edge;
            theme.colors.ring = p.focus;
            theme.colors.title_bar = p.surface;
            theme.colors.title_bar_border = p.edge.opacity(0.35);
            theme.colors.danger = p.red;
            theme.colors.success = p.green;
            theme.colors.warning = p.yellow;
            theme.colors.info = p.focus;
        });
    }
}

actions!(
    workspace,
    [
        Quit,
        SendMessage,
        NewProject,
        About,
        OpenSettings,
        CheckForUpdates
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
    let preferences = preferences::Preferences::load(&home);
    let bridge = Bridge::start(home, repository);
    let steps = automation.as_deref().map(automation::listen).transpose()?;
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.bind_keys([
                KeyBinding::new("enter", SendMessage, Some("ChatComposer > Input")),
                KeyBinding::new("cmd-enter", SendMessage, Some("ChatComposer > Input")),
                KeyBinding::new("cmd-n", NewProject, None),
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("cmd-q", Quit, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &About, _| show_about());
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
                        let settings_bridge = bridge.clone();
                        let view = cx.new(|cx| {
                            let v = Workspace::new(bridge, preferences, window, cx);
                            v.apply_theme(cx);
                            v
                        });
                        // Registered on the app so ⌘N and the menu work whatever has focus.
                        // Deferred because a shortcut arrives while the window is mid-update.
                        let workspace = view.downgrade();
                        let settings_workspace = workspace.clone();
                        cx.on_action(move |_: &OpenSettings, cx| {
                            let workspace = settings_workspace.clone();
                            let bridge = settings_bridge.clone();
                            cx.defer(move |cx| {
                                if let Some(workspace) = workspace.upgrade() {
                                    settings::Settings::open(&workspace, bridge, cx);
                                }
                            });
                        });
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
                        cx.on_action(move |_: &CheckForUpdates, cx| {
                            let workspace = workspace.clone();
                            cx.defer(move |cx| {
                                let _ = handle.update(cx, |_, window, cx| {
                                    workspace.update(cx, |view, cx| {
                                        view.check_for_updates(true, window, cx)
                                    })
                                });
                            });
                        });
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
