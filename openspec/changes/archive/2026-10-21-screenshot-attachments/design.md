## Context

See proposal.md. Attachments are checked by This Mac's host when dropped (`InspectAttachments`) and copied by the host into the message's folder when sent (`attachments::store`). The host is a separate process, so it cannot read a file macOS grants only to the app that received the drop. gpui-component's `Textarea::on_paste` lets the app take a paste before the input inserts text.

## Decisions

### 1. One staging folder for the app, one subfolder per file
Copies go to `<data dir>/staged-attachments/<id>/<file name>`. Drafts live in the app's memory and are lost when it quits, so staging belongs to the app, not to a session or a project: no project folder is touched until a message is sent, and the folder can be emptied at start without knowing which sessions exist. A subfolder per copy keeps each file's own name, which is what the message stores, and lets two drafts hold files with the same name.

### 2. Copy first, then let the host check the copy
The app copies the files off its main thread as soon as the drop or paste arrives, then asks the host to inspect the copies, so the host's limits and messages stay the only ones. A file the app cannot copy is refused with "Could not copy <name>: <reason>". Folders and files over 50 MB are not copied; their original path goes to the host, which refuses them with its usual reason.

### 3. Releasing copies
Each staged copy is held: by the draft it is attached to, and by a send that is still reading it, until the host answers a local send or a remote send's uploads end. The task awaiting a send's reply, or the remote upload worker, owns those holds, not the view, so closing the window mid-send keeps the copies until the host has read them. It is removed when its last hold goes: when it is taken out of the draft and no send reads it, when its draft is sent, when the host refuses it or its name is already attached, when the host could not check the drop, and when the conversation view is dropped, as when the window closes and the app keeps running. A drop whose check is still running when the view closes drops its holds with that check. The app also empties the staging folder when it starts, for copies a crash left.

### 4. What a paste attaches
The clipboard's first entry decides, as GPUI orders them: copied Finder files (paths) are attached like a drop, an image is saved as a PNG and attached, and anything else, text included, is left for the input. A non-PNG image is converted with the `image` crate. Its name is `Pasted image <date> at <time>.png`, numbered from 2 when the draft, or a paste still being saved or checked, already has one of that name.

## Risks

- The drop's read access must last until the background copy starts. It begins in the same event's handler, before the next frame, and was driven with the file-drop events of a Finder drag; the real screenshot thumbnail drag must be checked by hand.
