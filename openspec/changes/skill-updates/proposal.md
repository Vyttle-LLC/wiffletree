## Why

When the human rewrites a skill, Wiffletree keeps the stock text the rewrite started from as its base. When a release changes that stock text, the rewrite is marked out of date and keeps running exactly as written. The human sees what changed as a line diff and has to copy in what they want by hand, then choose Mark as up to date. Every release that touches a rewritten skill asks for that work, and until it is done the human's agents miss the stock improvement.

The human decided on October 12, 2026 how updates to a rewritten skill work:
1. merge as plain text when the changes don't overlap;
2. use a model to merge when they do;
3. tell the human only when there is a real question.

The result applies without waiting for approval, with a note of what changed that can be reviewed or undone. "Merging" means bringing Wiffletree's updates into the human's copy; nothing is sent upstream. This is change 6 of 6 in the plan `plan-pr-keeper-and-skills.md`. It builds on skills (change 4), whose `base.md` exists for this merge, and on the settings repository (change 5), which syncs skills between Macs.

## What Changes

- **A new stock version is merged into a rewrite automatically.** When a rewrite's base is an older stock version than this release's, the host merges the stock change into it three ways: the base, the new stock text and the rewrite. Changes that don't overlap merge as plain text. The merged text and the new base are saved together as one change, with a history entry. Additions mode needs no merge, because its rules sit beside the stock text.
- **Overlap is decided by paragraph.** A paragraph is a run of non-blank lines with the blank lines after it. A skill's first line, its title and version, is merged on its own and never overlaps, so a new version number never sends an edited first paragraph to the model. The merge compares paragraphs, not lines, so blank lines never separate two changes. A stock change and a change in the rewrite overlap when they touch the same base paragraph, or neighbouring paragraphs with no unchanged paragraph between them. The plain-text merge is built on `similar`, which the workspace already uses. It needs no new dependency.
- **A model merges only the overlapping passages.** Everything else is merged as plain text first. The model gets each overlapping passage as the base, the stock version and the human's version, with the three whole texts for context. It runs on the PR keeper's model, with every tool turned off, like the keeper's comment sorting. Its answer is checked before anything changes. A merge needs no model when nothing overlaps. Offline, or with no Claude model for the PR keeper, the rewrite keeps running as written, and its page offers the human the same choice per passage.
- **The human is asked only when there is a real question.** That is when the model reports that the two versions of a passage want different things, so that keeping one drops the other's intent. Nothing changes then. A workspace note in the Inbox and the skill's page show the question with both wordings. The human chooses the update's wording or their own for each passage, and the merge then applies. A missing model or a failed try is not a question: the page says so, and nothing goes to the Inbox.
- **A note of what changed, with Review changes and Undo.** After a merge, the skill's page shows what was merged and how, and the Skills list marks the skill "Rewritten · updated". **Review changes** shows the rewrite before and after, each change labelled as coming from the update or as merged by the model. **Undo** puts back the exact text and base from before the merge, and stops that stock version being merged again on its own. The note stays until the human reviews or dismisses it on that Mac, or the skill changes again. The history always keeps the merge.
- **One merge for all Macs.** With a settings repository, a merge waits for the launch's first sync pass, so a Mac usually takes another Mac's merge instead of making its own. When two Macs merge at once, the first push wins. The other Mac takes that version through sync without pausing the skill, and merges again only if it is still out of date. A rewrite merged on a Mac with a newer release is not marked out of date, and is not merged, on a Mac with an older one.
- **Safety.** The fixed core is never part of a merge: it isn't in the shared folder, and the model is never sent it. The model writes no file: the host applies its answer through the skill store, which writes the files and the history entry together or not at all. Undo restores the human's earlier text byte for byte, from the merge's own history entry.
- **Not included:** merging into Additions (none is needed), sending changes upstream, merging the fixed core, merging two Macs' edits of the same rewrite (change 5 still pauses those), new skills, and a merge model of its own in Models (question 1).

## Capabilities

### New Capabilities
<!-- None. Merging is part of the skills capability. -->

### Modified Capabilities
- `skills`: new requirements for merging a new stock version into a rewrite, the model's part, asking the human, and the note with Review changes and Undo, and for safety. "A rewrite is marked out of date by a new stock version", "Every skill has a history", "Only the human changes a skill", "The Skills page" and "The limits of skills are documented" change to match.
- `settings-repository`: a new requirement that a skill update is merged once for all Macs. "A file changed on two machines is paused" changes to except a Mac's own automatic merge.
- `model-routing`: "The PR keeper's model" also merges overlapping skill updates when it is a Claude model.

## Impact

- `workspace-host`: a new `skills/merge.rs` for the three-way merge and the model's input and answer; `skills.rs` gains merge, undo and answer changes, and the rule for when a merge is due; `provider.rs` gains a merge run, built like the sorting run; `sync.rs` takes another Mac's merge in place of this Mac's without pausing; host start and sync passes check for due merges.
- `workspace-core`: the merge's note, its review and an open question on `SkillState`; a second kind of workspace note; commands to undo a merge, merge on demand, answer a question and dismiss the note.
- `workspace-desktop`: the merge note, Review changes and Undo on a skill's page; the question with its choices on the page and in the Inbox; the "updated" and "needs you" marks in the Skills list; one line on the Models page's PR keeper card.
- Stored data: new history entry kinds in each skill's `history/`. No new table and no migration. The dismissed notes are kept in this Mac's database.
- Docs: `README.md`, `docs/PRD.md` and `docs/DEVELOPMENT.md`.
- Built in five slices, each its own PR of about 300 production lines or less; see tasks.md.

## Decided questions

The human approved this proposal on October 16, 2026, with the recommended answer to each question. design.md is written to these answers.

1. **Which model merges overlapping passages?** The PR keeper's model, when it is a Claude model. It needs no new setting, and the run must be Claude because it runs with every tool off, which Codex can't do (`provider.rs:233`).
2. **When does a merge run?** At host start, after each sync pass that changed a skill, and when the human chooses Merge v2 in. With a settings repository, host start waits for the launch's first pass to end, whether it reached GitHub or not.
3. **What does the human see while a model merge waits or fails?** Only the skill's page and the existing out-of-date mark, with the reason and Try again. No Inbox note.
4. **How long does the note of what changed stay?** On each Mac, until the human chooses Review changes or Dismiss there, or the skill changes again. Dismissing is kept in this Mac's database, not the shared history.
5. **What stops a merge from running again?** Undo stops automatic merges into that stock version until a newer one arrives, and so does restoring a rewrite from before a merge. A waiting question stops them while the skill's rewrite and base are still the ones it asked about. Merge v2 in always works.
6. **When a question has several passages, does the rest of the merge wait?** Yes: nothing applies until every passage is answered, so a rewrite's base is always one whole stock text.
