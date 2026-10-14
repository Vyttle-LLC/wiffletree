//! The Processes panel: what the selected agent has left running, or for the coordinator what
//! every agent of the project has, one row per process group with how long it has run, its CPU
//! and Stop. The host reads the process table for it once a second while the panel is open,
//! and never while it is closed.
use super::*;
use gpui_component::Disableable;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use ui::{empty_state, hint};

/// How often the panel asks for fresh running times and CPU while it is open.
const READING_INTERVAL: Duration = Duration::from_secs(1);
/// CPU at or above one full core is highlighted.
const HOT_CPU: f64 = 100.;
const RUNNING_WIDTH: f32 = 64.;
const CPU_WIDTH: f32 = 56.;
const STOP_WIDTH: f32 = 52.;

/// A process's identity: its id and start time.
type Identity = (i32, i64);

#[derive(Default)]
pub(super) struct Processes {
    /// The session whose panel `listing` answers.
    session_id: Option<String>,
    listing: Vec<AgentProcesses>,
    /// When the listing was read, and each process's CPU time then.
    reading: Option<(Instant, HashMap<Identity, u64>)>,
    /// Each group's CPU over the last two readings, by its record id.
    cpu: HashMap<String, f64>,
    /// Why the last reading failed; shown until one succeeds.
    error: Option<String>,
    /// The reading loop is running.
    reading_loop: bool,
    /// Groups whose stop is on its way, by record id; empty for Stop all.
    stopping: Option<Vec<String>>,
}
impl Processes {
    /// Takes a new listing for `session_id`, working out each group's CPU from the change in
    /// its processes' CPU time since the previous reading of the same panel.
    fn accept(&mut self, session_id: &str, listing: Vec<AgentProcesses>, at: Instant) {
        if self.session_id.as_deref() != Some(session_id) {
            self.reading = None;
        }
        let times: HashMap<Identity, u64> = groups(&listing)
            .flat_map(|group| &group.processes)
            .map(|process| ((process.pid, process.started), process.cpu_nanos))
            .collect();
        self.cpu = match &self.reading {
            Some((before, previous)) => groups(&listing)
                .filter_map(|group| {
                    let cpu = cpu_percent(group, previous, at.duration_since(*before))?;
                    Some((group.id.clone(), cpu))
                })
                .collect(),
            None => HashMap::new(),
        };
        self.reading = Some((at, times));
        self.session_id = Some(session_id.to_owned());
        self.listing = listing;
        self.error = None;
    }
}

/// The session to read processes for next, only while the Processes panel is open on one:
/// with it closed the host reads nothing.
fn reading(panel: Option<Panel>, selected: Option<&str>) -> Option<String> {
    selected
        .filter(|_| panel == Some(Panel::Processes))
        .map(str::to_owned)
}

fn groups(listing: &[AgentProcesses]) -> impl Iterator<Item = &ProcessGroup> {
    listing.iter().flat_map(|agent| &agent.groups)
}

/// The group's CPU between two readings `elapsed` apart, as a percentage of one core: the CPU
/// time used by the processes present at both, so one that started or exited in between
/// counts for nothing rather than for its whole life.
fn cpu_percent(
    group: &ProcessGroup,
    previous: &HashMap<Identity, u64>,
    elapsed: Duration,
) -> Option<f64> {
    if !group.confirmed || elapsed.is_zero() {
        return None;
    }
    let used: u64 = group
        .processes
        .iter()
        .filter_map(|process| {
            let before = previous.get(&(process.pid, process.started))?;
            Some(process.cpu_nanos.saturating_sub(*before))
        })
        .sum();
    Some(used as f64 / elapsed.as_nanos() as f64 * 100.)
}

fn cpu_label(cpu: f64) -> String {
    match cpu {
        cpu if cpu < 10. => format!("{cpu:.1}%"),
        cpu => format!("{cpu:.0}%"),
    }
}

/// How long a group has run: "12 s", "41 min", "1 h 16".
fn running_label(started_micros: i64, now_millis: i64) -> String {
    let seconds = (now_millis - started_micros / 1000).max(0) / 1000;
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h {:02}", seconds / 3600, seconds % 3600 / 60),
    }
}

fn started_label(started_micros: i64) -> String {
    chrono::DateTime::from_timestamp_micros(started_micros)
        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// "1 process", "8 processes".
fn process_count(count: usize) -> String {
    match count {
        1 => "1 process".into(),
        many => format!("{many} processes"),
    }
}

/// What the panel's head says: "8 processes running", or "Nothing running".
fn running_count(count: usize) -> String {
    match count {
        0 => "Nothing running".into(),
        count => format!("{} running", process_count(count)),
    }
}

/// The group's programs, the most common first: "yes", or "node, esbuild".
fn programs(group: &ProcessGroup) -> String {
    let mut names: Vec<(&str, usize)> = vec![];
    for process in &group.processes {
        match names.iter_mut().find(|(name, _)| *name == process.program) {
            Some((_, count)) => *count += 1,
            None => names.push((&process.program, 1)),
        }
    }
    names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let names: Vec<&str> = names.into_iter().map(|(name, _)| name).collect();
    names.join(", ")
}

/// The line under a group's programs: "8 processes · started 14:27 · group 43477".
fn group_detail(group: &ProcessGroup) -> String {
    format!(
        "{} · started {} · group {}",
        process_count(group.processes.len()),
        started_label(group.started),
        group.group
    )
}

/// The processes listed for an agent, as its heading counts them.
fn agent_count(agent: &AgentProcesses) -> usize {
    agent.groups.iter().map(|group| group.processes.len()).sum()
}

/// The rail's count for the panel of `session`: what the record holds for that agent, or for
/// a coordinator for every agent of its project that is not archived.
fn rail_count(snapshot: &Snapshot, session: &Session) -> u32 {
    let listed = |id: &str| {
        let Some(agent) = snapshot.sessions.iter().find(|s| s.id == id) else {
            return false;
        };
        match session.role {
            Role::ProjectOrchestrator => agent.project_id == session.project_id && !agent.archived,
            _ => agent.id == session.id,
        }
    };
    snapshot
        .running_processes
        .iter()
        .filter(|count| listed(&count.session_id))
        .map(|count| count.count)
        .sum()
}

impl Workspace {
    /// The count on the rail's Processes entry, if anything runs: the open panel's live count,
    /// or else the record's.
    pub(super) fn processes_badge(&self) -> Option<usize> {
        let (snapshot, session) = (self.snapshot.as_ref()?, self.selected_session()?);
        let count = match self
            .processes()
            .filter(|_| self.panel == Some(Panel::Processes))
        {
            Some(open) => open.listing.iter().map(agent_count).sum(),
            None => rail_count(snapshot, session) as usize,
        };
        (count > 0).then_some(count)
    }

    /// Reads the panel's processes now and then once a second, for as long as the panel is
    /// open on a selected session.
    pub(super) fn sync_process_readings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.processes.reading_loop || self.panel != Some(Panel::Processes) {
            return;
        }
        self.processes.reading_loop = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let next = this.update(cx, |view, _| {
                    let session_id = reading(view.panel, view.selected.as_deref());
                    view.processes.reading_loop = session_id.is_some();
                    Some((session_id?, view.bridge.clone()))
                });
                let Ok(Some((session_id, bridge))) = next else {
                    break;
                };
                let command = Command::Processes {
                    session_id: session_id.clone(),
                };
                let executor = cx.background_executor().clone();
                let result = activity::paced_reply(bridge, command, Duration::ZERO, executor).await;
                let _ = this.update(cx, |view, cx| {
                    view.accept_processes(&session_id, result);
                    cx.notify();
                });
                cx.background_executor().timer(READING_INTERVAL).await;
            }
        })
        .detach();
    }

    fn accept_processes(&mut self, session_id: &str, result: Result<Vec<AgentProcesses>, String>) {
        match result {
            Ok(listing) => self.processes.accept(session_id, listing, Instant::now()),
            Err(error) => {
                eprintln!("Processes: {error}");
                self.processes.error = Some(error);
            }
        }
    }

    /// Stops the named groups of the selected session's panel, or with none named every group
    /// it lists. The reply is the panel's listing after the stop.
    fn stop_processes(&mut self, groups: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected.clone() else {
            return;
        };
        if self.processes.stopping.is_some() {
            return;
        }
        let command = Command::StopProcesses {
            session_id: session_id.clone(),
            groups: groups.clone(),
        };
        self.processes.stopping = Some(groups);
        let reply = activity::paced_reply(
            self.bridge.clone(),
            command,
            Duration::ZERO,
            cx.background_executor().clone(),
        );
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = cx.update(|window, cx| {
                this.update(cx, |view, cx| {
                    view.processes.stopping = None;
                    match result {
                        Ok(listing) => view.processes.accept(&session_id, listing, Instant::now()),
                        Err(error) => view.toast_error(error, window, cx),
                    }
                    cx.notify();
                })
            });
        })
        .detach();
    }

    /// The listing, when it answers for the selected session.
    fn processes(&self) -> Option<&Processes> {
        Some(&self.processes).filter(|p| p.session_id.is_some() && p.session_id == self.selected)
    }

    /// The count and, on one agent's panel, Stop all, pinned above the list.
    pub(super) fn processes_head(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let processes = self.processes()?;
        let count: usize = processes.listing.iter().map(agent_count).sum();
        let agent = self.selected_role() != Role::ProjectOrchestrator;
        // An agent with nothing running has the panel's empty state instead.
        if agent && count == 0 {
            return None;
        }
        let stop_all = agent.then(|| {
            button("processes-stop-all")
                .xsmall()
                .label("Stop all")
                .loading(processes.stopping.as_ref().is_some_and(Vec::is_empty))
                .disabled(processes.stopping.is_some())
                .on_click(cx.listener(|v, _, w, c| v.stop_processes(vec![], w, c)))
        });
        Some(
            div()
                .flex_none()
                .px_5()
                .py_3()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .border_b_1()
                .border_color(p.edge.opacity(0.3))
                .child(hint(running_count(count), p))
                .children(stop_all),
        )
    }

    pub(super) fn processes_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(processes) = self.processes() else {
            return div();
        };
        if let Some(error) = &processes.error {
            return empty_state("terminal", "Processes unavailable", error.clone(), p);
        }
        let rollup = self.selected_role() == Role::ProjectOrchestrator;
        if !rollup
            && processes
                .listing
                .iter()
                .all(|agent| agent.groups.is_empty())
        {
            return empty_state(
                "terminal",
                "Nothing running",
                "Processes this agent starts and leaves running appear here.",
                p,
            );
        }
        let mut body = div().flex().flex_col().child(column_heads(p));
        for agent in &processes.listing {
            if rollup {
                body = body.child(
                    div()
                        .mt_3()
                        .mb_1()
                        .flex()
                        .justify_between()
                        .gap_2()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.subtle)
                        .child(div().min_w_0().text_ellipsis().child(agent.name.clone()))
                        .child(div().flex_none().child(agent_count(agent).to_string())),
                );
            }
            if agent.groups.is_empty() {
                body = body.child(
                    line(p)
                        .py(px(6.))
                        .text_color(p.subtle)
                        .child("Nothing running."),
                );
            }
            for group in &agent.groups {
                body = body.child(self.process_group_row(group, processes, p, cx));
            }
        }
        body
    }

    fn process_group_row(
        &self,
        group: &ProcessGroup,
        processes: &Processes,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        if !group.confirmed {
            return line(p)
                .py(px(6.))
                .flex_col()
                .child(div().text_size(px(12.5)).child(format!("Process group {}", group.group)))
                .child(hint(
                    "Wiffletree can't confirm these are still this agent's processes, so it won't stop them.",
                    p,
                ));
        }
        let cpu = processes.cpu.get(&group.id).copied();
        let hot = cpu.is_some_and(|cpu| cpu >= HOT_CPU);
        let stopping = processes
            .stopping
            .as_ref()
            .is_some_and(|ids| ids.is_empty() || ids.contains(&group.id));
        let id = group.id.clone();
        // The detail runs under the whole row, so it stays readable in a narrow inspector.
        let figures = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(".SystemMonoFont")
                    .text_ellipsis()
                    .child(programs(group)),
            )
            .child(figure(RUNNING_WIDTH, p).child(running_label(group.started, ui::now())))
            .child(
                figure(CPU_WIDTH, p)
                    .when(hot, |d| {
                        d.text_color(p.red).font_weight(FontWeight::SEMIBOLD)
                    })
                    .child(cpu.map_or("—".into(), cpu_label)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(STOP_WIDTH))
                    .flex()
                    .justify_end()
                    .child(
                        button(SharedString::from(format!("processes-stop-{}", group.id)))
                            .xsmall()
                            .label("Stop")
                            .loading(stopping)
                            .disabled(processes.stopping.is_some())
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.stop_processes(vec![id.clone()], w, c)
                            })),
                    ),
            );
        line(p).py(px(6.)).flex_col().child(figures).child(
            div()
                .text_size(px(11.5))
                .text_color(p.subtle)
                .child(group_detail(group)),
        )
    }
}

/// A row of the list, ruled off from the one above.
fn line(p: Palette) -> Div {
    div()
        .w_full()
        .flex()
        .border_t_1()
        .border_color(p.edge.opacity(0.4))
        .text_size(px(12.5))
}

fn figure(width: f32, p: Palette) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .text_right()
        .text_color(p.subtle)
}

fn column_heads(p: Palette) -> Div {
    div()
        .flex()
        .gap(px(10.))
        .pb_1()
        .text_size(px(11.))
        .text_color(p.subtle)
        .child(div().flex_1().child("Program"))
        .child(figure(RUNNING_WIDTH, p).child("Running"))
        .child(figure(CPU_WIDTH, p).child("CPU"))
        .child(div().flex_none().w(px(STOP_WIDTH)))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        Duration, HashMap, Instant, Panel, Processes, cpu_percent, programs, rail_count, reading,
        running_count, running_label,
    };
    use workspace_core::*;

    fn process(pid: i32, program: &str, cpu_nanos: u64) -> GroupProcess {
        GroupProcess {
            pid,
            started: i64::from(pid) * 10,
            program: program.into(),
            cpu_nanos,
        }
    }

    fn group(id: &str, processes: Vec<GroupProcess>) -> ProcessGroup {
        ProcessGroup {
            id: id.into(),
            group: 43477,
            confirmed: true,
            started: 0,
            processes,
        }
    }

    fn listing(groups: Vec<ProcessGroup>) -> Vec<AgentProcesses> {
        vec![AgentProcesses {
            session_id: "tester".into(),
            name: "Tester".into(),
            groups,
        }]
    }

    #[test]
    fn readings_are_asked_for_only_while_the_panel_is_open_on_a_session() {
        assert_eq!(
            reading(Some(Panel::Processes), Some("tester")).as_deref(),
            Some("tester")
        );
        assert_eq!(reading(Some(Panel::Files), Some("tester")), None);
        assert_eq!(reading(None, Some("tester")), None);
        assert_eq!(reading(Some(Panel::Processes), None), None);
    }

    #[test]
    fn cpu_is_the_time_used_between_two_readings_as_a_share_of_one_core() {
        // Eight processes that each used 0.85 s of CPU in one second: 680%, as in the incident.
        let before: HashMap<_, _> = (1..=8)
            .map(|pid| ((pid, i64::from(pid) * 10), 2_000_000_000))
            .collect();
        let now = group(
            "yes",
            (1..=8)
                .map(|pid| process(pid, "yes", 2_850_000_000))
                .collect(),
        );
        let cpu = cpu_percent(&now, &before, Duration::from_secs(1)).unwrap();
        assert!((cpu - 680.).abs() < 1e-9, "{cpu}");

        // A process that was not in the earlier reading counts for nothing, not its whole life;
        // one that has exited simply drops out.
        let joined = group(
            "dev",
            vec![
                process(1, "node", 2_004_000_000),
                process(9, "node", 50_000_000_000),
            ],
        );
        let cpu = cpu_percent(&joined, &before, Duration::from_millis(500)).unwrap();
        assert!((cpu - 0.8).abs() < 1e-9, "{cpu}");

        assert_eq!(cpu_percent(&joined, &before, Duration::ZERO), None);
        let unconfirmed = ProcessGroup {
            confirmed: false,
            ..joined
        };
        assert_eq!(
            cpu_percent(&unconfirmed, &before, Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn the_first_reading_has_no_cpu_and_the_next_one_of_the_same_panel_does() {
        let mut processes = Processes::default();
        let start = Instant::now();
        processes.accept(
            "tester",
            listing(vec![group("yes", vec![process(1, "yes", 0)])]),
            start,
        );
        assert!(processes.cpu.is_empty());
        let later = start + Duration::from_secs(2);
        processes.accept(
            "tester",
            listing(vec![group("yes", vec![process(1, "yes", 1_000_000_000)])]),
            later,
        );
        assert_eq!(processes.cpu.get("yes"), Some(&50.));
        // Another session's panel starts over.
        processes.accept(
            "other",
            listing(vec![group("yes", vec![process(1, "yes", 3_000_000_000)])]),
            later + Duration::from_secs(1),
        );
        assert!(processes.cpu.is_empty());
    }

    #[test]
    fn counts_come_first_and_programs_are_named_as_programs() {
        assert_eq!(running_count(0), "Nothing running");
        assert_eq!(running_count(1), "1 process running");
        assert_eq!(running_count(8), "8 processes running");
        let build = group(
            "build",
            vec![
                process(1, "esbuild", 0),
                process(2, "node", 0),
                process(3, "node", 0),
            ],
        );
        assert_eq!(programs(&build), "node, esbuild");
        let yes = group("yes", (1..=8).map(|pid| process(pid, "yes", 0)).collect());
        assert_eq!(programs(&yes), "yes");
    }

    #[test]
    fn running_time_reads_in_seconds_minutes_then_hours() {
        let started = 1_000_000_000_000_000;
        let at = |seconds: i64| started / 1000 + seconds * 1000;
        assert_eq!(running_label(started, at(12)), "12 s");
        assert_eq!(running_label(started, at(41 * 60 + 5)), "41 min");
        assert_eq!(running_label(started, at(76 * 60)), "1 h 16");
        assert_eq!(running_label(started, at(125 * 60)), "2 h 05");
    }

    #[test]
    fn the_rail_counts_the_agent_or_for_the_coordinator_its_projects_unarchived_agents() {
        let session = |id: &str, project: &str, role: Role, archived: bool| Session {
            id: id.into(),
            project_id: project.into(),
            parent_id: None,
            repository_id: None,
            name: id.into(),
            role,
            provider: Provider::Claude,
            status: Status::Ready,
            archived,
        };
        let coordinator = session("pm", "harbor", Role::ProjectOrchestrator, false);
        let tester = session("tester", "harbor", Role::Tester, false);
        let sessions = vec![
            coordinator.clone(),
            tester.clone(),
            session("old", "harbor", Role::Reviewer, true),
            session("elsewhere", "other", Role::Tester, false),
        ];
        let counts = [("tester", 8), ("pm", 1), ("old", 3), ("elsewhere", 2)].map(|(id, count)| {
            RunningProcesses {
                session_id: id.into(),
                count,
            }
        });
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "projects": [], "repositories": [], "attention": [],
            "sessions": sessions, "running_processes": counts,
        }))
        .unwrap();
        assert_eq!(rail_count(&snapshot, &tester), 8);
        assert_eq!(rail_count(&snapshot, &coordinator), 9);
    }
}
