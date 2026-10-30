//! Quitting beside a launchd host, which keeps its agents working after the app exits (design §7
//! of the standalone-host change). Quitting with agents working asks first; keeping them running,
//! or quitting with none working, closes the windows and leaves the Dock, and the menu bar icon
//! shows how many work. Only Quit Wiffletree from the icon, or Stop agents and quit, ends the
//! app, and only then does a staged update install. A child host stops with the app, so the app
//! simply quits.
use crate::{bridge::Bridge, menu_bar, native_quit, update};
use gpui::*;
use std::{rc::Rc, time::Duration};
use workspace_core::{Snapshot, Status, ask};

/// How long quitting waits to hear how many agents work before it just quits. Tests wait longer,
/// so a host slowed by heavy load still answers.
const COUNT_WAIT: Duration = Duration::from_secs(if cfg!(test) { 30 } else { 2 });
/// How long Stop agents and quit waits for the host, which answers once its turns have stopped.
const STOP_WAIT: Duration = Duration::from_secs(60);

/// What quitting does.
#[derive(Debug, PartialEq, Eq)]
enum Quit {
    /// Close the windows and leave the Dock, with the host and its turns still running.
    Background,
    /// Agents are working: ask first.
    Ask(QuitPrompt),
    /// End the app, after stopping every agent when `stop_agents`.
    Exit { stop_agents: bool },
}
impl Quit {
    /// What ⌘Q and the app menu's Quit do, with `working` agents in a turn.
    fn requested(outlives_app: bool, working: usize) -> Self {
        match (outlives_app, working) {
            (false, _) => Self::Exit { stop_agents: false },
            (true, 0) => Self::Background,
            (true, working) => Self::Ask(QuitPrompt { working }),
        }
    }
}

/// The question before quitting with agents working.
#[derive(Debug, PartialEq, Eq)]
struct QuitPrompt {
    working: usize,
}
impl QuitPrompt {
    const TITLE: &str = "Quit Wiffletree?";
    /// The first is the default, which Return chooses.
    const CHOICES: [&str; 2] = ["Keep them running", "Stop agents and quit"];
    fn detail(&self) -> String {
        let working = match self.working {
            1 => "1 agent is".to_string(),
            n => format!("{n} agents are"),
        };
        format!(
            "{working} working on this Mac. They can keep going after the window closes, and you can check in from your other devices."
        )
    }
    /// What the button at `index` of `CHOICES` does.
    fn choose(index: usize) -> Quit {
        match index {
            0 => Quit::Background,
            _ => Quit::Exit { stop_agents: true },
        }
    }
}

/// How many agents are in a turn.
pub(crate) fn working(snapshot: &Snapshot) -> usize {
    (snapshot.sessions.iter())
        .filter(|s| s.status == Status::Working)
        .count()
}

/// A downloaded release, kept by the app rather than a window so it survives closing into the
/// background, and installed only when the app exits.
#[derive(Default)]
pub(crate) struct PendingUpdate(pub(crate) Option<update::Staged>);
impl Global for PendingUpdate {}

/// What ending the app does: it disconnects, which stops a child host and leaves a launchd host
/// running, and hands a staged update to `install`.
pub(crate) fn exiting(
    bridge: &Bridge,
    pending: &mut PendingUpdate,
    install: impl FnOnce(&update::Staged),
) {
    bridge.shutdown();
    if let Some(staged) = pending.0.take() {
        install(&staged);
    }
}

/// What ⌘Q does beside `bridge`'s host, asking the host how many agents work. A host that is out
/// of reach, refuses the app or does not answer within `COUNT_WAIT` cannot say, so the app just
/// quits, which leaves a launchd host and its turns running. Blocks; run it off the main thread.
fn plan_quit(bridge: &Bridge) -> Quit {
    if !bridge.outlives_app() {
        return Quit::Exit { stop_agents: false };
    }
    match bridge.ask_within(ask::snapshot(), COUNT_WAIT) {
        Ok(snapshot) => Quit::requested(true, working(&snapshot)),
        Err(_) => Quit::Exit { stop_agents: false },
    }
}

/// Runs `blocking` on a thread of its own and hands its result back.
fn off_main_thread<T: Send + 'static>(
    blocking: impl FnOnce() -> T + Send + 'static,
) -> async_channel::Receiver<T> {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(blocking());
    });
    receiver
}

/// The app's way of quitting, and the menu bar icon while it runs in the background.
struct Lifecycle {
    bridge: Bridge,
    open_window: Rc<dyn Fn(&mut App)>,
    icon: Option<menu_bar::Icon>,
    choices: async_channel::Sender<menu_bar::Choice>,
    /// A quit is being decided: from asking the host how many agents work until the app acts
    /// on it, the human's answer to the prompt included.
    deciding: bool,
}
impl Global for Lifecycle {}

/// What acting on a quit does to the app. The app does it; tests stand in for it.
trait Quitter {
    /// Whether a quit is being decided, so another, such as the Dock's Quit again while the
    /// prompt shows, does nothing.
    fn deciding(&self) -> bool;
    fn set_deciding(&mut self, deciding: bool);
    /// Closes every window and leaves the Dock; the menu bar icon stays.
    fn enter_background(&mut self);
    /// Asks the human, whose answer comes back through `answered`.
    fn ask_first(&mut self, prompt: QuitPrompt);
    /// Ends the app, after the host stops every agent when `stop_agents`.
    fn exit(&mut self, stop_agents: bool);
}

/// A quit was asked for: whether to decide it, rather than ignore it while one is decided.
fn quit_requested(app: &mut impl Quitter) -> bool {
    !app.deciding() && {
        app.set_deciding(true);
        true
    }
}

/// Acts on a decided quit. Asking keeps the quit being decided until the human answers, and
/// exiting until the app ends.
fn act(plan: Quit, app: &mut impl Quitter) {
    match plan {
        Quit::Background => {
            app.set_deciding(false);
            app.enter_background();
        }
        Quit::Exit { stop_agents } => app.exit(stop_agents),
        Quit::Ask(prompt) => app.ask_first(prompt),
    }
}

/// The human answered the prompt with the choice at `index`, or it closed without an answer.
fn answered(index: Option<usize>, app: &mut impl Quitter) {
    match index {
        Some(index) => act(QuitPrompt::choose(index), app),
        None => app.set_deciding(false),
    }
}

impl Quitter for App {
    fn deciding(&self) -> bool {
        self.global::<Lifecycle>().deciding
    }
    fn set_deciding(&mut self, deciding: bool) {
        self.global_mut::<Lifecycle>().deciding = deciding;
    }
    fn enter_background(&mut self) {
        enter_background(self);
    }
    fn ask_first(&mut self, prompt: QuitPrompt) {
        ask_first(prompt, self);
    }
    fn exit(&mut self, stop_agents: bool) {
        exit(stop_agents, self);
    }
}

/// Sets up quitting for `bridge`'s host. `open_window` opens the main window again.
pub(crate) fn install(bridge: Bridge, open_window: impl Fn(&mut App) + 'static, cx: &mut App) {
    let (choices, chosen) = async_channel::unbounded();
    cx.set_global(PendingUpdate::default());
    cx.set_global(Lifecycle {
        bridge: bridge.clone(),
        open_window: Rc::new(open_window),
        icon: None,
        choices,
        deciding: false,
    });
    cx.on_action(|_: &crate::Quit, cx| quit(cx));
    // The Dock's Quit, and other quits from macOS, decide as ⌘Q does.
    let (requests, requested) = async_channel::unbounded();
    native_quit::intercept(requests);
    cx.spawn(async move |cx| {
        while requested.recv().await.is_ok() {
            cx.update(quit);
        }
    })
    .detach();
    // Quitting exits the process without dropping the bridge, so a child host is stopped here,
    // with its running turns, before it could outlive the app.
    cx.on_app_quit(|cx| {
        let bridge = cx.global::<Lifecycle>().bridge.clone();
        exiting(&bridge, cx.global_mut::<PendingUpdate>(), |staged| {
            let _ = staged.install_after_exit(false);
        });
        async {}
    })
    .detach();
    if !bridge.outlives_app() {
        return;
    }
    // Closing the last window leaves the Dock, as Keep them running does.
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            enter_background(cx);
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Ok(choice) = chosen.recv().await {
            cx.update(|cx| match choice {
                menu_bar::Choice::Open => come_forward(cx),
                menu_bar::Choice::StopAgentsAndQuit => exit(true, cx),
                menu_bar::Choice::Quit => exit(false, cx),
            });
        }
    })
    .detach();
    if let Some(changes) = bridge.menu_bar_changes() {
        cx.spawn(async move |cx| {
            while changes.recv().await.is_ok() {
                cx.update(count_working);
            }
        })
        .detach();
    }
}

/// ⌘Q: quits as `plan_quit` says, unless a quit is already being decided.
fn quit(cx: &mut App) {
    if !quit_requested(cx) {
        return;
    }
    let bridge = cx.global::<Lifecycle>().bridge.clone();
    let plan = off_main_thread(move || plan_quit(&bridge));
    cx.spawn(async move |cx| {
        if let Ok(plan) = plan.recv().await {
            cx.update(|cx| act(plan, cx));
        }
    })
    .detach();
}

/// Opening the app while it runs, from the Finder or the Dock, brings it back as Open
/// Wiffletree does.
pub(crate) fn reopened(cx: &mut App) {
    if cx.has_global::<Lifecycle>() {
        come_forward(cx);
    }
}

fn ask_first(prompt: QuitPrompt, cx: &mut App) {
    let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
        return act(Quit::Background, cx);
    };
    let answer = window.update(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Info,
            QuitPrompt::TITLE,
            Some(&prompt.detail()),
            &QuitPrompt::CHOICES,
            cx,
        )
    });
    let Ok(answer) = answer else {
        return answered(None, cx);
    };
    cx.spawn(async move |cx| {
        let index = answer.await.ok();
        cx.update(|cx| answered(index, cx));
    })
    .detach();
}

/// Closes every window and leaves the Dock; the menu bar icon stays.
fn enter_background(cx: &mut App) {
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
    cx.set_activation_policy(ActivationPolicy::Accessory);
    let lifecycle = cx.global_mut::<Lifecycle>();
    if lifecycle.icon.is_none() {
        lifecycle.icon = Some(menu_bar::Icon::show(lifecycle.choices.clone()));
    }
    count_working(cx);
}

/// Open Wiffletree: back in the Dock with the main window, and no icon.
fn come_forward(cx: &mut App) {
    let lifecycle = cx.global_mut::<Lifecycle>();
    lifecycle.icon = None;
    let open_window = lifecycle.open_window.clone();
    cx.set_activation_policy(ActivationPolicy::Regular);
    if cx.windows().is_empty() {
        open_window(cx);
    }
    cx.activate(true);
}

/// Ends the app; with `stop_agents`, once the host has stopped them and answered, or at once
/// when it is out of reach or refuses the app, since then it cannot stop them.
fn exit(stop_agents: bool, cx: &mut App) {
    if !stop_agents {
        return end_app(cx);
    }
    let bridge = cx.global::<Lifecycle>().bridge.clone();
    let stopped =
        off_main_thread(move || bridge.ask_within(ask::stop_agents_and_quit(), STOP_WAIT));
    cx.spawn(async move |cx| {
        if let Ok(Err(error)) = stopped.recv().await {
            eprintln!("Stop agents and quit: {error}");
        }
        cx.update(end_app);
    })
    .detach();
}

/// Ends the app process, which no quit decision holds back.
pub(crate) fn end_app(cx: &mut App) {
    native_quit::allow();
    cx.quit();
}

/// Shows on the menu bar icon, while there is one, how many agents work.
fn count_working(cx: &mut App) {
    if cx.global::<Lifecycle>().icon.is_none() {
        return;
    }
    let Ok(snapshot) = cx.global::<Lifecycle>().bridge.ask(ask::snapshot()) else {
        return;
    };
    cx.spawn(async move |cx| {
        if let Ok(snapshot) = snapshot.await {
            cx.update(|cx| {
                if let Some(icon) = &cx.global::<Lifecycle>().icon {
                    icon.set_working(working(&snapshot));
                }
            });
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    // Not a glob: gpui's `test` attribute would shadow the standard one.
    use super::{
        Bridge, PendingUpdate, Quit, QuitPrompt, Quitter, act, answered, exiting, menu_bar,
        plan_quit, quit_requested, update, working,
    };
    use crate::bridge::tests::serve;
    use std::{
        os::unix::fs::PermissionsExt as _,
        path::Path,
        time::{Duration, Instant},
    };
    use workspace_core::*;

    #[test]
    fn with_agents_working_beside_a_launchd_host_quitting_asks_and_keeps_them_by_default() {
        let Quit::Ask(prompt) = Quit::requested(true, 2) else {
            panic!("asks first");
        };
        assert_eq!(QuitPrompt::TITLE, "Quit Wiffletree?");
        assert_eq!(
            prompt.detail(),
            "2 agents are working on this Mac. They can keep going after the window closes, and you can check in from your other devices."
        );
        assert_eq!(QuitPrompt::CHOICES[0], "Keep them running", "the default");
        assert_eq!(QuitPrompt::choose(0), Quit::Background);
        assert_eq!(QuitPrompt::CHOICES[1], "Stop agents and quit");
        assert_eq!(QuitPrompt::choose(1), Quit::Exit { stop_agents: true });
        assert!(
            QuitPrompt { working: 1 }
                .detail()
                .starts_with("1 agent is working")
        );
    }

    /// A launchd host at `socket`, as far as the bridge can tell.
    struct Launchd(std::path::PathBuf);
    impl Launch for Launchd {
        fn endpoint(&mut self, _: &std::sync::atomic::AtomicBool) -> std::io::Result<Endpoint> {
            Ok(Endpoint {
                socket: self.0.clone(),
                credential: None,
            })
        }
        fn outlives_app(&self) -> bool {
            true
        }
    }

    /// An app that records what quitting did to it, with a staged update that only exiting
    /// installs.
    #[derive(Default)]
    struct Recorded {
        deciding: bool,
        prompts: usize,
        backgrounds: usize,
        exits: Vec<bool>,
        pending: PendingUpdate,
        installed: Option<String>,
    }
    impl Quitter for Recorded {
        fn deciding(&self) -> bool {
            self.deciding
        }
        fn set_deciding(&mut self, deciding: bool) {
            self.deciding = deciding;
        }
        fn enter_background(&mut self) {
            self.backgrounds += 1;
        }
        fn ask_first(&mut self, _: QuitPrompt) {
            self.prompts += 1;
        }
        fn exit(&mut self, stop_agents: bool) {
            self.exits.push(stop_agents);
            if let Some(staged) = self.pending.0.take() {
                self.installed = Some(staged.version);
            }
        }
    }

    #[test]
    fn quitting_with_nothing_working_goes_to_the_background_stops_nothing_and_keeps_the_update() {
        let served = serve(|_| {});
        let bridge = Bridge::connect(Box::new(Launchd(served.data.path().join(SOCKET_FILE))));
        let mut app = Recorded {
            pending: PendingUpdate(Some(update::Staged::fake("9.9.9"))),
            ..Recorded::default()
        };

        assert!(quit_requested(&mut app));
        act(plan_quit(&bridge), &mut app);

        assert_eq!((app.backgrounds, app.prompts), (1, 0));
        assert!(app.exits.is_empty(), "no exit, so no stop: {:?}", app.exits);
        assert!(
            app.pending.0.is_some() && app.installed.is_none(),
            "still staged"
        );
        assert!(!app.deciding, "the next quit is decided again");
        bridge.shutdown();
    }

    #[test]
    fn a_quit_while_the_prompt_shows_does_nothing_until_it_is_answered() {
        let mut app = Recorded::default();
        assert!(quit_requested(&mut app));
        act(Quit::requested(true, 2), &mut app);
        assert_eq!(app.prompts, 1);

        assert!(!quit_requested(&mut app), "the Dock's Quit again");
        assert_eq!(app.prompts, 1, "no second prompt");

        answered(None, &mut app);
        assert!(
            quit_requested(&mut app),
            "a closed prompt lets the next quit ask"
        );
        act(Quit::requested(true, 2), &mut app);
        answered(Some(0), &mut app);
        assert_eq!((app.prompts, app.backgrounds), (2, 1), "Keep them running");
        assert!(app.exits.is_empty());
        assert!(quit_requested(&mut app));
        act(Quit::requested(true, 2), &mut app);
        answered(Some(1), &mut app);
        assert_eq!(app.exits, [true], "Stop agents and quit");
        assert!(!quit_requested(&mut app), "nothing more while the app ends");
    }

    #[test]
    fn beside_a_host_that_refuses_the_app_quitting_and_stopping_agents_end_at_once() {
        use std::io::{BufRead, Write};
        let folder = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let socket = folder.path().join(SOCKET_FILE);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let _ = reader.read_line(&mut line);
                let refusal = serde_json::json!({"refused": {
                    "host_protocols": [PROTOCOL_VERSION + 1, PROTOCOL_VERSION + 1],
                    "client_protocols": [PROTOCOL_VERSION, PROTOCOL_VERSION],
                    "host_version": "9.0.0", "working": 0}});
                let _ = writeln!(&stream, "{refusal}");
            }
        });
        let bridge = Bridge::connect(Box::new(Launchd(socket)));
        assert!(bridge.refusal().is_some());

        let asked = Instant::now();
        assert_eq!(plan_quit(&bridge), Quit::Exit { stop_agents: false });
        let stop = bridge.ask_within(ask::stop_agents_and_quit(), Duration::from_secs(60));
        assert!(stop.is_err(), "nothing can stop the agents");
        assert!(
            asked.elapsed() < Duration::from_secs(1),
            "{:?}",
            asked.elapsed()
        );
        bridge.shutdown();
    }

    /// The host binary `cargo` built beside this test.
    fn host_binary() -> std::path::PathBuf {
        let exe = std::env::current_exe().unwrap();
        let binary = exe
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("workspace-host");
        assert!(binary.is_file(), "build workspace-host first: {binary:?}");
        binary
    }

    /// A login shell that only runs what it is given, so the host's start is quick and sees the
    /// environment the test sets.
    fn plain_shell(folder: &Path) -> std::path::PathBuf {
        let shell = folder.join("shell");
        std::fs::write(&shell, "#!/bin/sh\neval \"$4\"\n").unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        shell
    }

    #[test]
    fn a_child_host_quits_without_asking_and_stops_with_the_app() {
        assert_eq!(Quit::requested(false, 3), Quit::Exit { stop_agents: false });
        let data = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let launcher = ChildLaunch::new(&host_binary(), data.path())
            .env("SHELL", plain_shell(scratch.path()))
            .env("WORKSPACE_CLAUDE_BIN", "/nonexistent/claude")
            .env("WORKSPACE_CODEX_BIN", "/nonexistent/codex");
        let bridge = Bridge::connect(Box::new(launcher));
        assert!(!bridge.outlives_app());
        let host = bridge.with_child(|child| child.id()).unwrap();

        exiting(&bridge, &mut PendingUpdate::default(), |_| {});

        // SAFETY: probing a process id with signal 0 sends nothing.
        assert_ne!(unsafe { libc::kill(host as i32, 0) }, 0, "the child exited");
    }

    #[test]
    fn quitting_the_app_leaves_a_launchd_host_running_and_installs_the_staged_update() {
        let served = serve(|_| {});
        let mut pending = PendingUpdate(Some(update::Staged::fake("9.9.9")));
        let mut installed = None;

        exiting(&served.bridge, &mut pending, |staged| {
            installed = Some(staged.version.clone())
        });

        assert_eq!(installed.as_deref(), Some("9.9.9"));
        assert!(pending.0.is_none());
        let again = served.reconnect();
        assert!(
            again.ask_now(ask::snapshot()).is_ok(),
            "the host still runs"
        );
    }

    /// A provider whose turn runs until `$WIFFLETREE_TEST_RELEASE` exists, then finishes.
    const WAITING_PROVIDER: &str = r#"#!/bin/sh
case "$1" in -p|exec) ;; *) exit 0;; esac
cat >/dev/null
echo '{"type":"system","subtype":"init","session_id":"s","mcp_servers":[{"name":"agent_workspace","status":"connected"}]}'
while [ ! -e "$WIFFLETREE_TEST_RELEASE" ]; do sleep 0.05; done
echo '{"type":"result","is_error":false,"result":"DONE","usage":{}}'
"#;

    fn badge_until(bridge: &Bridge, what: &str, expected: Option<&str>) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let snapshot = bridge.ask_now(ask::snapshot());
            let badge = snapshot.map(|s| menu_bar::badge(working(&s)));
            if badge.as_ref().is_ok_and(|b| b.as_deref() == expected) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {what}, {badge:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn the_icon_counts_a_turn_a_timer_started_and_goes_quiet_when_it_ends() {
        let data = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let provider = scratch.path().join("provider");
        std::fs::write(&provider, WAITING_PROVIDER).unwrap();
        std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
        let release = scratch.path().join("release");
        {
            let mut host = workspace_host::Host::open_set_up(data.path()).unwrap();
            let project = host.create_project("Timers").unwrap();
            let main = (host.sessions().unwrap().into_iter())
                .find(|s| s.project_id == project.id)
                .unwrap();
            host.set_live(&project.id, true).unwrap();
            let timer = serde_json::json!({"label":"Check","prompt":"Check in","at":"+1h"});
            host.agent_tool(&main.id, "schedule", timer).unwrap();
        }
        // The timer came due while no host ran, so it fires as the host starts.
        let store = rusqlite::Connection::open(data.path().join("workspace.sqlite3")).unwrap();
        store
            .execute("UPDATE schedules SET next_fire_at=1", [])
            .unwrap();
        drop(store);
        let launcher = ChildLaunch::new(&host_binary(), data.path())
            .env("SHELL", plain_shell(scratch.path()))
            .env("WORKSPACE_CLAUDE_BIN", &provider)
            .env("WORKSPACE_CODEX_BIN", &provider)
            .env("WIFFLETREE_TEST_RELEASE", &release);
        let bridge = Bridge::connect(Box::new(launcher));
        assert_eq!(menu_bar::badge(0), None, "quiet with nothing working");

        badge_until(&bridge, "the timer's turn", Some("1"));
        std::fs::write(&release, "").unwrap();
        badge_until(&bridge, "the turn to end", None);
        bridge.shutdown();
    }
}
