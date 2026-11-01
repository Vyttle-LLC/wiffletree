## Why

The settings cascade shipped with brains left half outside it. Set up writes `wiffletree-brain.json` with no compile time, so a brain set up through the app is never scheduled, and the only way to give it a time is to edit the JSON by hand. A Space can set Router and Brain compiler rows that never run, because routing happens before a message has a Space and a brain can belong to several Spaces, yet the Models page and Effective settings report them as applying. The Space page's "set in the brain" names no place to go. Several Settings entries open the top of the same page, so they look like duplicates, and "Publishing" names a Behavior as well as a list of owners. Task 3.3 of `settings-cascade` was never done, so an additions line a Behavior now says still reaches agents twice.

The human approved all seven screens of the study in `ui/mockups.html` on November 1, 2026.

## What Changes

- **Set up asks for a compile time.** It becomes a short form on the Brains page, with 06:00 in the host's timezone filled in and both editable. Its PR writes `compile`, and the host validates the time and timezone as `refresh_compile_jobs` reads them.
- **A compile time can be set and changed.** A new human-only command, `set_brain_compile_time`, opens a config PR that keeps every other key. The Brains page warns about a brain with a config but no time, and offers Change compile time for one that has a time.
- **The Space page links to Brains.** Its Brain compile and Brain gate rows stay read-only and say "Change on Brains →", and Not scheduled is in the warning colour.
- **Router and Brain compiler rows are not Space rows.** A Space save refuses a new or changed one. One already in `space.yaml` is ignored at read with a note and blocks no other save. The Models page at a Space and Effective settings leave both jobs out, and a banner offers Remove them only when the Space holds one.
- **New Space says Additions,** as Behaviors does. The store key stays `instructions:`.
- **The Settings sidebar leads somewhere distinct.** The Spaces group becomes "Spaces & brains" with a Brains entry, "Publishing" becomes "Allowed owners", and Reviews, Who runs what, Subscriptions, PR keeper and Allowed owners each open their own card.
- **Old additions lines are offered for deleting.** The Behaviors page offers once per line to delete an additions line that matches a Behavior's sentence, never removes one unasked, and remembers Keep it.
- **Not included:** moving Who runs what or Reviews between sidebar groups, which stay where they are saved; changing the brain gate from the Space page.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `brain-registry`: Set up asks for a compile time, and changing the time goes through a PR.
- `brain-compile`: a brain with no compile time compiles only on Run now or a merged ruling, and says so.
- `settings-cascade`: Router and Brain compiler rows leave the Space layer, the Space page links to Brains, the Settings window's groups change, and old additions lines are offered for deleting.

## Impact

- `workspace-core`: `Job::space_may_set`, `validate_space_rows`, `SpaceTable` dropping those rows, `SpaceModels::ignored`, `BrainCompile` defaults, `Command::SetBrainCompileTime` and `SetUpBrain`'s `compile`, `BrainsPage.this_timezone`.
- `workspace-host`: `brains.rs`, `compile.rs` (the schedule's one reading of a compile), `policies.rs`, `behaviors.rs`, `live.rs`, `remote/access.rs`, `lib.rs`.
- `workspace-desktop`: `brains_view.rs`, `spaces_view.rs`, `models.rs`, `models_view.rs`, `repositories_view.rs`, `settings.rs`, `skills_view/behaviors.rs`, `ui.rs`, `shared.rs`, `main.rs`, `preferences.rs`.
- Protocol: `set_up_brain` gains a required `compile`; `set_brain_compile_time` is new; `brains` answers `this_timezone`.
- Docs: `docs/host-protocol.md`, `docs/PRD.md`, `design/README.md`, the study in `ui/mockups.html`.
