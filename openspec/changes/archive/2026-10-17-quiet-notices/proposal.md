## Why

The chat column is cluttered by Wiffletree's own notices: the PR watcher, the PR keeper, verification results, publish results and the rest. Each draws as a full message with an avatar, and each breaks the folded "N agent messages" run around it, although the coordinator's reply already says what they meant. The human approved folding them into the same run (study: `design/mockups/current/quiet-notices.html`), so the chat holds the human's messages and the session's own replies.

## What Changes

- **Notices fold.** Every message `host_notice` recognises is part of a run with the agent messages, timers and check-ins next to it. The human's messages, Inbox answers, and the session's own replies, errors and compaction lines still break a run.
- **The heading counts each kind.** "5 notices · Wiffletree PR watcher", or "2 agent messages, 2 notices · Implementer, Wiffletree, Wiffletree PR keeper"; agent-only runs read as before.
- **A notice's line.** Its sender, a muted pill naming what happened, its first line and time; opened, its full text with file links. The pill's words come from the notice's id and, where needed, its body, typed in core as `Notice` beside `MessageKind`. A notice whose words cannot be told reliably reads "Notice".
- **Failures count as trouble.** Failing checks, a conflict, a failed rebase, a failed or blocked verification and a failed publish draw a red pill and count in the run's "failed" or "blocked" figure.
- **No lone notice.** The desktop's separate notice message is removed.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `work-stream`: Wiffletree's notices fold into runs of peer messages instead of drawing as their own messages.

## Impact

- `crates/workspace-core/src/messages.rs`: `MessageKind::Notice(Notice)` and the `Notice` labels.
- `crates/workspace-desktop/src/transcript.rs`: runs take notices; the heading counts kinds; notice pills; `Shape::Notice` and `notice_message` removed.
- Design: `design/mockups/current/quiet-notices.html`, linked from `design/README.md`.
- Docs: `docs/DEVELOPMENT.md`.
- No host, storage or protocol change: what is stored, delivered and given to agents is unchanged.
