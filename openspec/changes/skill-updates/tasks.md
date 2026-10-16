Five slices, each its own PR of about 300 production lines or less (tests and Markdown not counted), in this order. Each leaves the later ones out and is useful alone. Every requirement of the delta specs belongs to exactly one slice, the one that completes it, named at the head of that slice. An earlier slice may build part of it, and its tasks say which scenarios they verify. A slice's PR implements its tasks and syncs its own requirements, and only those, into the main specs. Line numbers are at d267fd6c, settings-repository slice 6. No slice makes a real model call or reads or writes the human's own settings or any real repository. Tests use temporary data folders, local bare repositories and a fake `claude`, as the sorting run's tests do.

## 1. Slice 1: the plain-text merge on the host

About 250 production lines. Requirements. `skills`: "A new stock version is merged into a rewrite". It also builds the host part of "A rewrite is marked out of date by a new stock version" and "A merge leaves a note that can be reviewed or undone", and part of "Only the human changes a skill" and "A merge never touches the fixed core and is always recorded". Until slice 3, a machine with a settings repository chosen makes no automatic merge, so two Macs cannot merge apart.

- [ ] 1.1 Add `workspace-host/src/skills/merge.rs`: the three-way line merge of design decision 3, built on `similar::capture_diff_slices`, returning the merged parts and the overlapping passages, each with its base, stock and human lines. Verify "Changes that don't overlap", "Neighbouring paragraphs overlap" and "The human already copied the change", and also both sides inserting at one place, the human deleting a paragraph the update changed, and the version line.
- [ ] 1.2 Add `Change::Merged` (from and to versions, the rewrite and base before, each change's source, the model and tokens when one ran, the machine) and `Change::UndidMerge` to `skills.rs` (`:78`). Apply a merge through `SkillStore::change` (`:327`) with this release's stock text as the base. Verify "A record that cannot be written".
- [ ] 1.3 Add the due rule of design decision 1, and a base newer than this release's stock text is no longer out of date (`Rewrite::out_of_date`, `workspace-core/src/lib.rs:167`). At host start, after `record_new_stock_versions` (`lib.rs:249`), merge each due rewrite that has no overlap, when no settings repository is chosen. Leave a rewrite with an overlap out of date. Verify "Additions need no merge", "A paused skill waits", and "A base from a newer release" on the host with a stand-in stock text.
- [ ] 1.4 Add `Command::UndoSkillMerge` and `Command::MergeSkillUpdate` beside the other skill commands (`lib.rs:2608`). Undo writes the entry's before rewrite and base back byte for byte. After it, no automatic merge into that stock version runs. Verify "Undo" on the host, with a rewrite that has trailing spaces and no final newline, and "An agent cannot edit a skill" (no tool in `mcp.rs::tools` merges, undoes or answers). Verify "The core after a merge" for a plain merge.
- [ ] 1.5 Run `cargo fmt --all --check`, Clippy with warnings as errors, all workspace tests, `openspec validate skill-updates --strict` and `openspec validate --all --strict`. Sync this slice's requirement.

## 2. Slice 2: the note, Review changes and Undo on the page

About 280 production lines. Requirements. `skills`: "A merge leaves a note that can be reviewed or undone", "A rewrite is marked out of date by a new stock version".

- [ ] 2.1 Put the latest merge's note on `SkillState`, unless dismissed on this Mac, and where an out-of-date rewrite's merge stands: undone, waiting for a model, failed with a reason, or based on a newer release. Keep dismissals in this Mac's database. Add `Command::DismissSkillMerge`. Verify "Dismissed on one Mac" with two data folders holding copies of one skills folder.
- [ ] 2.2 On the skill's page (`skills_view.rs::skill_page`, `:646`), add the note's banner with Review changes, Undo and Dismiss, and "Rewritten · updated" in the list (`state_label`, `:142`). Show the out-of-date banner's merge state and Merge v2 in. For a base from a newer release, show the version and no Mark as up to date. Update the rewrite confirmation (`REWRITE_COST`, `:43`), which says a rewrite is never updated. Verify "After an automatic merge", and "A release changes Coordinating" and "Marking it up to date" on the page.
- [ ] 2.3 Add a request for a merge's review, answered from its history entry, and the Review changes view: before and after, line by line, each change labelled. Add Undo's confirmation, and Undo this merge on a "Merged" history entry. Verify "Undo" from the page, and that Cancel sends nothing.
- [ ] 2.4 Compare by eye with `design/mockups/current/skill-updates.html` in Tidal Light and Tidal Dark: panels (a)'s "updated" row, (b), (c), (f)'s waiting, undone and newer-release banners, and (g). List every difference. Run the checks of 1.5 and sync this slice's two requirements.

## 3. Slice 3: one merge for all Macs

About 220 production lines. Requirements. `settings-repository`: "A skill update is merged once for all machines".

- [ ] 3.1 With a settings repository chosen, hold the host-start merges until this launch's first pass ends (`sync.rs::finish_sync`, `:1068`), whether it reached GitHub or not. After any pass that changed a skill's files, check that skill for a due merge. Verify "One Mac merges first" and "Offline at start" with two data folders and one bare repository.
- [ ] 3.2 In the pass (`sync.rs::pass`, `:767`), when both sides changed a skill's unit and this Mac's only change since it last synced is its own automatic merge, write GitHub's version here with no pause and no note, keep both machines' history entries, and check the skill again. Name the merge in sync's commit message. Verify "Both merge at once", "A Mac with a newer release" and "An edit after the merge".
- [ ] 3.3 Run the checks of 1.5 and sync this slice's requirement.

## 4. Slice 4: the model merge

About 300 production lines. Requirements. `skills`: "Overlapping passages are merged by a model", "A merge never touches the fixed core and is always recorded", "Only the human changes a skill". `model-routing`: "The PR keeper's model".

- [ ] 4.1 Add the bundled `MERGING` system prompt and its answer schema, and a merge run in `provider.rs` built like the sorting run (`:249`): Claude only, `--tools ""`, no MCP server, no session persistence, hooks off, an empty temporary folder, no workspace socket or token, and a 5-minute limit. Run it off the host thread, with no session, as the context probe runs (`context.rs:63`). Verify "What the model is sent" with a fake `claude` that records its arguments and input.
- [ ] 4.2 Check the answer as design decision 4 says and put the merged passages into the plain merge. Drop the answer when the skill's files changed meanwhile. Keep a failed try in memory until the next start or Try again. Record the model and tokens. Verify "An overlapping passage", "An answer that can't be used", "The human saved while the model ran" and "The core after a merge" with a model merge.
- [ ] 4.3 With no PR keeper model, or a Codex one, run nothing and offer the choice per passage on the page, applied with `Command::AnswerSkillQuestion`. Add the PR keeper card's line about skill merges (`models_view.rs::keeper_card`, `:389`). Verify "No model", "An agent cannot edit a skill" with the answer command, and "A Codex keeper model and a skill update" and the other scenarios of "The PR keeper's model". Compare by eye with the mockup's panel (f), its no-model and failed banners, in both themes, and list every difference.
- [ ] 4.4 Run the checks of 1.5 and sync this slice's four requirements.

## 5. Slice 5: a real question

About 280 production lines. Requirements. `skills`: "The human is asked only when there is a real question", "The Skills page", "The limits of skills are documented".

- [ ] 5.1 Record a model's question as an "Asked you about" entry holding the merge so far, with any passages the model merged, and each questioned passage's question and two versions. While its rewrite and base are unchanged, start no run. Answering through `Command::AnswerSkillQuestion` builds the merge from the entry with no model. Verify "No second run" and "Keeping my wording" on the host.
- [ ] 5.2 Add the skill question as a second kind of `WorkspaceNote` (`lib.rs:827`), shown at the top of every project's Inbox as paused files are (`inspector.rs:594`), with each passage's question, both wordings and the choices, and Apply for several passages. Show the same on the skill's page, "Rewritten · needs you" in the list, and the dot on the sidebar's Skills row. Verify "Two wordings that disagree", "Updated and needs you", "Opening the page", "The fixed core" and "Answered on another Mac" with two data folders and one bare repository.
- [ ] 5.3 Update `README.md` (skills), `docs/PRD.md` ("Agent communication and shared context") and `docs/DEVELOPMENT.md` (the merge, its model run and its entries). Verify "README".
- [ ] 5.4 Compare by eye with the mockup in both themes: panels (a)'s "needs you" row, (d) and (e). List every difference.
- [ ] 5.5 Run the checks of 1.5, sync this slice's three requirements, and archive the change.
