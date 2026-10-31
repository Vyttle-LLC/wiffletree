## MODIFIED Requirements

<!-- Delta order for these requirements: 1 brain-browser, which adds them for projects → 2 assistant-spaces-jobs-boards (this), which gives assistant threads, issue threads and job runs query_brain and the search overlay (moved from brain-browser, whose task 2.1 shipped query_brain before the assistant existed). Each restates brain-browser's version with the threads' additions. -->

### Requirement: Search is a keyboard quick-open over one brain
⌘K and Ctrl+K anywhere in the window, and `/` when focus is not in a text field, SHALL open a search overlay over one brain:
- in the browser, the browser's brain;
- in a project or assistant thread, its Space's brain;
- otherwise, the brain of the Space last used.

A project or thread whose Space has no brain SHALL get an overlay that says so, with a link to Settings › Spaces.

Scope chips SHALL offer this project and Whole brain, and Tab SHALL cycle them. Results SHALL be notes and log entries ranked by relevance, with decisions above observations, each with its title, kind, place and an excerpt with the matched terms marked. Arrow keys SHALL move the selection, Enter SHALL open a note in Wiki or an entry in Logs, and Esc SHALL close the overlay and return focus. Results SHALL never mix brains or organizations.

#### Scenario: Searching from a project
- **WHEN** the human presses ⌘K in "Watch app call relay" and types "retry telnyx"
- **THEN** results come only from `sagehaven-docs` in that project's scope, decisions first, with "retry" and "Telnyx" marked in each excerpt

#### Scenario: Searching from a thread
- **WHEN** the human presses ⌘K in a thread of the Vyttle Space
- **THEN** the overlay searches `vyttle-docs`, the Vyttle Space's brain, over the whole brain

#### Scenario: Keyboard only
- **WHEN** the human presses `/`, types a query, presses the down arrow twice and Enter
- **THEN** the third result opens, and Esc in a later search returns focus to where it was

### Requirement: Agents query the brain with query_brain
The host SHALL offer a read-only `query_brain` MCP tool to the project coordinator, implementers, testers and reviewers, and to assistant threads, issue threads and job runs (see `assistant`). It SHALL take `question`, and optionally `scope` (`project`, the default, or `brain`); a session owned by a Space SHALL always get the scope `brain`. The brain SHALL be the brain of the session's Space: its project's Space, or the Space that owns it (`brain-registry`), so it is never chosen by the agent. The tool SHALL refuse when that Space has no brain.

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

#### Scenario: A thread asks
- **WHEN** a thread in the Sage Space asks what was decided about API date formats
- **THEN** only the Sage Space's brain is searched, over the whole brain
