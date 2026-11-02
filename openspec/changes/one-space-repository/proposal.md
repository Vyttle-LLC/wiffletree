## Why

A Space names a brain and a storage repository today, with two sets of rules and three places to look. A brain needs a gate, a compiler host and a pull request for every batch, and it only runs when a Space names one. The Vyttle Space names none, so its projects can neither log nor query. Linear still holds the issues, and four second-brain skills still sit beside the app.

On November 1, 2026 Michael approved one model, in `model.md` and `ui/mockups.html`: a Space names one repository and one folder, and its issues, log entries and wiki live there and are all written the same way. Wiffletree does the logging, compiling and reconciling by itself.

## What Changes

- **One repository, one folder.** A Space names one private repository, or a local one, and a folder in it that defaults to `wiffletree/`. **BREAKING:** a Space no longer names a brain, and every path Wiffletree writes moves under the folder.
- **Every Space has issues, logs and a wiki.** There is no Brain field. A Space on one Mac gets them too.
- **Straight in.** Issue changes, log entries, compiles and rulings all go through the one sync engine, are scanned on the Mac and land on `main`. A file that fails stays on that Mac with an Inbox card. Log entries stop waiting for batches. **BREAKING:** batch branches, auto-merge, brain pull request watching and the GitHub gate go.
- **Volunteer Macs compile.** Anyone can turn compiling on. Every volunteer is due at the set time, the first awake claims the run, a crashed claim expires after an hour, and a Mac out of model quota skips. **BREAKING:** the compiler named in `wiffletree-brain.json` goes.
- **Each Mac scans what it pulls** and raises an Inbox alert when something on `main` fails, once per Mac.
- **Refs are checked through GitHub** when a repository is not cloned on the compiling Mac.
- **The wiki gets product pages and issue links.** Finished tickets do not feed it. It links to them.
- **Bring in a brain** reads this Mac's clone, so files never pushed are not lost, and leaves the original as it was.
- **New setup screens:** New Space, repository checks, the Space page's one Repository box, joining, and who compiles on the Brains page.
- **The second-brain skills retire.** Wiffletree's own instructions and docs stop naming them.

## Capabilities

### New Capabilities

None. Each change belongs to a capability that exists.

### Modified Capabilities

- `spaces`: one repository and folder, the setup form, repository checks, Protect history, the Repository box, joining, upgrade.
- `shared-repositories`: the folder, finding it, moving root files into it once, rule sets for logs and the wiki, scanning what is pulled, alerts. The one-role rule goes.
- `shared-issues`: the tracker's files sit in the folder, numbering reads history from before the move, and the wiki links to issues without being fed by them.
- `settings-repository`: a repository that was a brain can be chosen, and settings stay at the repository's root.
- `brain-registry`: a Space's brain is its folder's `second-brain/`. Batches, grants, checkouts and config files go. Who compiles and Bring in a brain arrive.
- `brain-gate`: the seven GitHub checks go. Two checks stay as the scan on the Mac, with the entry contract.
- `brain-compile`: volunteers, claims, landing on `main`, GitHub ref checks, product pages and issue IDs.
- `brain-browser`: no batches or pull requests in its views, every Space has a brain, and issue IDs link.
- `project-memory`: entries land straight in, and logging works in every Space.
- `scheduled-jobs`: a built-in job belongs to a wiki and is due on every volunteer Mac.
- `skills`: the standalone second-brain skills are retired.
- `assistant`: a thread's entries land straight in, and every Space keeps them.
- `settings-cascade`: a Space's page no longer shows Brain compile and Brain gate rows.
- `workspace-host`: a compile session is owned by its wiki and works in a worktree of the repository's clone.

## Impact

- `workspace-core`: `SpaceStorage.root`, `Space.brain` read only as a record of an older brain, `TrackerConfig` gains `compile` and `allowed_email_domains`, and the brain batch and PR gate types and commands go. `docs/host-protocol.md` follows.
- `workspace-host`: `repos.rs`, `storage.rs`, `issues_sync.rs`, `scan.rs`, `compile.rs`, `verify_refs.rs`, `memory.rs`, `legacy_logs.rs`, `brain_index.rs`, and most of `brains.rs`. New tables for claims, scan marks, scan alerts and held files. The bundled compile procedure becomes v2.
- `workspace-desktop`: `spaces_view.rs`, `brains_view.rs`, `brain_browser_view.rs`, `brain_markdown.rs`, the Inbox.
- `gate/`: everything but the two checks `scan.rs` bundles, their fixtures and the entry fixtures.
- GitHub: `gh` creates a private repository when the human asks and sets a ruleset for Protect history. Nothing needs a workflow or the `workflow` permission.
- Older hosts: a store of any earlier schema still opens, and a repository's move stops an older host with "Update to use it", as `design.md` sets out.
- The "Move a Space's storage" ticket in Wiffletree: Spaces, and Michael's dotfiles, `personal-agents` and `vyttle-docs`, as `design.md` lists. This change touches none of them.
- This change assumes `choose-space-repositories`, whose code is merged, is archived first: its delta and this one both modify two `spaces` requirements, and the text here carries its wording.
