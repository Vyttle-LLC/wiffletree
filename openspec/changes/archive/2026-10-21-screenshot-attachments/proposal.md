## Why

The human cannot attach screenshots on macOS. Dragging a screenshot's floating thumbnail into the composer adds a chip, but Send fails with "Could not copy <name>: Operation not permitted (os error 1)": the file sits in macOS's protected `$TMPDIR/TemporaryItems/NSIRD_screencaptureui_*`, which only the app that received the drop may read, and only around the drop, while the host copies it later, at Send. Pasting a copied screenshot with Cmd+V does nothing, because the app never reads images from the clipboard.

## What Changes

- **Copy on drop.** The desktop copies each dropped file into a staging folder in its data folder when it is dropped, and the draft attaches that copy. A file that cannot be copied is refused at the drop with its reason. Send copies from the staged copy as before.
- **Paste files and images.** Cmd+V in the composer with an image on the clipboard saves it as a PNG named like `Pasted image 2026-10-21 at 14.03.12.png` in the same staging folder and attaches it like a drop. Files copied in Finder are attached like a drop. Text pastes as before.
- **Staged copies are temporary.** A copy is removed when its draft is sent or it is taken out of the draft, and the whole staging folder when the app starts.
- **Unchanged.** The host's checks (size, type, duplicate names), the chips, the refusal notice, the message store and delivery to Claude and Codex.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: dropped and pasted files are copied into the draft when they are attached.

## Impact

- `crates/workspace-desktop/src/staging.rs` (new): the staging folder, copies, pasted PNGs and their names.
- `crates/workspace-desktop/src/conversation.rs`: drop and paste stage files before the host checks them; send and remove release copies.
- `crates/workspace-desktop/src/attachment_chips.rs`: `add_staged` merges staging and the host's check and releases copies left out.
- `crates/workspace-desktop/Cargo.toml`: `image`, to convert a pasted non-PNG image.
- Docs: `docs/DEVELOPMENT.md`. No host, storage or protocol change.
