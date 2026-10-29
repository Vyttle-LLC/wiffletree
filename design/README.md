# Design studies

## Current direction

[Open the current mockup](mockups/current/index.html), served locally as described in the root README. Its editable fragment is [source.html](mockups/current/source.html).

The UI leads with the project coordinator's conversation. The native sidebar lists only projects, one row per coordinator; ticket workspaces (“repository · ticket”) and their agents are listed in the Agents panel. The HTML study's tree still shows ticket and agent rows, and it contains simulated examples from the earlier task-orchestrator and repository-coordinator models; the current PRD and native app define the updated workflow. Glyphs distinguish working, blocked, done, and paused; human escalation appears on the project coordinator. A scoped inspector rail stays at the right edge. Selecting a tool opens a panel naming the selected project, task or agent; project selection hides task-only Git tools. Sidebar and inspector widths are adjustable. Context files remain separated by project, task and conversation scope.

While a worker is working, a fixed-height activity card replaces the working indicator. It shows the latest narration line and the three most recent steps. Clicking it opens the turn's full history in a drawer on the right. The chevron minimizes the card to one line that still shows the current step, and the choice carries over to other agents. A finished turn collapses to a “Worked 4m 53s · 12 steps” line above its reply. Open Baseline tests or Profiler to watch a turn run, or theme-init for a finished one. The native app's behavior is specified in `openspec/specs/work-stream`.

Progress bars appear in three places. A strip pinned under the session title shows the project's segmented bar when the coordinator is selected, and an agent's own checklist plus its ticket's figure when a worker is selected. Only the project row in the sidebar carries a thin bar and a percentage. The “7 agents” button on the coordinator's strip opens the Agents panel, which the app opens from every conversation's header instead, and which lists each ticket with its stage and each of its agents with model, checklist count and elapsed time. Striped means working with nothing to count, red means blocked, green means finished. This is a simulation: the checklists, stages, models and times are sample data, and opening a working agent starts its scripted turn, so its bar is striped until that turn's first plan step and then follows the plan steps. The sample project has no accepted or blocked ticket; `openspec/specs/work-progress` specifies every state.

Wiffletree's own notices have a static study, [quiet-notices.html](mockups/current/quiet-notices.html), with no script: today's five full PR watcher messages, the same five folded into one "5 notices" run, and a mixed run of agent messages and notices, opened. `openspec/specs/work-stream` specifies what the native app implements.

Project order and groups have their own static study, [project-groups.html](mockups/current/project-groups.html), with no script. It shows today's newest-first list, the same nine projects grouped by the first word of their names, and dragging: a project inside its group, a whole group among the other rows, and the flat list with the Group button off. Its “How it behaves” list is what the native app implements; `openspec/specs/native-workspace` and `openspec/specs/workspace-host` specify it. The interactive study's sidebar still shows the earlier fixed list.

Remote control has its own static study, [remote-hosts/ui/mockups.html](../openspec/changes/archive/2026-10-20-remote-hosts/ui/mockups.html), with no script: the sidebar's host switcher, Settings › Remote access on the host with a pairing code, Connect a computer, a remote project connected and reconnecting, the quit prompt, a later iPhone view, and whose settings a Mac driving another shows. The human approved it on October 19, 2026. `openspec/changes/archive/2026-10-19-standalone-host` and `openspec/changes/archive/2026-10-20-remote-hosts` specify what the native app implements.

Shared issues have their own static study, [shared-issues/ui/mockups.html](../openspec/changes/shared-issues/ui/mockups.html), with no script: the Issues page as a list and a board, one issue with the issue assistant beside it, New issue with its warning, the Inbox with an issue paused by a conflict, and setting up the tracker repository with what it holds on disk. The human approved it on October 29, 2026. Its people, repositories and issues are invented Celadon samples. `openspec/changes/shared-issues` specifies what the native app will implement.

The assistant, Spaces, scheduled jobs and Boards have a proposed static study, [assistant-spaces-jobs-boards/ui/mockups.html](../openspec/changes/assistant-spaces-jobs-boards/ui/mockups.html), with no script: the assistant with thread tabs in light and dark, the router's inline "Is this Sage or Personal?" question, a job recipe to confirm, the Boards library, an open HTML board and a Space's settings page. It awaits the human's approval. `openspec/changes/assistant-spaces-jobs-boards` specifies the proposal.

The team-shared second brain has its own study, [brains.html](mockups/current/brains.html). It is the approval gate before any OpenSpec change for the brain. Unlike the other standalone studies it has an inline script and still no network: its CSP allows only the page's own style and script. Clicking reaches the Workspace › Brains page with the compiler host, the PR gate and the repository → brain mapping; the brain browser's Status, Wiki, Logs and Conflicts views; a project's Memory and Inbox panels, with one failed sensitive-information scan; and ⌘K search. The human approved it on October 29, 2026; the decisions on its open questions are listed at the top of the file, and the `brain-registry-sync`, `brain-gate`, `brain-log-capture`, `brain-compile` and `brain-browser` OpenSpec changes specify what the native app implements. The repository mapping shown in the study is superseded by Spaces (Michael, 2026-10-29).

**Wiffletree** is the product name, selected October 5, 2026; **Tidal** remains the selected brand color palette as of October 2, 2026. The current study uses Wiffletree branding and paired Tidal light and dark modes. Celadon remains its sample project; it is no longer the product's theme family. See [Brand colors](brand.md) for the palette, semantic roles, and usage rules. The [brand kit](brand/README.md) pairs the selected red Interlock open-joint symbol with the original two-tone wordmark; [preview both appearances](brand/png/preview.png).

## Prototype boundaries

- Chat submission, permissions, Git actions, PR data, timers, and usage are local simulations.
- Activity cards and turn histories replay scripted sample steps; they read no provider output.
- Progress bars are computed in the page from sample checklists and stages; no host derives them and nothing is read from an agent.
- Context selection and sent file-reference chips are interactive. File imports retain only names and sizes in the session; no durable upload or provider delivery is implemented.
- Some presentation choices and drafts persist in browser storage. Sample conversations, created agents, approvals, and imported file metadata may reset on reload.
- The file-picker flow was not verified end to end in the sandboxed in-app browser. Selection of sample context files, sent references, task/agent creation, and approval routing were exercised.
- The responsive layout was checked at desktop and 320 px widths. The application is intended for a desktop-first native implementation.
- The project order and groups study is static: its project names, figures, states and drag positions are drawn by hand, and nothing in it moves.
- The remote control study is static: its hosts, devices, codes, states and figures are drawn by hand, and nothing in it connects, pairs or reaches the network.
- The shared issues study is static: its issues, people, states and sync times are drawn by hand, and nothing in it reads or writes a repository.
- The assistant, Spaces, jobs and Boards study is static: its Spaces, threads, jobs, outputs and figures are drawn by hand, its dashboard is drawn with CSS, and nothing in it sends a message, asks a router, schedules a job or touches a repository.
- The brains study's brains, entries, commits, PRs, people and times are sample data. Rulings, redaction and remapping change only the page's state and reset on reload; nothing reads Git, GitHub or a provider.
- These are HTML studies, not the GPUI implementation or evidence of native performance.

## Earlier studies

These preserve the design progression and may contain superseded repository-first layouts or palettes.

| Study | Browser-ready version | Editable source |
| --- | --- | --- |
| Initial agent workspace | [Preview](mockups/archive/agent-workspace/index.html) | [Source](mockups/archive/agent-workspace/source.html) |
| Git and PR workspace | [Preview](mockups/archive/agent-git-workspace/index.html) | [Source](mockups/archive/agent-git-workspace/source.html) |
| Project studio layouts | [Preview](mockups/archive/project-studio/index.html) | [Source](mockups/archive/project-studio/source.html) |
| Earlier color study | [Preview](mockups/archive/project-studio-theme/index.html) | [Source](mockups/archive/project-studio-theme/source.html) |
| Celadon overview study | [Preview](mockups/archive/celadon-studio/index.html) | [Source](mockups/archive/celadon-studio/source.html) |

## References and provenance

- `themes/wiffletree-tidal.json` is the canonical, versioned Tidal token file. The mockup builder embeds its paired light and dark variants in the current study's marked palette block; edit the JSON rather than its generated copy.
- The Celadon, Powder, Jade, and Sky JSON snapshots in `themes/` support historical studies, from [Celadon Theme](https://celadontheme.com/palette) and its [JSON ports](https://github.com/celadon-theme/celadon-theme/tree/main/ports/json). They do not define the current product colors.
- Tidal follows the user-selected stone, deep teal, and rust reference from the branding discussion. The semantic variants adapt it for readable light and dark interfaces; see `brand.md`.
- `runtime/` preserves the standalone preview wrapper exported from the conversation, including its sandbox, CSP, state bridge, icon and tooltip utilities. It is preview support only, not proposed application infrastructure.
- The initial PRD and studies were migrated on October 2, 2026 from the original planning page. Future requirements live in this repository.

Run `python3 scripts/build_mockups.py` after editing a source fragment. This only rebuilds local HTML exports; it does not deploy a website.
