## 1. Core

- [x] 1.1 Add `RunningProcesses` to the snapshot (serde default), `AgentProcesses`, `ProcessGroup`, `GroupProcess`, `Command::Processes` and `Command::StopProcesses`.

## 2. Host

- [x] 2.1 `agent_processes/panel.rs`: the panel's agents, the listing under the confirmation rule with CPU times, the human's stop and the counts.
- [x] 2.2 `StopReason::StoppedByHuman` with its tag and its occasion in the notice.
- [x] 2.3 `service.rs`: answer `Processes` as a read that signals a change only when it forgot a group.
- [x] 2.4 Tests with real `sleep` processes through the real `Service`: listing for an agent and a coordinator, a row's stop and Stop all with their record, notice and the agent left as it was; an unconfirmed group listed and never signalled; an emptied group forgotten.

## 3. Desktop

- [x] 3.1 `Panel::Processes` beside Files with its count on the rail.
- [x] 3.2 `processes_view.rs`: readings once a second only while open, CPU across two readings, rows, the coordinator's grouping, Stop and Stop all.

## 4. Docs

- [x] 4.1 `docs/PRD.md`, `docs/DEVELOPMENT.md` and the mockup study.
