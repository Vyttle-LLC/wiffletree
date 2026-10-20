## Why

The decision card above the coordinator's composer has no height limit and nothing inside it scrolls. A long approval, such as a pull request approval with a full Description, grows past the bottom of the window, so its Deny and Open pull request buttons and the composer cannot be reached. The human approved panel (b) of the study: cap the card and scroll its middle (mockup: `design/mockups/current/decision-card-scroll.html`).

## What Changes

- **A capped card.** The card takes at most half the window's height. A short card looks as it does today, with no scroll bar.
- **One scrolling middle.** Everything between the header row (asker and pill) and the button row scrolls as one: the prompt, detail rows, Description, a question's choices and its answer box. The header, the buttons and the composer stay visible. A long question's prompt no longer has its own inner scroll in this card.
- **Every kind alike.** Questions, permission approvals and pull request approvals. Stepping to another decision starts it at its top.
- **Unchanged.** The Inbox panel, which already scrolls as a list and keeps its prompt cap, and minimize and expand.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: the decision card above the composer is capped and scrolls its middle.

## Impact

- `crates/workspace-desktop/src/inbox.rs`: `attention_card` builds header, body and buttons separately and, given a height, caps the card and scrolls the body.
- `crates/workspace-desktop/src/conversation.rs`: `decisions()` passes half the window's height.
- Docs: `docs/DEVELOPMENT.md`, `docs/PRD.md`; mockup `design/mockups/current/decision-card-scroll.html`.
- No host, storage or protocol change.
