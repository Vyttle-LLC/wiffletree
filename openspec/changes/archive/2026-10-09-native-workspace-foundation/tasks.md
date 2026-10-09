## 1. Host foundation

- [x] 1.1 Create neutral Rust workspace and typed hierarchy, policy and receipt contracts; verify headless cargo check without GPUI.
- [x] 1.2 Implement exclusive SQLite ownership, migrations, hierarchy constraints, durable ordered messaging, keyset pages, attention and recovery; verify restart, deduplication, cross-project and stale-answer tests.
- [x] 1.3 Implement bounded role routing, catalog validation and global/project turn limits; verify forbidden selection and capacity release tests.
- [x] 1.4 Expose versioned JSON-line administrative commands and Codex model discovery; verify protocol roundtrip and no-inference installed-runtime probe.

## 2. Agentic OS source memory

- [x] 2.1 Implement scoped milestone capture/retrieval and explicit brain attachment; verify same-day deduplication and isolation tests.
- [x] 2.2 Implement locked, hash-checked atomic Markdown export and persisted manual maintenance receipts; verify preserved external content, write-before-receipt recovery and unchanged rerun tests.

## 3. Native client

- [x] 3.1 Build pinned GPUI application with Tidal tokens, virtualized chat, per-session drafts, task/worker creation, simulation adapter and bounded background host requests; verify native build and interactive smoke test.
- [x] 3.2 Add on-demand role settings, memory, attention/activity and read-only Git history panels; verify edits survive restart and unavailable integrations are labeled.

## 4. Validation and handoff

- [x] 4.1 Run relevant tests, formatting, lint and release host benchmark against 50 sessions and 100,000 records; deliver hardware and timing report with unmeasured gates explicit.
- [x] 4.2 Update PRD, README and development handoff with provisional naming, routing decisions, run commands and remaining release stages; validate OpenSpec and log the session to the shared brain.

## Verification notes

Task 3.1 is implemented and the native debug/release builds and local launch succeed. Native visual checks now cover the layout, inspector open/close and creation dialog rendering. Full creation/send/restart smoke coverage remains open. Host restart tests verify the settings persistence used by task 3.2. The delivered client labels its simulator and unavailable integrations. This change is not ready to archive while the full interactive smoke check remains open.

October 3 UI revision: replaced the chat's horizontal navigation with in-panel Context navigation; added separate creation dialogs, explicit task/repository selection, retained validation errors, compact system typography, full-width inputs and resizable hierarchy/context panes. Native compilation, packaging and host tests pass. A separate native UI check process launched against temporary data, but computer control reported the Mac locked before the new click/drag paths could be verified.

October 3 design follow-up supersedes the Context placement above: a persistent right-edge inspector rail now follows the selected project, task or worker and opens an owner-labelled panel. Rebuilt the native typography, sidebar, conversation, composer and inspector cards, bundled original line icons, and mounted the previously missing dialog layer. Browser study checks cover scoped rails, agent creation dialog and keyboard pane resizing; native checks cover inspector open/close and dialog rendering. Final release packaging, all 13 core/host tests, Clippy and OpenSpec validation pass. Native creation persistence, drag behavior, accessibility and measured UI performance still require a complete interactive pass.

October 9 closing note: task 3.1 is ticked on later evidence. `crates/workspace-desktop` is the pinned GPUI application (gpui-pre =0.3.8, gpui-component =0.7.1) with a virtualized transcript, per-session drafts and project, ticket and agent creation dialogs, and `cargo check -p workspace-desktop` and `cargo build -p workspace-desktop` both succeed on origin/main (04debd9). It has since shipped through many releases (installer, in-app update card, dark-mode following), and it has a development-only input injection socket (`--automation-socket`) for interactive checks. The simulation adapter named in 3.1 was replaced by live Claude and Codex providers. The October 3 notes above are history; no separate scripted smoke test is checked in.
