## Why

When the human rewrites a skill, Wiffletree keeps the stock text the rewrite started from as its base. When a release changes that stock text, the rewrite is marked out of date and keeps running exactly as written. The human sees what changed as a line diff and has to copy in what they want by hand, then choose Mark as up to date. Every release that touches a rewritten skill asks for that work, and until it is done the human's agents miss the stock improvement.

The human decided on October 12, 2026 how updates to a rewritten skill work:
1. merge as plain text when the changes don't overlap;
2. use a model to merge when they do;
3. tell the human only when there is a real question.

The result applies without waiting for approval, with a note of what changed that can be reviewed or undone. "Merging" means bringing Wiffletree's updates into the human's copy; nothing is sent upstream. This is change 6 of 6 in the plan `plan-pr-keeper-and-skills.md`. It builds on skills (change 4), whose `base.md` exists for this merge, and on the settings repository (change 5), which syncs skills between Macs.

## What Changes

- **A new stock version is merged into a rewrite automatically.** When a rewrite is out of date, the host merges the stock change into it three ways: the base, the new stock text and the rewrite. Changes that don't overlap merge as plain text. The merged text and the new base are saved together as one change, with a history entry. Additions mode needs no merge, because its rules sit beside the stock text.
- **Overlap is decided by paragraph.** Stock texts have one paragraph per line, so a line diff is a paragraph diff. A stock change and a change in the rewrite overlap when they touch the same base paragraph, or neighbouring paragraphs with nothing unchanged between them. The plain-text merge is built on `similar`, which the workspace already uses. It needs no new dependency.
- **A model merges only the overlapping passages.** Everything else is merged as plain text first. The model gets each overlapping passage as the base, the stock version and the human's version, with the three whole texts for context. It runs on the PR keeper's model, with every tool turned off, like the keeper's comment sorting. Its answer is checked before anything changes. A merge needs no model when nothing overlaps. Offline, or with no Claude model for the PR keeper, the rewrite keeps running as written, and its page offers the human the same choice per passage.
- **The human is asked only when there is a real question.** That is when the model reports that the two versions of a passage want different things, so that keeping one drops the other's intent. Nothing changes then. A workspace note in the Inbox and the skill's page show the question with both wordings. The human chooses the update's wording or their own for each passage, and the merge then applies. A missing model or a failed try is not a question: the page says so, and nothing goes to the Inbox.
- **A note of what changed, with Review changes and Undo.** After a merge, the skill's page shows what was merged and how, and the Skills list marks the skill "Rewritten · updated". **Review changes** shows the rewrite before and after, each change labelled as coming from the update or as merged by the model. **Undo** puts back the exact text and base from before the merge, and stops that stock version being merged again on its own. The note stays until the human reviews or dismisses it on that Mac, or the skill changes again. The history always keeps the merge.
- **One merge for all Macs.** With a settings repository, a merge waits for the launch's first sync pass, so a Mac usually takes another Mac's merge instead of making its own. When two Macs merge at once, the first push wins. The other Mac takes that version through sync without pausing the skill, and merges again only if it is still out of date. A rewrite merged on a Mac with a newer release is not marked out of date, and is not merged, on a Mac with an older one.
- **Safety.** The fixed core is never part of a merge: it isn't in the shared folder, and the model is never sent it. The model writes no file: the host applies its answer through the skill store, which writes the files and the history entry together or not at all. Undo restores the human's earlier text byte for byte, from the merge's own history entry.
- **Not included:** merging into Additions (none is needed), sending changes upstream, merging the fixed core, merging two Macs' edits of the same rewrite (change 5 still pauses those), new skills, and a merge model of its own in Models (open question 1).

## Capabilities

### New Capabilities
<!-- None. Merging is part of the skills capability. -->

### Modified Capabilities
- `skills`: new requirements for merging a new stock version into a rewrite, the model's part, asking the human, and the note with Review changes and Undo, and for safety. "A rewrite is marked out of date by a new stock version", "Only the human changes a skill", "The Skills page" and "The limits of skills are documented" change to match.
- `settings-repository`: a new requirement that a skill update is merged once for all Macs.
- `model-routing`: "The PR keeper's model" also merges overlapping skill updates when it is a Claude model.

## Impact

- `workspace-host`: a new `skills/merge.rs` for the three-way merge and the model's input and answer; `skills.rs` gains merge, undo and answer changes, and the rule for when a merge is due; `provider.rs` gains a merge run, built like the sorting run; `sync.rs` takes another Mac's merge in place of this Mac's without pausing; host start and sync passes check for due merges.
- `workspace-core`: the merge's note, its review and an open question on `SkillState`; a second kind of workspace note; commands to undo a merge, merge on demand, answer a question and dismiss the note.
- `workspace-desktop`: the merge note, Review changes and Undo on a skill's page; the question with its choices on the page and in the Inbox; the "updated" and "needs you" marks in the Skills list; one line on the Models page's PR keeper card.
- Stored data: new history entry kinds in each skill's `history/`. No new table and no migration. The dismissed notes are kept in this Mac's database.
- Docs: `README.md`, `docs/PRD.md` and `docs/DEVELOPMENT.md`.
- Built in five slices, each its own PR of about 300 production lines or less; see tasks.md.

## Open questions

Each question has a recommendation. design.md is written as if every recommendation is accepted.

1. **Which model merges overlapping passages?** (a) The PR keeper's model, when it is a Claude model. The plan already calls it the small model. (b) A new choice on the Models page, "Skill updates", Claude only, with nothing chosen by default. (c) No model: every overlap goes to the human as a choice per passage. *Recommendation: (a).* It needs no new setting, and the job is a small, bounded text task like comment sorting. The run must be Claude because it runs with every tool off, which Codex can't do (`provider.rs:233`). (b) separates cost and quality, at the price of one more setting. (c) drops decision 2 of the plan.
2. **When does a merge run?** (a) At host start, after each sync pass that changed a skill, and when the human chooses Merge v2 in. With a settings repository, it waits for the launch's first pass, or for that pass to fail. (b) Only at host start. (c) Only after a pull. *Recommendation: (a).* A new stock text arrives only with a release, so host start finds most merges. A pass matters for a rewrite pulled from a Mac with an older release, and for a merge another Mac made first. Waiting only for the first pass means an offline Mac still merges.
3. **What does the human see while a model merge waits or fails?** (a) Only the skill's page and the existing out-of-date mark, with the reason and Try again; no Inbox note. (b) Also an Inbox workspace note. *Recommendation: (a).* The plan says to notify only for a real question. The rewrite keeps running as written, as it does today.
4. **How long does the note of what changed stay?** (a) On each Mac, until the human chooses Review changes or Dismiss there, or the skill changes again. (b) Seven days. (c) Until it is dismissed once, on any Mac. *Recommendation: (a).* Each Mac's agents run the merged text, so each Mac shows it. Dismissing is a view preference, not a change to the skill, so it isn't written to the shared history.
5. **What stops a merge from running again?** (a) Undo stops automatic merges into that stock version, until a newer one arrives. A waiting question stops them while the skill's rewrite and base are still the ones it asked about. Merge v2 in always works. (b) Only Undo. (c) A stock version is merged automatically at most once per skill, ever. *Recommendation: (a).* Undo must stick, or the merge would come straight back. A waiting question must not cost a model run on every launch. (c) also stops merging a rewrite that is restored or pulled later, which the human would have to notice and do by hand.
6. **When a question has several passages, does the rest of the merge wait?** (a) Yes: nothing applies until every passage is answered. (b) Apply the passages that merged and ask about the rest. *Recommendation: (a).* A rewrite's base must be one whole stock text. A partly applied merge would need a base no release ever had.
