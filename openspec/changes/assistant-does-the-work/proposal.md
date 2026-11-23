## Why

The Assistant cannot act. Its threads are read-only outside a scratch folder, cannot start agents or tickets, and send every code change through a "Make this a project" brief the human confirms. So almost every request becomes another project, each with a coordinator the human has to talk to and look after. The router, thread tabs, brief cards and recipe cards add steps without adding ability.

The human wants one conversation where they ask questions, start work and see their loops, with no project upkeep. Two requests in the same chat can be two separate pieces of work, and the agents behind them stay out of sight.

## What Changes

- **One conversation across every Space.** The Assistant is one provider session with one chat. It infers each request's Space and asks inline, with one-click choices, only when it cannot tell. **BREAKING:** thread tabs, idle-tab handling, the router model, its confidence threshold and the Router row go away.
- **The Assistant coordinates coordinators.** A request that needs work becomes a **work item**: a Wiffletree project in the inferred Space whose coordinator's parent is the Assistant. Coordinator reports, `ask_user` questions and check-ins go to the Assistant, which answers what it can and brings the human only real decisions, as cards in the chat. Human-only approvals (publishing outside allowed owners, merges after a blocked review, anything a protected branch needs) still reach the human, in the chat as well as the Inbox.
- **It starts work without asking.** Each work card has Stop and Open details. Small code requests take a fast path: one implementer on the small-work line and the lightest review level the Space allows.
- **Assistant work leaves the sidebar's Projects list.** It lives in the Assistant's rail: **Needs you**, **Running**, **Loops** and **Done recently**. Projects lists only what the human started.
- **Follow-ups go to the work they are about,** with a visible tag (`→ Sign-in fix`) and Change. There is no router and no confidence threshold.
- **Real power on this Mac.** The Assistant runs in YOLO mode like a coordinator, with full read and run access to every repository, worktree and project folder: it inspects diffs, runs builds and tests and reads logs in its work, and reads its projects' tickets, reviews and reports directly with `work_status`. Code changes to repositories Wiffletree manages still land through work, by instruction as coordinators are told not to implement, because edits from two places collide with the implementer and reviewed pull requests are the product's guarantee. (Revised by Michael on November 23, 2026: no sandbox.)
- **Direct actions, no confirmation cards.** The Assistant creates, edits, pauses, deletes and runs loops (the UI's name for scheduled jobs) and shows what it did with Undo; it files, edits and comments on issues directly, marked as written by the assistant. **BREAKING:** the recipe card and Make this a project go away. Loops still only read from other apps, sources are still checked before each run, and only a turn the human's own message started can change a loop.
- **A true personal assistant.** The Assistant reaches email, calendar and similar apps through each Space's sources (MCP servers and CLIs the human installed and signed in to); Wiffletree builds no connectors and runs no app sign-in. The Space page makes adding one a single form. Reading is free; anything that sends, posts, creates, changes or deletes in another app shows an action card with its exact target and content and runs on one click, unless the human marked that kind "Do without asking". An instruction inside an email, invite or page never acts by itself: a turn that read others' text, or was not started by the human, can at most raise a card. Loops stay read-only. (Decided by Michael on November 23, 2026.)
- **What stays:** issue threads keep their restricted mode; text written by others is context, never instructions; Boards, the brain and Spaces are unchanged.
- **Migration:** existing threads become read-only history; projects already made from threads stay in Projects; a saved Router row and router threshold are ignored.

## Capabilities

### New Capabilities

None. The work stays inside existing capabilities.

### Modified Capabilities

- `assistant`: one conversation instead of threads in tabs; Space inference; the Assistant's authority and direct reads of its work; apps through Space sources, action cards and text written by others; work items, their cards, routing and Change; decision and approval cards; direct loop and issue actions; the rail; history from earlier threads. Issue threads keep their restricted mode.
- `assistant-router`: removed entirely.
- `scheduled-jobs`: loops are created and changed directly by the Assistant on the human's word, shown with Undo; the UI calls them Loops; the recipe card goes; loops use only sources' read tools, and the pre-run check stays.
- `workspace-host`: the Assistant is a session owned by the install, and the parent of the coordinators of its work; check-ins and `ask_user` of those coordinators go to it; they cannot create projects.
- `model-routing`: the Router row goes.
- `skills`: Assisting serves the Assistant, issue threads and loop runs; the router's instructions go.
- `native-workspace`: the sidebar's Projects list leaves out Assistant work; the Assistant row counts what needs the human.
- `settings-repository`: the router threshold is no longer a shared setting.
- `settings-cascade`: the conversation and the Brain compiler resolve their rows on this Mac only; the Router row leaves the Models page.
- `spaces`: the Assistant may not create or change a Space; sources gain act lists and "Do without asking" marks, and Add source is one form.

## Impact

- `workspace-core`: an Assistant session owner, the work item record (project id, Space, small flag, state), Assistant commands (send, change route, undo), removal of `Job::Router` and the router's answer types.
- `workspace-host`: `assistant.rs` (the conversation session, Assistant tools, decision cards, routing tags, Undo, history), `live.rs` and `mcp.rs` (Assistant tools; reports, `ask_user` and check-ins of Assistant-parented coordinators; `create_project` refusal), `lib.rs` (`create_project_from` gains a sibling that creates work under the Assistant), `provider.rs` (the Assistant's YOLO launch with MCP read tools only), `jobs.rs` (loop changes by the Assistant, Undo), `router.rs` and `provider/router.rs` deleted, `skills.rs` and `skills/core/assistant.md` and `coordinator.md`.
- `workspace-desktop`: `assistant_view` (one chat, work, decision, approval and loop cards, routing tag with Change, the rail, history), `sidebar.rs` (hide Assistant work, badge), `models_view.rs` (no Router row), settings (no threshold).
- Docs: `docs/PRD.md`, `README.md`, `design/README.md`.
- Mockup: `ui/mockups.html`, approved by Michael on November 23, 2026.
