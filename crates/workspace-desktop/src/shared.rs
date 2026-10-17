//! What the shell shares with every view, and how a view talks to the host.
use super::*;
use serde::de::DeserializeOwned;

/// The host's latest workspace joined with its archive, what is selected, which page and panel
/// show, and the figures several views draw. The shell owns it and hands each view a copy when
/// it changes; the copy is cheap, its large parts are shared.
#[derive(Clone)]
pub(super) struct Shared {
    pub(super) index: Option<Rc<WorkspaceIndex>>,
    pub(super) selected: Option<String>,
    /// Counts selections, so selecting the selected session again still reads as one.
    pub(super) selections: u64,
    pub(super) page: Page,
    pub(super) panel: Option<Panel>,
    pub(super) show_archived: bool,
    /// How far along every agent, ticket and project is, as the host last derived it.
    pub(super) progress: Rc<progress::ProgressBoard>,
    pub(super) quotas: Rc<Vec<QuotaReading>>,
    /// A fresh reading of the account limits is on its way.
    pub(super) quota_pending: bool,
    /// The models each provider offers, as the Models page last listed them.
    pub(super) catalog: Rc<Vec<ModelOption>>,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            index: None,
            selected: None,
            selections: 0,
            page: Page::Conversation,
            panel: None,
            show_archived: false,
            progress: Rc::default(),
            quotas: Rc::default(),
            quota_pending: false,
            catalog: Rc::new(fallback_catalog()),
        }
    }
}
impl Shared {
    /// The latest snapshot of the live workspace; `index` also has the archive.
    pub(super) fn snapshot(&self) -> Option<&Snapshot> {
        self.index.as_deref().map(WorkspaceIndex::snapshot)
    }
    /// The archive last read; empty until the first is.
    pub(super) fn archive(&self) -> Rc<Archive> {
        self.index
            .as_ref()
            .map_or_else(Rc::default, |index| index.archive().clone())
    }
    pub(super) fn selected_session(&self) -> Option<&Session> {
        self.session(self.selected.as_deref()?)
    }
    pub(super) fn session(&self, id: &str) -> Option<&Session> {
        self.index.as_ref()?.session(id)
    }
    /// The revision of a session's messages in the latest snapshot; `None` when it has none,
    /// as for a session only the archive holds, or from an older host.
    pub(super) fn message_revision(&self, session_id: &str) -> Option<u64> {
        self.snapshot()?.message_revisions.get(session_id).copied()
    }
    pub(super) fn runtime(&self, session_id: &str) -> Option<&SessionRuntime> {
        self.index.as_ref()?.runtime(session_id)
    }
    pub(super) fn selected_role(&self) -> Role {
        self.selected_session()
            .map_or(Role::ProjectOrchestrator, |s| s.role)
    }
    pub(super) fn project_id(&self) -> Option<String> {
        self.selected_session().map(|s| s.project_id.clone())
    }
    pub(super) fn live_enabled(&self) -> bool {
        self.index.as_ref().is_some_and(|index| {
            self.project_id()
                .is_some_and(|id| index.snapshot().live_projects.contains(&id))
        })
    }
    /// Where a session's relative file references resolve: its ticket worktree or directory,
    /// then its project's home.
    pub(super) fn link_bases(&self, session_id: &str) -> Vec<PathBuf> {
        let runtime = self.runtime(session_id);
        let directory = runtime.and_then(|r| r.workdir.as_ref().or(r.directory.as_ref()));
        let home = self.session(session_id).and_then(|session| {
            let project = self.index.as_ref()?.project(&session.project_id)?;
            project.home.as_ref()
        });
        directory
            .into_iter()
            .chain(home)
            .map(PathBuf::from)
            .collect()
    }
    pub(super) fn project_attention(&self) -> Vec<&Attention> {
        let project = self.project_id();
        self.index
            .iter()
            .flat_map(|index| &index.snapshot().attention)
            .filter(|a| Some(&a.project_id) == project.as_ref())
            .collect()
    }
    /// The notes about the whole workspace, which every project's Inbox lists first.
    pub(super) fn workspace_notes(&self) -> &[WorkspaceNote] {
        self.index
            .as_ref()
            .map_or(&[], |index| &index.snapshot().workspace_notes)
    }
    pub(super) fn selected_profile(&self) -> Option<ModelProfile> {
        let session = self.selected_session()?;
        self.runtime(&session.id)
            .and_then(|runtime| runtime.profile.clone())
            .or_else(|| {
                // Only an unpinned main coordinator runs on its provider's first allowed model;
                // any other unpinned agent is held until it is given one.
                if session.parent_id.is_some() {
                    return None;
                }
                let model = self
                    .snapshot()?
                    .model_selection
                    .allowed(session.provider)
                    .first()?
                    .clone();
                Some(ModelProfile {
                    provider: session.provider,
                    model: model.model,
                    effort: model.effort,
                })
            })
    }
    pub(super) fn owner_name(&self) -> String {
        self.selected_session()
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }
    pub(super) fn project_name(&self) -> String {
        let project = self.project_id().unwrap_or_default();
        (self.index.as_ref())
            .and_then(|index| index.project(&project))
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }
    pub(super) fn repository(&self) -> Option<&Repository> {
        let id = self.selected_session()?.repository_id.as_ref()?;
        self.snapshot()?.repositories.iter().find(|r| &r.id == id)
    }
    /// The ticket a worker session belongs to.
    pub(super) fn ticket_of(&self, session_id: &str) -> Option<&Ticket> {
        self.index.as_ref()?.ticket_of(session_id)
    }
    /// The selected project's board entry.
    pub(super) fn project_progress(&self) -> Option<&ProjectProgress> {
        self.progress.project(&self.selected_session()?.project_id)
    }
    /// A running compaction reads as work in a session's badge.
    pub(super) fn compacting(&self, session_id: &str) -> bool {
        context_view::compacting(self.runtime(session_id))
    }
    /// New project is offered once a setup is saved; see `welcome::offers_new_project`.
    pub(super) fn offers_new_project(&self) -> bool {
        let saved = self.snapshot().map(|s| &s.model_selection);
        welcome::offers_new_project(saved.is_some(), saved.unwrap_or(&ModelSelection::default()))
    }
    /// Archived sessions are listed only while the sidebar shows them.
    pub(super) fn listed(&self, session: &Session) -> bool {
        self.show_archived || !session.archived
    }
    pub(super) fn shown_status(&self, session: &Session) -> Status {
        match &self.index {
            Some(index) => sidebar::shown_status(session, index),
            None => session.status,
        }
    }
    /// Whether a change selected a session, even the same one again, or replaced the
    /// workspace it reads.
    pub(super) fn moved_from(&self, old: &Shared) -> (bool, bool) {
        let selection = self.selected != old.selected || self.selections != old.selections;
        let workspace = !match (&self.index, &old.index) {
            (Some(new), Some(old)) => Rc::ptr_eq(new, old),
            (None, None) => true,
            _ => false,
        };
        (selection, workspace)
    }
}

/// What a view asks the shell to change in `Shared`, or to do on its behalf.
#[derive(Clone)]
pub(super) enum Intent {
    Select(String),
    OpenPage(Page),
    ShowPanel(Panel),
    TogglePanel(Panel),
    ClosePanel,
    ShowArchived(bool),
    /// The transcript shows a new page of messages.
    MessagesShown,
    RefreshQuotas,
    /// The host archived or restored these sessions.
    Archived {
        archived: bool,
        affected: Vec<Session>,
    },
    /// Opens the Skills page on one skill.
    OpenSkill(Skill),
    Catalog(Rc<Vec<ModelOption>>),
    /// Shows a change the host confirmed before the snapshot that has it arrives.
    Amend(Amendment),
    /// Opens a creation dialog; `ticket` preselects the ticket when assigning an agent.
    Create {
        kind: Creation,
        ticket: Option<String>,
    },
}

/// A change to the snapshot the host has confirmed.
#[derive(Clone)]
pub(super) enum Amendment {
    Project(Project),
    Projects(Vec<Project>),
    Skill(Box<SkillState>),
}

/// A view that asks the host for things and handles the replies itself.
pub(super) trait Requests: Sized + 'static {
    fn bridge(&self) -> &Bridge;
    /// Handles the host's reply to a command this view sent; the view redraws after it.
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
    /// Archives or restores a session, and says which were.
    fn set_archived(
        &mut self,
        session_id: String,
        archived: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        Self: EventEmitter<Intent>,
    {
        let ask = ask::set_archived(session_id, archived);
        self.ask(
            ask,
            window,
            cx,
            move |_, affected, window, cx| match affected {
                Ok(affected) => cx.emit(Intent::Archived { archived, affected }),
                Err(error) => toast_error(error, window, cx),
            },
        );
    }
    /// Starts or stops a project's agents.
    fn set_live(
        &mut self,
        project_id: String,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = Command::SetLive {
            project_id,
            enabled,
        };
        self.request(command, window, cx);
    }
    /// Sends `command` to the host; its reply comes back to `accept`.
    fn request(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let sent = command.clone();
        send(self, sent, window, cx, move |view, reply, window, cx| {
            view.accept(command, reply, window, cx)
        });
    }
    /// Sends `ask`; its reply, decoded as what the host answers it with, comes back to `then`.
    fn ask<R: DeserializeOwned + 'static>(
        &mut self,
        ask: Ask<R>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<R, String>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let command = ask.command().clone();
        send(self, command, window, cx, move |view, reply, window, cx| {
            then(view, reply.and_then(|reply| ask.decode(reply)), window, cx)
        });
    }
}

/// Sends `command` to the host and hands its reply to `then`; the view redraws after it.
fn send<V: Requests>(
    view: &mut V,
    command: Command,
    window: &mut Window,
    cx: &mut Context<V>,
    then: impl FnOnce(&mut V, Result<Value, String>, &mut Window, &mut Context<V>) + 'static,
) {
    let sent = command.clone();
    let reply = match view.bridge().send(command) {
        Ok(reply) => reply,
        Err(error) => return then(view, Err(error), window, cx),
    };
    cx.spawn_in(window, async move |this, cx| {
        let reply = reply.await;
        let wrote = reply.is_ok();
        let _ = cx.update(|window, cx| {
            let _ = this.update(cx, |view, cx| {
                then(view, reply, window, cx);
                cx.notify();
            });
            replied(&sent, wrote, cx);
        });
    })
    .detach();
}

/// Set after each change the human made, so its observers hear of every one. A reader of an
/// older transcript page goes back to the newest after any of them.
pub(super) struct HumanWrote;
impl Global for HumanWrote {}

/// After a reply to `command`, tells the transcript of a write the human asked for that
/// `succeeded`. Views that send outside `Requests` call it too.
pub(super) fn replied(command: &Command, succeeded: bool, cx: &mut App) {
    if succeeded && human_write(command) {
        cx.set_global(HumanWrote);
    }
}

/// A write the human asked for, unlike the refreshes the app makes by itself.
fn human_write(command: &Command) -> bool {
    !command.is_read()
        && !matches!(
            command,
            Command::RefreshCodexQuota | Command::RefreshRepositories
        )
}

pub(super) fn toast(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::success(message), cx);
}

pub(super) fn toast_error(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    struct RequestFailed;
    window.push_notification(
        Notification::error(message)
            .title("That didn’t work")
            .id::<RequestFailed>(),
        cx,
    );
}
