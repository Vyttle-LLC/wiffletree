## Context

After the other three brain changes:
- each brain has a host-owned checkout whose `main` is fetched after every merge (`brain-registry-sync`);
- entries are one file each under `logs/<repo>/YYYY/MM/`, and this machine's own entries also have rows in `work_logs` with their ticket, session, provider and machine (`brain-log-capture`);
- notes carry `title`, `kind`, `updated` and `sources` frontmatter, cite each claim with Markdown footnotes linking to entry files, and `reconcile-queue.md` exports the open conflicts (`brain-compile`);
- ref verification is a shared host function (`verify_refs.rs`).

The approved design study is `design/mockups/current/brains.html`. The `brain-query` skill defines the ladder: STATUS, then INDEX, then the relevant notes following wikilinks, then logs, then archive and git history. It stops at the first layer that answers. Decisions outrank observations, and gaps are stated plainly.

The PRD's memory budget is a fixture of 100,000 log entries and 10,000 notes, with cached first-page browsing inside the 100 ms UI response target and no continuous full-repository scans while idle.

## Goals / Non-Goals

**Goals:**
- Read everything in a brain from Wiffletree, scoped to a project, with sources and verification one click away.
- Fast search from anywhere, by keyboard.
- One retrieval path for agents, with bounded output, that never crosses organizations.

**Non-Goals:**
- Editing notes in the browser. Notes are compiler-managed. A human change to current truth goes in as a log entry (the capture form) or a ruling.
- Searching across brains or organizations.
- Archive and git-history browsing beyond linking to GitHub.
- Embeddings or semantic search; FTS5 ranking is enough to start.

## Decisions

### Decided by Michael (October 29, 2026)

- There is one workspace Brain page, filtered by project. The Memory panel is the summary that opens it; there is no separate project Brain destination.
- Search never spans organizations.
- Author plus PR is enough for entries the skills write.

### Decided in this proposal, for review

1. **The browser reads `main` only**, from the host's checkout, plus this machine's own unmerged entries. Those are shown in Logs as pending, with their batch's stage, so a just-logged entry is visible before it merges. Teammates' unmerged entries are not visible until they merge.
2. **Scope:**
   - "This project" means entries whose repository folder is one of the project's repositories, and notes whose `sources` include such an entry.
   - Notes with no repository-specific sources stay visible in every scope.
   - Whole brain shows everything in the selected brain.
   - Opening from the Memory panel selects the project; opening from the Brains page selects Whole brain.
   - Choosing another brain resets to Whole brain.
3. **The note index tree** follows `notes/INDEX.md`: its headings become groups. Notes not listed in INDEX appear under "Not in the index", so a drift is visible rather than hidden. Each row shows the note's `kind` as a coloured dot.
4. **Sources per claim come from footnotes.** Each footnote is one claim, and each linked entry is one source. A source shows the entry's date, id and author, then its anchors (commit, PR, tag) and its verification state from `verify_refs`, computed on this machine. A footnote whose entry is missing shows as broken. Backlinks come from wikilinks in notes and in `STATUS.md`.
5. **The PR shown on an entry:**
   - for this machine's entries, the batch PR and its live stage from `brain-registry-sync`;
   - for other merged entries, the PR number from the squash commit that added the file, which is the batch PR, shown as merged;
   - an entry added by a direct push shows "no PR".

   The ticket and conversation links appear only for entries this machine wrote. Skill and teammate entries say they were logged outside this install.
6. **Logs paginate by keyset**, 50 per page, newest id first. The filters are applied in the index's SQL, so the first page stays within budget at 100,000 entries.
7. **The index** is one SQLite FTS5 table per brain, plus a metadata table:
   - The metadata is path, kind, type, repository, author, date, entry id, title and content hash.
   - After each fetch, only files whose hash changed are read and reindexed; deleted files are removed.
   - The index is a rebuildable projection: deleting it rebuilds it from the checkout.
   - It lives in the host's store, so remote clients use the host's index through the host API.
   - Ranking is BM25, then kind: decisions sort above observations, then by BM25 score.
   - Excerpts come from FTS5 `snippet()`, with the matched terms marked.
8. **Search UI:**
   - ⌘K or Ctrl+K anywhere, and `/` when focus is not in a text field, opens the overlay over the current brain: the browser's brain, or the selected project's when it has exactly one.
   - A project with several brains opens on the first and offers the others as chips; results never mix brains.
   - Arrow keys move, Enter opens the note or the entry in Logs, Tab cycles the scope chips, and Esc closes and returns focus to where it was.
9. **`query_brain`:**
   - It takes `question` (free text), and optionally `scope` (`project`, the default, or `brain`) and `repository` (to choose a brain when the project has several).
   - It is offered to the coordinator, implementers, testers and reviewers, because reading is safe. It is not offered to the compile session, which receives its input directly.
   - It walks the ladder, stopping when the budget of about 4,000 characters is filled or the ladder ends:
     1. STATUS excerpts that match;
     2. INDEX lines that match;
     3. the top notes by rank, following one level of wikilinks;
     4. the top log entries.
   - Each result carries its path, kind or type, `updated` or date, verification for entries, and its footnoted sources for note excerpts.
   - The response states the last compile time and the count of pending entries, and when nothing matches says "The brain has nothing on this", never a guess.
   - It is read-only and never writes a note. The skill's consented note-save has no equivalent here, because notes change only through compile.
10. **The Memory panel**, for a project with a brain, shows:
    - the brain and its sync state;
    - the project's batches that are not yet merged, with their stages;
    - a link to the Inbox with the count of open conflicts;
    - the five most recent entries;
    - unsent rows;
    - the capture form;
    - Open brain browser and Search.

    Without a brain, it says so and links to the Brains page (`brain-registry`).

### Options considered

- **A project Brain destination**, which the PRD suggested. This was rejected by Michael in favour of the workspace page with a project filter.
- **Scanning files on every view.** This was rejected: it cannot meet the 100 ms budget at the fixture size, and it breaks the no-idle-scan rule.
- **Cross-brain search.** This was rejected by Michael.

## Risks / Trade-offs

- **[FTS5 tokenisation of code identifiers and paths]** → Use the `unicode61` tokenizer with `_`, `-` and `/` as separators and prefix queries for the last term. Tune with the fixture.
- **[Verification computed per install can differ]** → An install without a repository shows unverified, with the reason "not on this machine". This is honest, and a link to GitHub stays available.
- **[Footnote parsing depends on compile output]** → `brain-compile` checks footnotes before pushing. A malformed note still renders, with a "sources not readable" note instead of failing.

## Migration Plan

Ship after `brain-log-capture`. Status and Conflicts show "No compile yet" until `brain-compile` ships and runs. The first index build runs in the background after each brain's first fetch.

## Overlaps with other proposals

These are merged on `main`:
- PR #162 (`shared-issues`), as 6194b99a;
- PR #163 (`assistant-spaces-jobs-boards`), as 419a55ef;
- PR #164 (`create_project`), as a8dded2d.

#162's and #163's changes are open there, not yet archived; #164's is archived. Where they change the same thing as this change, this change goes after them.

| Shared concept | Where else | Share or align | Open question |
| --- | --- | --- | --- |
| A SQLite index rebuilt from files | #162 indexes each tracker in `issues.sql` for its list, search and unread marks; this change indexes each brain with FTS5 | **Share** the pattern: a rebuildable projection, reindexed by content hash after each fetch. A common indexing helper is worth extracting. | One FTS facility for both, or one per feature? |
| Search UI | ⌘K brain search here; #162's Issues page has its own search | **Align** on one ⌘K overlay with a source chip (Brain, Issues) that never mixes organizations in one result list. | Should ⌘K also search issues once #162 ships, and how does "never across organizations" map to trackers that are per team rather than per owner? |
| Agent read tools | #162's assistant has `list_issues`, `read_issue` and `search_repositories`; agents here get `query_brain` | **Align** on read-only tools with bounded excerpts and authorship marked. | Should coordinators get both, and should `query_brain` results be marked as untrusted context the way issue text is? (Recommended: yes.) |
| Workspace pages | The Brains page here; #162's Issues page | **Align** with `brain-registry-sync`'s question on one Shared repositories page versus separate pages. | See `brain-registry-sync`. |
| Inspector rail | `files-panel` and `processes-panel` | **Align**: the Memory panel keeps its place after Git, Files and Processes. Its new summary does not change the rail. | None. |
| Rendering Markdown | Issue descriptions and comments (#162); notes and STATUS here | **Share** one Markdown renderer with wikilink and footnote support. | None. |
| Libraries of rendered Markdown with history | #163 (merged): Boards lists outputs, renders Markdown natively, and keeps each output's Git history | **Share** the Markdown renderer and the history view. Brain notes stay in the brain browser, because Boards holds outputs, not knowledge. | Should Boards link to the brain notes a job cites? |
| Search | #163 (merged): the assistant's search finds threads by title and text, filtered by Space; here, ⌘K searches one brain's notes and logs | **Align** on one ⌘K overlay with source chips (Brain, Threads and, from #162, Issues). It never mixes organizations or Spaces in one result list. | **Open:** one overlay for all three? Is a search scope an organization (brain) or a Space? |
| Threads reading the brain | #163: threads read their Space's brain and add entries to it; the router never sees brains | **Align**: give threads `query_brain`, which is read-only and so within thread authority, instead of letting them read brain files directly. | **Open:** give threads `query_brain`? |
| Where the browser sits | #163 pins the assistant and Boards above Projects and edits Spaces in Settings; here, Brains is a Workspace page and the Memory panel opens it | **Align** with `brain-registry-sync`'s navigation question. | See `brain-registry-sync`. |
| Projects created by a coordinator | #164 (merged) | None: a created project's Memory panel works like any project's. A project with every repository may span several brains, and the panel lists each. | None. |

## Open Questions

None blocking.
