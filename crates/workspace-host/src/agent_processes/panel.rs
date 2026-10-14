//! The Processes panel: what each agent has left running, read from the process table only
//! when the panel asks, and the human's stop of a group from it. A group is listed as the
//! agent's, and stopped, only while a process the host recorded is still alive in it.
use super::*;

impl Host {
    /// The agents a panel opened on `session_id` lists: that agent, or for a coordinator every
    /// agent of its project that is not archived.
    fn panel_agents(&self, session_id: &str) -> Result<Vec<Session>> {
        let session = self.session(session_id)?;
        if session.role != Role::ProjectOrchestrator {
            return Ok(vec![session]);
        }
        let agents = self.sessions()?.into_iter();
        Ok(agents
            .filter(|s| s.project_id == session.project_id && !s.archived)
            .collect())
    }

    /// What the panel lists, as the process table has it now. A group that no longer runs is
    /// forgotten; the second value says whether one was, which changes the snapshot's counts.
    pub(crate) fn running_processes(
        &mut self,
        session_id: &str,
    ) -> Result<(Vec<AgentProcesses>, bool)> {
        let mut forgot = false;
        let mut listing = vec![];
        for agent in self.panel_agents(session_id)? {
            let mut groups = vec![];
            for recorded in self.recorded_processes(Some(&agent.id), true)? {
                match processes::confirmed_members(recorded.group, &recorded.members) {
                    Some(live) => groups.push(confirmed(&recorded, &live)),
                    None if processes::recorded_group_runs(recorded.group) => {
                        groups.push(unconfirmed(&recorded))
                    }
                    None => {
                        self.forget_process_group(&recorded.id)?;
                        forgot = true;
                    }
                }
            }
            listing.push(AgentProcesses {
                session_id: agent.id,
                name: agent.name,
                groups,
            });
        }
        Ok((listing, forgot))
    }

    /// Stops the named groups of the panel opened on `session_id`, or every group it lists,
    /// as `stop_group` allows, and tells each agent what was stopped. A group that has
    /// already ended is passed over. The agents themselves keep working.
    pub(crate) fn stop_processes_by_human(
        &mut self,
        session_id: &str,
        groups: &[String],
    ) -> Result<()> {
        let agents: Vec<String> = self
            .panel_agents(session_id)?
            .into_iter()
            .map(|a| a.id)
            .collect();
        let mut recorded = self.recorded_for(&agents)?;
        if !groups.is_empty() {
            recorded.retain(|recorded| groups.contains(&recorded.id));
        }
        if !recorded.is_empty() {
            let (_, stopped) = self.stop_recorded(recorded, StopReason::StoppedByHuman)?;
            self.notice_stopped(&stopped, StopReason::StoppedByHuman);
        }
        Ok(())
    }

    /// How many processes each agent is recorded as having left running, for the rail's
    /// count. Read from the record alone, so it costs nothing while the panel is closed.
    pub(crate) fn running_process_counts(&self) -> Result<Vec<RunningProcesses>> {
        Ok(self
            .db
            .prepare(
                "SELECT g.session_id,COUNT(*) FROM process_groups g JOIN process_group_members m ON m.group_id=g.id
                 WHERE g.ended_at IS NULL AND g.provider=0 GROUP BY g.session_id",
            )?
            .query_map([], |r| {
                Ok(RunningProcesses {
                    session_id: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

fn confirmed(recorded: &Recorded, live: &[ProcessInfo]) -> ProcessGroup {
    let processes = live
        .iter()
        .map(|process| GroupProcess {
            pid: process.pid,
            started: process.started,
            program: process.command.clone(),
            cpu_nanos: cpu_nanos(process.pid).unwrap_or_default(),
        })
        .collect();
    ProcessGroup {
        id: recorded.id.clone(),
        group: recorded.group,
        confirmed: true,
        started: live.iter().map(|p| p.started).min().unwrap_or_default(),
        processes,
    }
}

/// A group that runs with none of the processes the host recorded in it any more.
fn unconfirmed(recorded: &Recorded) -> ProcessGroup {
    ProcessGroup {
        id: recorded.id.clone(),
        group: recorded.group,
        confirmed: false,
        started: recorded
            .members
            .iter()
            .map(|m| m.1)
            .min()
            .unwrap_or_default(),
        processes: vec![],
    }
}

/// The user and system time a process has used, in nanoseconds.
#[cfg(target_os = "macos")]
fn cpu_nanos(pid: i32) -> Option<u64> {
    use std::{ffi::c_int, sync::LazyLock};
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> c_int;
    }
    // The kernel counts task time in Mach ticks, which are not nanoseconds on Apple silicon.
    static TICK: LazyLock<(u64, u64)> = LazyLock::new(|| {
        let mut base = Timebase { numer: 1, denom: 1 };
        unsafe {
            mach_timebase_info(&raw mut base);
        }
        (u64::from(base.numer), u64::from(base.denom.max(1)))
    });
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as c_int;
    let read =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTASKINFO, 0, (&raw mut info).cast(), size) };
    let (numer, denom) = *TICK;
    let ticks = u128::from(info.pti_total_user + info.pti_total_system);
    (read == size).then(|| (ticks * u128::from(numer) / u128::from(denom)) as u64)
}

#[cfg(not(target_os = "macos"))]
fn cpu_nanos(_pid: i32) -> Option<u64> {
    None
}
