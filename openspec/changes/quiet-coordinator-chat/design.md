## Context

A coordinator's turn ends in `service.rs` (`ProviderEvent::Finished`), which writes the reply with `Host::append_output` under the id `output:<run>`. The turn's input message ids are in `Active::messages`. The desktop folds peer messages with `transcript::placements`; the coordinator's own reply is always `Placement::Alone`. The Tickets inspector reads `WorkspaceIndex::tickets_owned_by`, which joins the snapshot's open tickets with the archive's settled ones.

## Goals / Non-Goals

**Goals:** fold only what the records prove unchanged; keep every reply's text, delivery and record as it is; a table that is always current without a coordinator turn or a new poll.

**Non-Goals:** judging the reply's words; summarising or rewriting replies; changing how peer messages fold; a table in worker conversations.

## Decisions

### The news mark is an optional `news` field on `set_plan`
Every role already calls `set_plan` during a turn, so the coordinator marks a reply as news by sending `news: true` with its checklist. It is held on the running turn (`Active::news`) and read when the turn ends; a restart ends the turn anyway. Alternatives: a new tool adds surface for one bit; a text prefix in the reply is what the brief rules out. Other roles may pass it and nothing happens.

### Facts are compared, not words
`ProjectFacts` is what the host records about the coordinator's project:
- each ticket the coordinator owns: state, branch head for open tickets (`git rev-parse --verify refs/heads/<branch>` in the repository; unreadable reads as unknown and is never a change), the latest cycle's number, outcome, round count and each verifier's result, ledger entry ids with their status, and the PR's number, state, checks and unresolved threads;
- the project's open questions, leaving out the host's own check-in notes;
- the project's unarchived agents other than the coordinator, each with whether it is stopped (paused or disconnected) or has held input.

Wiffletree words each difference when the turn ends, such as "Stage 1 committed 684cd0f", "Stage 2: ready for testing" or "Implementer · Stage 2 started", and stores the words, so every client shows the same line and a restart changes nothing.

### Where the result and the baseline live
- The result is a nullable JSON column `messages.reply_update`, written in the reply's own insert: `{"kind":"unchanged"}` or `{"kind":"changed","changes":[…]}`. `Message::update` carries it to clients; older messages have none.
- The baseline is one row per coordinator in a new `reply_baselines` table: the facts at its last reply shown in full. Every full reply replaces it, the human-started ones included; a folded reply leaves it alone.
- The message-revision trigger lists `reply_update` among the columns it watches; the column is added before that trigger is created.

### Which turns can fold
Only a project coordinator's turns. A turn is human-started when its input is empty (a retry or Go live) or holds a message from the human (a sender-less message that is not a Wiffletree notice, which includes Inbox answers). Such a reply gets no update and resets the baseline. Otherwise:

| Baseline | Facts changed | Trouble or news | Result |
|---|---|---|---|
| none yet | – | – | none (full), baseline set |
| present | yes | any | `changed` with the list, baseline set |
| present | no | yes | none (full), baseline set |
| present | no | no | `unchanged`, baseline kept |

Trouble is any input that a folded run would count in red: a failed or blocked report, a turn that stopped with an error, or a notice of a failure (`MessageKind::trouble`, shared by host and desktop).

### How the desktop folds
`placements` treats the session's own reply with `update == unchanged` as a peer message, so it joins the run of agent messages around it. The run's heading adds "N replies, no change", leaves the coordinator out of the sender names, and the opened run starts with a muted line saying Wiffletree found no change. Each such reply's line shows the coordinator's name, a "No change" pill and its first line, and opens to the full reply as any line does. A `changed` reply draws as before with a muted "Since last update:" line above its text.

### The pinned table
`ticket_table.rs` draws it between the header and the transcript, for the selected coordinator only. Rows come from `tickets_owned_by`: in progress means open and not `planned`, or with an open PR; accepted and closed count as done. Status is the Tickets inspector's state wording, with "· round N" while a cycle runs; the PR column reads "#N open", "#N merged" or "#N closed", linked to the PR, and "Not published" otherwise. The done line opens to the same rows. The collapse choice is `collapse_ticket_table` in `desktop-preferences.json`. The table redraws when the conversation does on a new snapshot; it adds no fetch.

## Risks / Trade-offs

- [A coordinator writes something important in an unchanged turn] → it can mark news, the reply stays one click away, and nothing is hidden from the record or agents.
- [A Git read per open ticket at each coordinator turn end] → one `rev-parse` each, bounded by the host's Git timeout; a failure is unknown, never a change.
- [Verifier start and archive count as agent changes] → they coincide with cycle changes, which are news anyway.

## Migration Plan

The column and table are created on open with `IF NOT EXISTS`-style checks; no data moves. A store opened by an older release ignores the column.
