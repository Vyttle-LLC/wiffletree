## Why

Once logs, sync and compile run through Wiffletree, the human and the agents still need to read the brain. Today the human opens the brain in Obsidian and agents read files with the `brain-query` skill, from one machine-wide pointer. Wiffletree's Memory panel shows only a project's own captured rows and a capture form. Nothing shows the wiki, its sources, the conflicts or what the PR gate is doing, and nothing searches.

Michael approved the Brains page and brain browser in `design/mockups/current/brains.html` on October 29, 2026. He decided on one workspace Brain page filtered by project, with the project's Memory panel as the summary that opens it, and on search that never spans organizations.

## What Changes

- **The brain browser**, opened from a brain on the Brains page (whole brain) and from a project's Memory panel (filtered to that project's repositories). It has scope chips for this project or the whole brain, a brain switcher, and four views:
  - **Status:** the rendered `STATUS.md`, with the last compile time, the next run, pending entries and the brain's open PRs.
  - **Wiki:** the note index tree with kind markers, and the rendered note with a `kind` badge, clickable `[[wikilinks]]` and backlinks. A Sources list per claim links each claim to its log entries, then to the commits, PRs and tags, marked verified, claimed or unverified.
  - **Logs:** a paginated timeline with filters for repository, type, author and verification. Each entry shows the PR that added it (open, auto-merging, merged, failed check) and, for entries this machine wrote, its ticket and conversation.
  - **Conflicts:** the conflict cards with evidence and the four rulings from `brain-compile`.
- **The Memory panel becomes the project's summary.** It shows:
  - the project's brain and its sync state;
  - the project's batches with their stages;
  - open conflicts, linking to the Inbox;
  - recent entries and unsent rows;
  - the capture form;
  - Open brain browser and Search.
- **Search:** a ⌘K quick-open field over notes and log entries, also opened with Ctrl+K and `/`. Results are ranked excerpts with matched terms highlighted, decisions above observations, with scope chips for this project or the whole brain. It is keyboard-reachable throughout. It searches one brain at a time and never spans organizations.
- **An incremental SQLite FTS5 index** per brain over `main` of the host's checkout. It is updated after each fetch by file hash, rebuildable, and does no work while idle.
- **A `query_brain` MCP tool** for agents. It follows the cheapest-first ladder (STATUS, INDEX, notes, logs) over the same index. It returns compact excerpts with paths, kind, verification and freshness, and says so plainly when the brain has nothing. It replaces direct file reads by `brain-query` inside Wiffletree.

## Capabilities

### New Capabilities
- `brain-browser`: the browser's views and scopes, the Memory panel summary, search and its index, and the `query_brain` tool.

### Modified Capabilities
<!-- None. -->

## Impact

- `crates/workspace-host/src/brain_index.rs` and `brain_index.sql` (new): the FTS5 index, incremental by file hash, and the read API for the views (paginated logs, notes, backlinks, footnote sources, conflicts).
- `crates/workspace-host/src/mcp.rs`: `query_brain`.
- `crates/workspace-host/src/verify_refs.rs` (from `brain-compile`): reused to show verification.
- `crates/workspace-desktop`: the brain browser page; the reworked `memory_view.rs`; the ⌘K search overlay; Markdown rendering of notes, with wikilinks and footnotes.
- The fixed core instructions mention `query_brain` for the coordinator and workers.
- Performance: the PRD's fixture of 100,000 entries and 10,000 notes, with a cached first page within the 100 ms UI target.
- `docs/PRD.md`: Memory interface and performance.
