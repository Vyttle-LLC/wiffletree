## Context

See proposal.md for why. The code this builds on, at settings-repository slice 6, d267fd6c:

- **A rewrite and its base.** `SkillStore` (`workspace-host/src/skills.rs:120`) keeps each skill's current state in `skills/<skill>/` of the shared folder: `additions.md`, or `rewrite.md` with `base.md`, the stock text the rewrite started from or was last marked up to date against. `SkillStore::change` (`:327`) writes those files and one history entry together, or puts back what was there. Every history entry holds the skill's whole state after the change, the stock version and what happened (`Entry`, `Change`, `:70`, `:78`).
- **Out of date.** A rewrite is out of date when `base.md` is not this release's stock text, compared as text (`Rewrite::out_of_date`, `workspace-core/src/lib.rs:167`). Its page shows what changed from the base to the stock text with `line_diff` (`skills.rs:108`, built on the `similar` crate), with Mark as up to date and Reset to stock. The rewrite is never changed for the human, and the confirmation that starts a rewrite says so (`skills_view.rs:43`).
- **Stock texts.** Bundled in `workspace-host/skills/stock/`, each with a version on its first line ("# Coordinating — v1"), and one paragraph per line. They are 300 to 3,100 characters today. A rewrite or rules may be up to 32,000 characters (`MAX_SKILL_TEXT`).
- **New stock versions.** At host start, `record_new_stock_versions` (`skills.rs:261`, called at `lib.rs:249`) writes a "New stock version" entry for each skill whose history records only older stock versions. A machine on an older release writes none, and the skill's page says another machine runs a newer version (`find_newer_stock_versions`, `skills.rs:507`).
- **Sync (change 5).** The shared folder is a clone of the settings repository. A pass (`sync.rs:767`) runs off the host thread. It fetches, compares each file whole with the version last synced, writes changes made only on GitHub, commits this Mac's changes on GitHub's head and pushes, never by force. A skill's `additions.md`, `rewrite.md` and `base.md` are compared as one unit. When both sides changed the unit differently, it pauses, this Mac keeps its own version, and a workspace note appears at the top of every project's Inbox (`WorkspaceNote`, `lib.rs:827`). History entries are one file each and always sync. A pass runs at launch, every 5 minutes, on Sync now, and 5 seconds after a shared write.
- **Model runs.** Agents' turns are provider runs of a session (`service.rs:2972`). The PR keeper's comment sorting is the one run with no tools. It uses Claude with `--tools ""`, no MCP server, no session persistence and hooks off, a fixed `--system-prompt` and a `--json-schema` for its answer (`provider.rs:249`). Codex can't be started that way, so the host refuses a sorting run on Codex (`provider.rs:232`), and the comments trigger requires a Claude keeper model (`check_sorting_model`, `lib.rs:1026`). The PR keeper's model is one exact model chosen by the human in Models, kept on each machine, with nothing chosen by default (`ModelSelection::keeper`, `selection.rs:56`). The context probe (`context.rs:63`) shows how the host runs a one-off Claude process outside any session.

## Words

- **Update**: a release's new stock text for a skill.
- **Merge**: bringing an update into the human's rewrite, three ways. Nothing goes upstream.
- **Base**, **stock**, **yours**: the three texts of a merge. The base is `base.md`, the stock is this release's stock text, and yours is `rewrite.md`.
- **Passage**: a run of paragraphs that a merge treats as one. A stock text has one paragraph per line, so a line is a paragraph.
- **Overlap**: a passage that both the update and the human changed (decision 3).
- **A real question**: an overlap where the two versions want different things (decision 5).
- On the page, in the human's words: "Merged v2 into your rewrite", "Review changes", "Undo", "Needs you", "Take v2's wording", "Keep my wording", "Merge v2 in".

## Goals / Non-Goals

**Goals:** an update reaches a rewritten skill without the human's work when nothing they wrote is in question; a model is used only where plain text can't merge, and only on those passages; the human is asked only about real disagreements; every merge can be reviewed and undone to the exact earlier text; one merge for all Macs, with no pause; each slice small and useful alone.

**Non-Goals:**
- Merging in Additions mode. The rules sit beside the stock text and never clash (skills H6).
- Sending changes upstream, or merging the fixed core.
- Merging two Macs' own edits of one rewrite. Change 5 pauses those, and still does.
- Judging whether a merged rewrite is wise. Wiffletree doesn't judge a skill's text (skills H9).
- Counting merge runs on the Usage page, which counts runs by project.

## Decisions

### Who decided what

**Decided by the human on October 12, 2026** (the plan, "Updates to a rewritten skill" and "Skills page and editing"), numbered here H1 to H5:
- H1. Merge as plain text when the changes don't overlap.
- H2. Use a model to merge when they do.
- H3. Notify the human only when there is a real question.
- H4. The result applies without waiting for approval, with a note of what changed that can be reviewed or undone.
- H5. "Merging" means Wiffletree's updates into the human's copy; sending edits upstream is out of scope. Rewrite needs a merge on each new stock version; Additions never clash.

From change 4: authority is enforced by the app, never by text, and the fixed core is not a skill. From change 5: when two machines changed the same file, it pauses with a note in the Inbox.

**Everything else here is a design decision for the human to approve**, including the answers to the proposal's open questions 1 to 6. This design is written as if each recommendation is accepted.

### 1. What is merged, and when a merge is due

- Only a rewrite is merged. A skill in Additions mode needs nothing (H5).
- A rewrite is **due** a merge when all of these hold:
  - it is out of date: its base is not this release's stock text;
  - its base is not newer than this release's stock text (decision 8);
  - its files can be read, and sync has not paused them;
  - no Undo of a merge into this stock version is in its history (open question 5). Merge v2 in still works;
  - no question about this stock version is waiting, that is, asked about exactly the rewrite and base the skill has now (decision 5);
  - no merge of it is already running on this Mac.
- A due rewrite keeps running exactly as written until a merge applies, as it does today.

### 2. When a merge runs

Open question 2. A new stock text arrives only with a release, so most merges start at host start. A pull can't change this Mac's stock text. It can bring a rewrite that another Mac changed, so a pass can make a rewrite due or settle one.

- **At host start**, after the "New stock version" entries are written, the host checks each skill and starts a merge for each due rewrite.
- **After a sync pass** that changed a skill's files, the host checks that skill again. For example, a Mac on an older release saved a rewrite on the old base.
- **On demand**: the skill's page offers **Merge v2 in** on an out-of-date rewrite that no automatic merge will touch: after an Undo, after a failed try, or with a question waiting the human would rather answer by merging again.
- **With a settings repository**, the host-start merges wait for the launch's first sync pass to end, whether it reached GitHub or not. That pass usually brings another Mac's merge first, so this Mac merges nothing. A Mac that can't reach GitHub merges anyway; decision 8 covers what happens when it reconnects.

### 3. The plain-text merge

- A three-way merge of base, stock and yours, line by line. A line is a paragraph in every stock text, so a merge works by paragraph.
- Built on `similar`, already a dependency of `workspace-host` (`skills.rs:108`). The host diffs base→stock and base→yours with `similar::capture_diff_slices` on lines, then walks the base's lines and groups each side's changes into hunks: a range of base lines and the lines that replace it. About 80 lines in a new `skills/merge.rs`, with tests. **No new dependency.** *Not chosen:* the `diffy` crate's `merge`. It works, but it adds a second diff engine whose hunks would not match the line diff the page already shows, and it returns conflict markers that the host would parse back out.
- **Overlap.** Two hunks, one from each side, overlap when their base ranges share a line, when they touch with no unchanged base line between them, or when both insert at the same place. Overlapping hunks, and any hunk that overlaps one of them, form one **passage**. The rest is not overlap:
  - a hunk only the update changed takes the stock's lines;
  - a hunk only the human changed keeps theirs;
  - a passage both changed in the same way, as when the human already copied in the stock change by hand, is taken once.
- *Why neighbouring paragraphs count as overlap:* in prose, a paragraph added right after one the human rewrote often depends on it, and git's merge treats adjacent changes the same way. It sends a few more passages to the model, and never merges a dependent pair blindly.
- With no overlap, the merge result is the new rewrite and the stock text is the new base. They are saved with a "Merged" entry (decision 6). No model runs.
- The first line, "# Coordinating — v1", is a line like any other. When the human left it alone, it becomes "v2".

### 4. The model merge

- **Which model** (open question 1): the PR keeper's model, when it is a Claude model. A merge needs it only for passages that overlap.
- **What it is sent.** One JSON input: the skill's name; the base, stock and yours as whole texts, for context; and the overlapping passages, each with an id and its base, stock and human versions. Nothing else: no fixed core, no other skill, no project, no file. Every text in it is either bundled with Wiffletree or written by the human, so none of it is untrusted input. It still runs with every tool off, because it needs none.
- **The run.** A one-off Claude process, off the host thread, like the context probe and built like the sorting run: `-p`, `--tools ""`, `--strict-mcp-config` with no MCP server, `--no-session-persistence`, hooks off, the model and effort of the PR keeper's model, a fixed `--system-prompt` (`MERGING`, bundled), a `--json-schema` for the answer, in an empty temporary folder, with no workspace socket or token. Its time limit is 5 minutes. It belongs to no project, ticket or agent.
- **The instructions** say, in short: for each passage, write one version that keeps every change the human made and brings in what the update adds or changes; keep the human's wording where both can stand; add nothing that neither version says. When the two versions want different things, so that keeping one drops what the other means, don't merge that passage: answer one sentence saying what each wants.
- **The answer**: for each passage id, either `merged` with its text or `question` with one sentence.
- **Checks before anything changes.** The try fails, and nothing changes, when:
  - the answer doesn't match the schema, or doesn't answer every passage exactly once;
  - a merged passage is longer than the passage's stock and human versions together;
  - a question is longer than 300 characters;
  - the merged rewrite is over 32,000 characters, the limit for any rewrite.
- **Applied, or dropped.** When the skill's files changed while the model ran, because the human saved or a pass pulled, the answer is dropped and the merge is due again from the new state. Otherwise, merged passages are put into the plain merge and saved with a "Merged" entry. Questions go to the human (decision 5).
- **The fixed core is untouched by construction.** It is bundled, it is not in the shared folder, and it is never sent to the model. The model can only write passage text, and the host puts that only between the plain merge's parts. Every line outside an overlapping passage is exactly what the plain merge produced.
- **Cost.** One run per skill per stock version, across all Macs (decision 8), and only when something overlaps. A rewrite is at most 32,000 characters, so a run reads at most about 30,000 tokens and writes only the overlapping passages. With today's stock texts, a run reads a few thousand tokens. The "Merged" entry records the model and the tokens, and Review changes shows them. Merge runs belong to no project, so the Usage page doesn't count them.
- **No model, or offline.** With no PR keeper model, or one that isn't Claude, nothing changes: the rewrite runs as written. The page says the update and the rewrite both change N passages, links to Models, and offers the same choice per passage as a question (decision 5), without a model's sentence. A run that fails or can't reach Claude is kept in memory as failed. The page shows the reason and **Try again**, and the next host start tries again. No Inbox note (open question 3).
- **No fallback model.** As for the keeper (`model-routing`, "No fallback"), a failed run never moves to another model.

### 5. A real question

- **When.** Only when the model answers `question` for a passage (H3). Overlap alone is not a question, and neither is a missing model or a failed try.
- **Nothing applies meanwhile** (open question 6). The rewrite runs as written, and the base stays as it was. A rewrite's base must be one whole stock text.
- **It is recorded.** The host writes an "Asked you about v2" history entry that holds the merge so far, that is, the plain merge's parts and any passages the model merged, and, for each passage in question, the question and its stock and human versions. Every Mac can then show and settle the question without running the model again. Writing it also stops other Macs from running their own model (decision 1).
- **Where.**
  - A **workspace note**, a second kind beside change 5's paused files, at the top of every project's Inbox and counted in each: "Coordinating · v2 needs you". It shows each passage's question, then v2's wording beside the human's.
  - The skill's page, with the same question and choices.
  - The Skills list marks the skill "Rewritten · needs you", and the sidebar's Skills row shows its dot.
- **Choices.** For each passage: **Take v2's wording** or **Keep my wording**. With one passage, which is the usual case, a choice applies at once. With several, the note and the page show a choice per passage, and **Apply** once each has one. Applying writes the merge with a "Merged" entry. The rest of the page still offers editing by hand, Mark as up to date, and Reset to stock.
- **It closes** when the human chooses, on any Mac, or when the skill's rewrite or base changes in any other way. Any later change means the question is no longer about the text the skill has.

### 6. Applying a merge, and its history entries

- A merge is applied through `SkillStore::change`: `rewrite.md` and `base.md` are replaced and the entry is written together, or nothing changes. The new base is this release's stock text.
- New entry kinds:
  - **Merged v1 → v2**, written by an automatic merge or by the human's choices. It holds the state after the merge, like every entry. It also holds the rewrite and base from **before** the merge, exactly. It records each change as from the update, merged by the model, or chosen by the human; the model and tokens when one ran; and the Mac that merged.
  - **Undid the merge of v2**: the state put back.
  - **Asked you about v2**: decision 5. The skill's state is unchanged.
- Nothing is ever deleted from a history (change 4).

### 7. The note of what changed

- **Where.** On the skill's page, a banner above the rewrite: "Merged Coordinating v2 into your rewrite · 3 changes from the update, 1 passage merged by Claude haiku". It has **Review changes**, **Undo** and **Dismiss**. On the Skills list, "Rewritten · updated". No Inbox note and no dot (H3).
- **Review changes** shows the rewrite before and after, line by line. Each change is labelled "From v2" or "Merged by Claude haiku". A merged passage can be opened to show its base, v2's and the human's versions beside the result. It also names the Mac and the time.
- **Undo** asks first, naming what stops applying. It writes the entry's **before** rewrite and base back exactly, byte for byte, with an "Undid the merge" entry. The rewrite is then out of date again, no automatic merge into that stock version runs, and the banner offers **Merge v2 in**, Mark as up to date and Reset to stock. Undo is offered while the merge is the skill's latest change. After a later save, the merge's history entry still offers **Undo this merge**, which asks first and says the later changes stay in the history.
- **How long it stays** (open question 4). On each Mac, until the human chooses Review changes or Dismiss there, or the skill changes again. This Mac's database keeps which merges were dismissed. It is a view preference, not a change to the skill. The merge itself stays in the history.

### 8. Two Macs

- **The same new release, one after the other.** Mac A starts first and merges, and its next pass pushes the rewrite, the base and the entry. Mac B's host-start merge waits for its first pass (decision 2), which pulls A's version. B's base is then its stock text, so B merges nothing, runs no model, and shows the same note.
- **The same new release, at the same time.** Both merge before either pulls. A pushes first. B's push is refused, and its next pass finds that both sides changed the skill. Today that pauses the skill. With this change, **when this Mac's only change to a skill since it last synced is its own automatic merge, the pass takes GitHub's version without pausing**. B's merge was derived from the version both Macs last synced, which GitHub's version supersedes, so nothing the human wrote is lost. B's "Merged" entry stays in the history, with A's. B then checks the skill again (decision 2) and finds nothing due. A plain-text merge gives both Macs the same bytes, so for those the pass sees nothing to do. If B's human also edited the skill after the merge, it is a real two-machine edit, and it pauses as before.
- **Different releases.** Mac A runs v2 and Mac B runs v3.
  - When A merges v1→v2 first, B pulls it, finds the rewrite out of date against v3, and merges v2→v3.
  - When B merges v1→v3 first, A pulls a rewrite whose base, v3, is **newer** than A's stock text. A neither marks it out of date nor merges it, nor offers Mark as up to date, which would move the base back to v2. A's agents get the rewrite as written, and the page says "Merged with v3 on Mike's MacBook Air, which runs a newer Wiffletree." A newer base is told by the version number on its first line.
  - When both merge at once, the first push wins, as above. If A's v2 merge wins, B takes it and merges v2→v3. If B's v3 merge wins, A takes it and leaves it.
- **Offline.** A Mac that can't reach GitHub merges on its own after its first pass fails (decision 2). When it reconnects, the cases above apply. If another Mac merged meanwhile, this Mac's merge is replaced without a pause.
- **Questions.** "Asked you about v2" is an entry, so it reaches every Mac. Every Mac shows the note, and a Mac whose first pass brought it runs no model. An answer on one Mac changes the files, which sync, and the note closes everywhere.
- The settings repository's commit message names the merge: "Coordinating: merged v2 into your rewrite · Mike's MacBook Air".

### 9. Safety

- **The fixed core is never merged.** It is not a skill, not in the shared folder, and never sent to the model. A test builds every role's instructions after a merge and finds the whole core.
- **A model never changes a skill without a history entry.** The model has no tools and writes no file. Its answer is checked and applied only by the host, through `SkillStore::change`, which writes the files and the entry together.
- **Undo restores the human's exact earlier text.** The "Merged" entry holds the rewrite and base from before the merge, and Undo writes those bytes. It doesn't rebuild them from a diff or an older entry. A test compares bytes, including trailing spaces and a missing final newline.
- **Only the human's client sends** Undo, Merge v2 in, answers and Dismiss. No agent tool reaches them or the skills folder. The host's own automatic merge is the one change to a skill the host makes without a command.
- **Limits stay.** A merged rewrite is held to 32,000 characters like any rewrite. Wiffletree doesn't judge whether the merge is wise. The note and Undo are the human's check.

### 10. The snapshot and commands

- `SkillState` gains: the latest merge's note when not dismissed on this Mac (versions, counts, model, Mac, time); the merge state of an out-of-date rewrite (waiting for the first pass, merging, failed with a reason, undone, needs a model, a newer base on another Mac); and a waiting question with its passages.
- `WorkspaceNote` gains a second kind, a skill question, beside a paused file.
- New commands from the human's client: `UndoSkillMerge`, `MergeSkillUpdate` (Merge v2 in, or Try again), `AnswerSkillQuestion` (a choice per passage) and `DismissSkillMerge`. The before-and-after review is asked for when it opens, as the history and the diff are today.

### 11. What the tests check

- The plain merge: no overlap; same-paragraph overlap; neighbouring paragraphs; both inserting at one place; the same change on both sides; the human deleting a paragraph the update changed; the version line.
- The model merge with a fake Claude: the arguments match the sorting run's restrictions and the input holds no core; each failing check leaves the files and history unchanged; an answer is dropped when the files changed meanwhile.
- Undo restores bytes exactly; a question stops further runs and closes on any other change.
- Two data folders and one bare repository: the merges of decision 8, with no pause and one model run where the first pass brings the other Mac's merge.

## Risks / Trade-offs

- **A model may merge badly.** Its passages apply without approval (H4). It touches only overlapping passages, its answer is bounded and checked, the note says which passages it merged, and Undo is exact.
- **A merge changes every agent's next turn** on every Mac. It is the same reach as any saved edit (change 4), and the note and the history show it.
- **Paragraph granularity** sends some merges to the model that a word-level merge would combine. That keeps the plain merge predictable and the page's line diff consistent with it.
- **Two Macs can each run a model** when they merge at the same moment while both are online but before either pulls. That costs one extra run. The result is still one version, with no pause.
- **An older Mac runs a rewrite merged for a newer release.** Its text may describe behaviour this Mac's core lacks. Change 5 already allows this for a rewrite saved on a newer release, and the page says so.
- **Undo is sticky for that stock version.** A human who undoes a merge and later wants it uses Merge v2 in.

## Mockup

See `design/mockups/current/skill-updates.html`, in Tidal Light and Tidal Dark:
- (a) the Skills list with "Rewritten · updated" and "Rewritten · needs you", and the dot on the Skills row;
- (b) a skill's page after an automatic merge, with its note, Review changes, Undo and Dismiss;
- (c) Review changes, with a passage the model merged;
- (d) a merge that needs the human, as a workspace note in the Inbox;
- (e) the same question on the skill's page;
- (f) where an out-of-date rewrite's merge stands: waiting for the first sync, no model, a failed try, after Undo, and a base from a newer release;
- (g) Undo's confirmation and the history with a merge entry.
