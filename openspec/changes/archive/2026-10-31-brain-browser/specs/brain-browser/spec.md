## ADDED Requirements

### Requirement: The brain browser opens from the Brains page and the Memory panel
The desktop SHALL offer one brain browser page in the workspace. Opening a brain from the Brains page SHALL show that brain with the scope Whole brain. Open brain browser in a project's Memory panel SHALL show the project's brain, its Space's brain (`brain-registry`), with the scope set to that project. The browser SHALL show scope chips for each project in a Space that names the brain, plus Whole brain, and a switcher among the registered brains; choosing another brain SHALL reset the scope to Whole brain. It SHALL have the views Status, Wiki, Logs and Conflicts. It SHALL read the brain's `main` from the host's checkout, plus this machine's unmerged entries in Logs.

#### Scenario: From the Memory panel
- **WHEN** the human presses Open brain browser in the Memory panel of "Watch app call relay"
- **THEN** the browser shows `sagehaven-docs` with the chip "This project · Watch app call relay" selected

#### Scenario: From the Brains page
- **WHEN** the human opens `vyttle-docs` on the Brains page
- **THEN** the browser shows `vyttle-docs` with Whole brain selected

### Requirement: Project scope filters by the project's repositories
With a project selected, Logs and Conflicts SHALL show only entries and conflicts whose repository folder is one of the project's repositories. Wiki SHALL show notes whose `sources` include such an entry, plus notes with no repository-specific source, and SHALL say how many notes it hides, with a way to show them all. Whole brain SHALL show everything in the selected brain. No view SHALL show content from another brain.

#### Scenario: Hidden notes
- **WHEN** the scope is "Watch app call relay" and two notes cite only `simpatico-sage` entries
- **THEN** the index tree omits them and says "2 notes outside this project hidden", with show all

### Requirement: The Status view shows STATUS and the brain's pipeline
The Status view SHALL render `STATUS.md` with clickable wikilinks. Beside it, it SHALL show the last compile time, the next run, the compiler host, the number of pending entries and the brain's open batch and compile PRs with their stages, each linking to the PR gate on the Brains page. Before the first compile it SHALL say "No compile yet".

#### Scenario: A failed batch in Status
- **WHEN** batch PR #214 has failed `sensitive-info`
- **THEN** the Status view lists #214 as failed check, linking to its row on the Brains page

### Requirement: The Wiki view shows notes with sources and backlinks
The Wiki view SHALL show:
- a note index tree grouped by `notes/INDEX.md`'s headings, with notes missing from INDEX under "Not in the index" and each note's `kind` marked;
- the selected note rendered with a `kind: decision` or `kind: observation` badge, its `updated` date and clickable `[[wikilinks]]`; a wikilink to a missing note SHALL be shown as broken;
- backlinks from other notes and from `STATUS.md`;
- a Sources list per claim, built from the note's footnotes, where each source shows its entry's date, id and author, then its commit, PR and tag anchors, and its verification state (verified, claimed or unverified, as `brain-compile` defines them, computed on this machine).

Selecting a claim's marker SHALL highlight its sources, and selecting an entry SHALL open it in Logs.

#### Scenario: Following a wikilink
- **WHEN** the human clicks `[[Watch Call Token Lifetime]]` in "Call Relay Architecture"
- **THEN** that note opens, and its backlinks include "Call Relay Architecture"

#### Scenario: A claim's sources
- **WHEN** the human selects the retry claim's footnote
- **THEN** the Sources list highlights two entries: Oscar's, verified with commit a41f2c0, PR #81 and tag v1.6.0, and Michael's, claimed with commit 7d03b1e and PR #92 open

### Requirement: The Logs view is a filtered, paginated timeline
The Logs view SHALL list entries newest first, 50 per page, with older and newer page controls. It SHALL filter by repository, type, author and verification. Each entry SHALL show:
- its heading, type and verification;
- its repository, author, source and file path;
- its five fields;
- the PR that added it: for this machine's entries, the batch PR with its live stage (open, checks running, auto-merging, merged, failed check); for other entries, the PR of the commit that added the file, shown as merged, or "no PR";
- for entries this machine wrote, links to the originating ticket and conversation; other entries SHALL say they were logged outside this install.

This machine's unmerged entries SHALL appear as pending, with their batch's stage. Opening an entry from elsewhere SHALL clear the filters and show the page that holds it.

#### Scenario: Filtering
- **WHEN** the human filters Logs to type decision and author Oscar
- **THEN** only Oscar's decision entries in scope are listed, newest first

#### Scenario: A teammate's entry
- **WHEN** the human views an entry RB logged with the skill
- **THEN** it shows RB, `source: skill` and its merged PR, and says it was logged outside this install

### Requirement: The Conflicts view shows open conflicts with rulings
The Conflicts view SHALL show each open conflict from `reconcile-queue.md` in scope as a card with the new claim, the established decision, the evidence, the recommended ruling and the actions Uphold existing, Keep new, Keep both and Defer, as `brain-compile` specifies. A ruled conflict SHALL show its ruling and that a compile starts when its entry merges. With no conflicts, it SHALL say so.

#### Scenario: No conflicts
- **WHEN** `vyttle-docs` has no open conflicts
- **THEN** the Conflicts view says there are none

### Requirement: The Memory panel summarizes the project's brain
A project's Memory panel SHALL show:
- the project's brain and its sync state;
- the project's batches that are not yet merged, with their stages;
- the number of open conflicts, linking to the Inbox;
- the five most recent entries in scope;
- unsent rows;
- the capture form;
- Open brain browser and Search.

Selecting an entry SHALL open it in the browser with the project scope. A project without a brain SHALL see the "No brain" state from `brain-registry`.

#### Scenario: The relay project
- **WHEN** the human opens Memory in "Watch app call relay"
- **THEN** it shows `sagehaven-docs`, the failed batch #214 and the auto-merging batch #213, a link to 2 conflicts in the Inbox, and the five newest entries

### Requirement: Search is a keyboard quick-open over one brain
⌘K and Ctrl+K anywhere in the window, and `/` when focus is not in a text field, SHALL open a search overlay over one brain:
- in the browser, the browser's brain;
- in a project, its Space's brain;
- otherwise, the brain of the Space last used.

A project whose Space has no brain SHALL get an overlay that says so, with a link to Settings › Spaces.

Scope chips SHALL offer this project and Whole brain, and Tab SHALL cycle them. Results SHALL be notes and log entries ranked by relevance, with decisions above observations, each with its title, kind, place and an excerpt with the matched terms marked. Arrow keys SHALL move the selection, Enter SHALL open a note in Wiki or an entry in Logs, and Esc SHALL close the overlay and return focus. Results SHALL never mix brains or organizations.

#### Scenario: Searching from a project
- **WHEN** the human presses ⌘K in "Watch app call relay" and types "retry telnyx"
- **THEN** results come only from `sagehaven-docs` in that project's scope, decisions first, with "retry" and "Telnyx" marked in each excerpt

#### Scenario: Keyboard only
- **WHEN** the human presses `/`, types a query, presses the down arrow twice and Enter
- **THEN** the third result opens, and Esc in a later search returns focus to where it was

### Requirement: The index is incremental and rebuildable
The host SHALL keep one SQLite FTS5 index per brain over notes, `STATUS.md` and log entries on `main` of its checkout, with each file's path, kind or type, repository, author, date, entry id, title and content hash. After each fetch it SHALL reindex only files whose hash changed and remove deleted ones. It SHALL do no indexing work while no fetch has changed anything. Deleting the index SHALL rebuild it from the checkout. The views and search SHALL read through the index, so that with 100,000 entries and 10,000 notes the first page of Logs and the first search results are served from cache within the 100 ms UI response target.

#### Scenario: One changed note
- **WHEN** a compile PR merges that changes three notes and `STATUS.md`
- **THEN** the next fetch reindexes those four files only

#### Scenario: Rebuild
- **WHEN** the index file is deleted while the host is stopped
- **THEN** the host rebuilds it from the checkout on start, and search returns the same results

### Requirement: Agents query the brain with query_brain
The host SHALL offer a read-only `query_brain` MCP tool to the project coordinator, implementers, testers and reviewers. It SHALL take `question`, and optionally `scope` (`project`, the default, or `brain`). The brain SHALL be the brain of the session's project's Space (`brain-registry`), so it is never chosen by the agent. The tool SHALL refuse when that Space has no brain.

It SHALL walk the ladder in order:
1. matching `STATUS.md` excerpts;
2. matching INDEX lines;
3. the top notes by rank, following one level of wikilinks;
4. the top log entries.

It SHALL stop when about 4,000 characters of excerpts are gathered or the ladder ends, with decisions above observations at each step. Each excerpt SHALL carry its path, kind or type, date, its footnoted sources for notes and its verification for entries. The response SHALL state the last compile time and the number of pending entries. When nothing matches, it SHALL say the brain has nothing on the question. It SHALL never span brains and never write.

#### Scenario: STATUS answers
- **WHEN** a coordinator asks "what is the state of watch calling?"
- **THEN** the response leads with the matching STATUS excerpt and stops before reading logs when the budget is filled

#### Scenario: Nothing found
- **WHEN** an implementer asks about a topic the brain never mentions
- **THEN** the response says the brain has nothing on it, with the last compile time, and invents nothing

#### Scenario: Never across organizations
- **WHEN** an implementer in a project of the Vyttle Space asks about the call relay
- **THEN** only `vyttle-docs`, the Vyttle Space's brain, is searched, and no `sagehaven-docs` content is returned
