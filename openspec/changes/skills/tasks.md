Six slices, each its own PR of about 300 production lines or less (tests and Markdown not counted), in this order. Each leaves the later ones out and is useful alone. Every requirement of the delta specs belongs to exactly one slice, the one that completes it, named at the head of that slice; an earlier slice may build part of it, and its tasks say which scenarios they verify. A slice's PR implements its tasks and syncs its own requirements, and only those, into the main specs. Line numbers are at origin/main 9abbd5f, checked again when slice 1 started after PR keeper slice 5 (#79) merged (decided question 11).

## 1. Slice 1: the fixed core and four stock skills

About 80 production lines, and the text moved. Requirements. `skills`: "Instructions are a fixed core and a skill", "The fixed core holds what the app depends on", "Stock skills are bundled and versioned". `ticket-publishing`: "Instructions state the publishing rules in Wiffletree's own words".

- [ ] 1.1 Split the files as design decision 2 says into `skills/core/` (`communication.md` v9, `coordinator.md`, `implementer.md`, `verifier.md`, `keeper.md`, each "… core — v1" except the contract) and `skills/stock/` (`coordinating.md`, `implementing.md`, `reviewing.md`, `keeping-a-pr.md`, each "… — v1"). Move sentences without rewording; where one must be cut in two, list it in the PR description with its old and new wording. Add the contract's new sentence and the fixed heading and sentence for the human's own rules (design decision 3) to the core.
- [ ] 1.2 Add `Skill` (`Coordinating`, `Implementing`, `Reviewing`, `KeepingAPr`) with `Skill::for_role(Role) -> Skill`, since every role has one, its name, what it is for and its stock text, and make `provider::skill(role)` (`provider.rs:115`) return the core and the stock skill in design decision 3's order. Leave `keeper::SORTING` (`keeper.rs:25`) on the keeper core alone. Verify "Every skill at stock", "A tester and a reviewer" and "The sorting run".
- [ ] 1.3 In `tests/contracts.rs`, add a helper that builds a role's instructions with every skill rewritten to an empty text, and move every assertion that names a tool, a report kind, the findings format, a message from Wiffletree or the publishing paragraph onto it, `LEVEL_RULES` (`contracts.rs:279`) included: the core keeps the whole sentence on choosing a level by its `use_when` line (design decision 2, paragraph 5), as `verify_ticket`'s description does. Keep the stop rule, the objection rule, the blocking definition and who runs the full suite on the stock instructions. Update the version assertions. Verify "An empty rewrite" (the core's), "A stock statement", and the ticket-publishing scenarios "No pull request skill" and "Merging". With the human's rules passed to the composition, verify "Rules that contradict the core" (the rules as written, the core's publishing paragraph unchanged; `publish_ticket` and its tests are not touched) and the ticket-publishing scenario "The human's own PR rules".
- [ ] 1.4 Add a test that, with every skill at stock, each role's instructions contain every sentence of today's files except the reworded ones listed in 1.1. Verify "Every skill at stock" and "A fresh install or an update" (no skills folder exists yet, so nothing is read).
- [ ] 1.5 Update the PRD's "Agent communication and shared context" sentence on bundled instructions and `docs/DEVELOPMENT.md`'s description of the skill files.
- [ ] 1.6 Run `cargo fmt --all --check`, Clippy with warnings as errors, all workspace tests, the deterministic communication checks, `openspec validate skills --strict` and `openspec validate --all --strict`. Sync this slice's requirements into `openspec/specs/skills` and `ticket-publishing`.

## 2. Slice 2: the human's rules, stored and sent

About 260 production lines. Requirements. `skills`: "Additions are the default way to edit a skill", "An edit applies from each agent's next turn", "Skills are stored in a folder that can move". It also builds the host part of "Only the human changes a skill", "Every skill can be reset to stock" and "Every skill has a history".

- [ ] 2.1 Before any code depends on it, check with the installed `claude` and `codex` on a throwaway session in an empty folder that a resumed session follows a changed `--append-system-prompt` or `developer_instructions` on its next turn: give a first turn an instruction to answer "one", resume with one to answer "two". Record the CLI versions and the result in `docs/DEVELOPMENT.md`. If either keeps the old instructions, stop and report before going on.
- [ ] 2.2 Add `workspace-host/src/skills.rs` with `SkillStore` rooted at `<data folder>/skills`: read a skill's state (missing or unreadable files count as none, and an unreadable one is reported), save the human's rules atomically like `settings.rs::save_settings` (`:245`), and write one history entry per change as `history/<time>.json` with the skill's whole state, the stock version and what happened. Verify "Saving rules" (the file and one entry), "A file that cannot be read" and "A fresh install or an update" with a folder that does not exist.
- [ ] 2.3 Make the host build each turn's instructions: replace `provider::skill(role)` with `Host::instructions(role)`, which reads the store, and carry the text on `Turn` (`provider.rs:70`), set where turns are built (`service.rs:2532`, `:2936`) and used by `configure` (`provider.rs:239`) and `context.rs::claude` (`:73`). Verify "An implementer with the human's rules", "A new stock version under the human's rules", "A running implementer" (with the fake provider: the turn that was running keeps its arguments, the next one has the rules) and that the context probe's text equals the turn's.
- [ ] 2.4 Add `Command::SaveSkillRules`, `ResetSkill` and `RestoreSkill` beside `SetKeeper` (`workspace-core/src/lib.rs:2289`), refusing a text over 32,000 characters. Restore writes the entry's state and a new entry. Verify "A text too long", "An agent cannot edit a skill" (no tool in `mcp.rs::tools` changes a skill), "Restoring earlier rules" and, on the host, "Resetting a skill with rules".
- [ ] 2.5 At host start, write a "New stock version" entry for each skill with a history whose latest entry names another stock version. Verify "An update", and that a skill with no history gets none.
- [ ] 2.6 Put each skill's state on the snapshot (design decision 10), without its history. Run the checks of 1.6 and sync this slice's three requirements.

## 3. Slice 3: the Skills page and the human's rules

About 290 production lines. Requirements. `skills`: "The Skills page".

- [ ] 3.1 Add `Page::Skills` (`main.rs:73`) and the Skills row after Models in `sidebar.rs::workspace_nav` (`:322`). Verify "Opening the page".
- [ ] 3.2 Add `workspace-desktop/src/skills_view.rs`: the list of the four skills with what each is for, its roles and its state, and the fixed core marked read-only; the fixed core's page with its text and no editor or Reset. Verify "The fixed core".
- [ ] 3.3 A skill's page in Additions mode: the stock text read-only and folded, the editor for the human's rules with Save and Discard as on the Models page, "Applies from each agent's next turn", and the box of what the app enforces (design decision 4). The Reviewing page shows each review level's "Use it" line read-only with a link to Models; the Coordinating page links to the model guide; the Implementing page's hint suggests putting PR rules in the human's own rules. A refused save shows the host's error toast. Verify "Saving rules" end to end and "A text too long" on the page.
- [ ] 3.4 Compare by eye with `design/mockups/current/skills.html` in Tidal Light and Tidal Dark on a throwaway store: panels (a), (b), (c) and (h). List every difference in the ready_for_testing report.
- [ ] 3.5 Add the Skills page to `README.md`, with the limits of "The limits of skills are documented" that hold so far. Run the checks of 1.6 and sync this slice's requirement.

## 4. Slice 4: Reset to stock and the history on the page

About 200 production lines. Requirements. `skills`: "Every skill can be reset to stock". It also builds the page part of "Every skill has a history", which slice 5 completes.

- [ ] 4.1 Add Reset to stock to a skill's page with its confirmation, naming what stops applying and that it stays in the history. Verify "Resetting a skill with rules" and "Cancelling" (no command is sent).
- [ ] 4.2 Add a request for a skill's history, answered from `SkillStore`, and the History list on the skill's page, newest first, each entry with View (read-only) and Restore. Verify "Restoring earlier rules" from the page, and that the list reads an "An update" entry as "New stock version v1 → v2".
- [ ] 4.3 Compare by eye with the mockup in both themes: panels (e) and (f). List every difference. Run the checks of 1.6 and sync this slice's requirement.

## 5. Slice 5: Rewrite on the host

About 200 production lines. Requirements. `skills`: "Only the human changes a skill" (its last commands) and "Every skill has a history" (restoring a rewrite). It also builds the host part of "A rewrite replaces the stock text on purpose" and "A rewrite is marked out of date by a new stock version".

- [ ] 5.1 Add `Command::StartSkillRewrite`, `SaveSkillRewrite` and `MarkSkillUpToDate` to `SkillStore`: starting writes `rewrite.md` as the stock text followed by the human's rules under their heading, `base.md` as the stock text, and removes `additions.md`; `Host::instructions` sends the rewrite in place of the stock text. Restoring a rewrite entry restores its base. Verify "Starting a rewrite with rules", "An empty rewrite" (the skills scenario) and "Restoring an old rewrite".
- [ ] 5.2 Compare `base.md` with the stock text to set the snapshot's out-of-date flag, and answer a request for what changed with a line diff of the base and the stock text, adding the `similar` crate to `workspace-host`, since the workspace has no line diff (none at 9abbd5f). Verify "A release changes Coordinating" with a stand-in stock text, "Marking it up to date" and "A stock change without a new version".
- [ ] 5.3 Make `ResetSkill` on a rewritten skill remove `rewrite.md` and `base.md` with `additions.md`, clear the out-of-date flag and write one entry. Verify "Resetting a rewritten skill" on the host: the files are gone, the next turn's instructions have the stock text, and the previous entry holds the rewrite and its base.
- [ ] 5.4 Run the checks of 1.6 and sync this slice's two requirements.

## 6. Slice 6: Rewrite on the page

About 260 production lines. Requirements. `skills`: "A rewrite replaces the stock text on purpose", "A rewrite is marked out of date by a new stock version", "The limits of skills are documented".

- [ ] 6.1 Add "Rewrite the stock text…" to a skill's page with its confirmation, and the Rewrite mode page: the editor holding the rewrite, Save and Discard, Reset to stock and History. Verify "Starting a rewrite with rules" from the page, and "Resetting a rewritten skill" from the page: the confirmation names the rewrite, and Cancel sends nothing.
- [ ] 6.2 Add the out-of-date mark to the list, the dot on the sidebar's Skills row, and the banner with both versions, what changed as added and removed lines, Mark as up to date and Reset to stock. Verify "A release changes Coordinating" and "Marking it up to date" on the page.
- [ ] 6.3 Complete `README.md` and `docs/DEVELOPMENT.md`: a rewrite is not updated by new releases, and agents' shells can write the skills folder. Verify "README".
- [ ] 6.4 Compare by eye with the mockup in both themes: panels (d) and (g), and (a)'s out-of-date row and the dot on the Skills row. List every difference.
- [ ] 6.5 Run the checks of 1.6, sync this slice's three requirements, and archive the change.
