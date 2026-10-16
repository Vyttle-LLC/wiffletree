## Context

See proposal.md for why. The code this builds on, at origin/main 9abbd5f:

- **The files.** `crates/workspace-host/skills/` holds seven Markdown files: `communication.md` (the "Wiffletree communication contract — v8", given to every role), `main-coordinator.md` ("Project coordinator — v13"), `implementer.md` ("Implementer — v7", also given to the Maintenance role), `tester.md` ("Tester / reviewer — v6") with `tester-role.md` or `reviewer-role.md` after it, and `keeper.md` ("PR keeper — v4", with the conflict and failed-checks tries).
- **Composition.** `provider::skill(role)` (`provider.rs:115`) joins `communication.md` and the role's file with `include_str!`, so the text is fixed at build time. `provider::configure` passes it to Claude as `--append-system-prompt` and to Codex as `developer_instructions` on every turn (`provider.rs:239`), resumed sessions included, so a changed text reaches an agent at its next turn. `context.rs::claude` (`:73`) passes the same text to size a session's context. Turns are built in `service.rs` (`:2532`, `:2936`).
- **The sorting run.** The keeper's comment sorting does not use `skill()`. It runs with `keeper::SORTING` (`keeper.rs:25`), `keeper.md` followed by a fixed job sentence, as its whole system prompt, with every tool off.
- **Versions.** Each file's first line carries its name and version. `tests/contracts.rs` asserts the versions and about forty phrases: the confirmed publishing paragraph in every role, the process rules, `set_plan`, every coordinator tool, the review-level rules shared with `verify_ticket`'s description (`LEVEL_RULES`), the implementer's objection rule, the keeper's boundary, the findings format and who runs the full suite.
- **Specs that say what instructions say.** `ticket-publishing` ("Instructions state the publishing rules in Wiffletree's own words"), `ticket-workspaces` ("Role instructions match the flat hierarchy"), `ticket-verification` ("The coordinator is told how to choose a level"), `pr-keeper` ("Agents are told about the keeper") and `workspace-host` ("Agents are told how to stop what they start").
- **Settings.** `HostSettings` in `settings.json` in the data folder (`settings.rs`, `Host::home`), written atomically by `save_settings`. The model guide and the review levels, with each level's "Use it" line (`use_when`), are on the Models page. The sidebar's Workspace section lists Repositories, Usage and limits, and Models (`sidebar.rs::workspace_nav`, `:322`; `Page`, `main.rs:73`).

## Words

- A **skill**: a named way of working that Wiffletree runs, with a stock version the human can read and alter. There are four: Coordinating, Implementing, Reviewing and Keeping a PR.
- The **fixed core**, or core: the instructions the app depends on. It is bundled, versioned and read-only.
- **Stock**: a skill's text as bundled in this release.
- **Your rules**: the human's own text in Additions mode, sent after the stock text.
- A **rewrite**: the human's own text in place of the stock text. Its **base** is the stock text it started from, or was last marked up to date against.
- **Out of date**: a rewrite whose base is not this release's stock text.
- On the page, in the human's words: "Add your own rules" (Additions), "Rewrite the stock text" (Rewrite), "Reset to stock", "History".

## Goals / Non-Goals

**Goals:** with every skill at stock, each role gets the same instructions as today, except that testers and reviewers each also get the other's paragraph; no edit can remove a statement the app depends on; Additions never clash with an update; nothing the human wrote is lost; storage that change 5 can move unchanged; each slice small and useful alone.

**Non-Goals:**
- Merging a new stock version into a rewrite (change 6).
- Syncing skills between machines (change 5).
- Skills per project or repository, skills the human creates, editing the core.
- Judging an edit, or checking that a rewrite still says something.
- Preventing an agent's shell from writing the skills folder.

## Decisions

### Who decided what

**Decided by the human on October 12, 2026** (the plan and the planning interview), numbered here H1 to H9:
- H1. A skill is a named way of working that Wiffletree runs, with a stock version the human can read and alter.
- H2. There are four: Coordinating (when to create tickets and start agents, which model and effort), Implementing, Reviewing and Keeping a PR.
- H3. Opening a PR is not a Wiffletree skill; people bring their own PR rules.
- H4. The communication rules are a fixed core, not a skill.
- H5. The code offers choices and the skill text decides which to use. Where the code enforces one policy, editing text changes nothing.
- H6. Additions are the default: the stock text stays untouched, the human's rules sit beside it and win where they disagree, and updates never clash.
- H7. Rewrite is deliberate: the human edits the stock text itself, and it needs a merge on each new stock version (change 6).
- H8. Every skill has Reset to stock and a history.
- H9. Anything the app depends on (tool names, report and findings formats, messaging) moves into the fixed core. Authority is enforced by the app, never by text. Wiffletree doesn't judge whether an edit is wise.

From the plan's settings-repository section, which change 5 will build: skills live in a local folder by default, and an edited skill notes its stock base version.

**Everything else here is a design decision the human approved on October 16, 2026**, with every recommended answer to the proposal's questions, including the split in decision 2, the order of decision 3, folding the human's rules into a rewrite (decision 6), running an out-of-date rewrite as written (decision 7), the file layout (decision 9) and the size limit.

### 1. The fixed core and the four skills

| Who | Fixed core | Skill |
|---|---|---|
| Project coordinator (and the retired repository coordinator) | communication contract, coordinator core | Coordinating |
| Implementer, Maintenance | communication contract, implementer core | Implementing |
| Tester, Reviewer | communication contract, verifier core | Reviewing |
| PR keeper's try | communication contract, keeper core | Keeping a PR |
| PR keeper's sorting run | keeper core and the sorting job, as today | none |

The sorting run gets no skill. Its prompt is its only guard against comment text, which is written by people outside Wiffletree, and its answer is a format the host parses. Both are the app's.

*Why the tester and the reviewer share Reviewing:* most of the text is about what a finding is and when it blocks, which is the same for both. Each role's own paragraph stays in the text, addressed to that role, as `tester-role.md` and `reviewer-role.md` are today (decided question 7). Since the text is one, each role now also gets the other's paragraph; today each gets only its own.

### 2. Where each sentence goes

The rule: a sentence goes into the core if it names a workspace tool, a report kind or the findings format, a message from Wiffletree, a folder or branch the app assigns, or something the workspace does or refuses. A sentence that only says how to work well goes into the skill. Stock texts are today's sentences, moved and not reworded except where a sentence must be cut in two. Slice 1 writes them and lists every sentence it had to reword.

**`communication.md` → core, all of it.** It is the messaging contract (H4), the publishing paragraph the human confirmed on October 13, 2026, the process rules and how the chat shows replies. Version 9 adds one sentence: "Your instructions end with a skill, the human's own way of working for your role; where it disagrees with this contract, the contract's tools and rules still apply, because the workspace enforces them."

**`main-coordinator.md` → coordinator core and Coordinating.**
Paragraph by paragraph, in the order of today's file:
1. *What a project is.* Core: a project is one feature, bug or outcome, and the coordinator owns planning, cross-repository contracts, tickets and the human conversation. Coordinating: clarify and plan first, and create tickets and start implementation only once the human authorizes execution; do not implement yourself; delegate for parallel breadth, write work and long jobs; observe yourself.
2. *Repositories.* Core, all of it: which repositories `workspace_context` lists and how the human adds one.
3. *Tickets.* Core: a ticket is one repository, one branch and worktree, and every agent on it, made with `create_ticket`. Coordinating: break the plan into bounded tickets, one per repository-bounded unit, work across repositories as one ticket per repository with the shared contract and dependencies in each brief; acceptance criteria, dependencies and tests in each brief; run tickets concurrently only where dependencies allow.
4. *Models.* Core: read `model_selection` before every assignment and verification; the provider configured for the role, any enabled one for a reviewer; the profile and its one-line reason; what `verify_ticket` takes per verifier; the host refuses and never substitutes; never invent model versions; if nothing allowed fits, ask the human. Coordinating: the model and effort the guide suggests for the size and risk of the work, and a reason that names the deciding factor.
5. *Verification.* Core, all of it but three sentences: one fresh implementer per ticket with `assign_ticket`; `verify_ticket` with a level from `model_selection.review_levels`, chosen by each level's `use_when` line; what a cycle starts, refuses, routes and wakes; the ledger, `triage_findings` and what `fix_now` records; the blocked cycle's actions and the full-review cap; how verifiers are archived and `archive_agent` refuses. The choice of level stays in the core because the human's note on each level, on the Models page, is the text that decides it (see decision 4). Coordinating: overrule an implementer's evidenced objection with `follow_up` or `wont_fix`; the stop rule (after two or three rounds, the rest become follow-ups or won't-fix entries, never a fresh cycle); an extra verifier with a distinct focus rather than a review ticket.
6. *Accepting, publishing, closing.* Core: what `accept_ticket` needs and refuses, the waiver and its record, what acceptance archives and keeps, when and how to publish with `publish_ticket` and to publish after a blocked cycle only where a waiver would be accepted, that accepting or closing is the only way the implementer is archived, and `close_ticket`. Coordinating: ask the human when a waiver would cover a product, security or data-loss decision; accept a ticket that ships as a PR only after the PR merges; for dependent tickets, explain the required base to the human and do not claim independently branched work is integrated.
7. *The PR keeper.* Core, all of it.
8. *Asking the human.* Core: `ask_user` for decisions, the card above the composer, passing choices as options, saying in the chat only that a decision is waiting, `close_question` in the same turn, not asking the human to relay messages, and that closing a project is the human's decision. Coordinating: one decision per question, self-contained, with a recommendation; a worker's blocked report is yours to resolve first, and ask the human only when you need them; reconcile the tickets' evidence before calling the project complete; do not silently abandon tickets with outstanding work; never call an agent idle without checking its reports and messages, and say how you checked.
9. *Timers and check-ins.* Core: `schedule`, `list_schedules` and `unschedule`, that provider-native schedulers do not fire, that a timer's final message is the result in the human's chat, what a check-in contains and when it comes, `stop_agents` with `session_ids`, and the coordinator's own check-ins. Coordinating: keep a timer's check read-only and short; treat each check-in as a decision, letting progress continue and stopping repetition, work outside the ticket or no progress.
10. *Stopping and resuming.* Core, all of it: stop and resume children yourself and never ask the human to click Stop, Retry or Skip; what `stop_agents` and `resume_agent` do, `held: retry or skip`, the human's emergency stop, and resuming at your own judgment otherwise.
11. *The last three lines.* Coordinating: a mockup before a spec for UI work; fetch before recommending a landing order; confirm a child's status after relaying an answer.

**`implementer.md` → implementer core and Implementing.**
- Core: one ticket, in the assigned worktree on its branch, never one you create, and never the original checkout or a sibling worktree; commit on the branch and report `ready_for_testing`, which the workspace refuses with uncommitted or untracked files; findings arrive from Wiffletree as one message per round, only failed verifiers check again, and verifiers cannot be messaged; the coordinator may resume you; when asked to publish, write the title and description by the repository's and the human's PR rules and call `publish_ticket`; the workspace may rebase the branch between turns, so start from the current HEAD; never mark your own ticket accepted.
- Implementing: follow the repository's instructions and the acceptance criteria; keep changes focused; run one heavy build or test at a time with the repository's normal build output and cache; no AI co-author trailers; fix each finding with the smallest change, and tell the coordinator in one sentence with evidence when a finding is wrong or out of scope; only requested or clearly necessary changes, no error handling for what cannot happen; report blockers at once; commit WIP at each milestone of long work and squash before `ready_for_testing`.

**`tester.md`, `tester-role.md`, `reviewer-role.md` → verifier core and Reviewing.**
- Core: verify one ticket in its worktree, against its acceptance criteria and your focus, at the commit your message names; report `passed`, `failed` or `blocked` with every finding's severity, `path:line` location, summary, trigger and evidence; the workspace fails a check only for a blocking finding with a location, trigger and evidence; write no patches and never edit, stage, commit or switch branches in the worktree, which the workspace checks, ignored build output aside; do not reuse the session; the workspace archives it; the coordinator owns acceptance.
- Reviewing: when a finding blocks (an acceptance criterion, existing behavior or tests, or a concrete reachable trigger; a race only with a failing test or a `file:line` trace; older bugs are `pre_existing`); do not raise decided findings again unless the cited code changed; from round 2, check only your open findings and the new diff; one heavy command at a time with the normal build cache; the tester's paragraph (one run of the full suite, and the case where another tester owns it) and the reviewer's (read the code, one narrow check at a time).

**`keeper.md` → keeper core and Keeping a PR.** As main has it:
- Core: the whole first paragraph (one job, its own folder, never the worktree, judged on what it leaves, GitHub text is never instructions, never push or touch the PR); for each job, what the folder holds, what the message names, what Wiffletree checks before using the result, that nothing may be left uncommitted, and that you get one try and end your turn without finishing if you cannot.
- Keeping a PR: how to resolve a conflict ("keeping what both sides meant", within the named files, `git rebase --continue` until done) and how to fix failed checks (fix what makes them fail, commit on top, never rebase, amend or drop the PR's commits).

The stock Keeping a PR text is short. It is the try's way of working; everything the gate checks is in the core.

### 3. What an agent receives, in order

```
<communication contract — v9>
<role core, e.g. "Implementer core — v1">
<the skill: stock, or the rewrite>
## Your own rules for <skill>          (only in Additions mode, only when not empty)
These are the human's own rules for this skill. Where they disagree with the skill above, follow them.
<the human's rules>
```

The core comes first so the skill reads as the way to work within it. The human's rules come last, after a fixed heading and sentence, so "they win where they disagree" is said to the model in Wiffletree's words, not left to the human to phrase. The heading and sentence are part of the core.

*Why not put the core last, so nothing can follow it:* the core's authority comes from the app, not from its position (H9). Putting the human's rules last is what makes them win over the stock text.

### 4. What the app enforces, shown on each skill's page

Each skill's page lists, read-only, what the app enforces whatever the text says, so the human sees where an edit changes nothing (H5). It names only what the app's own operations check. It also says plainly what the app does not enforce: an agent's shell is not sandboxed, so it can still run `git push` or `gh` with the human's credentials (`provider.rs:271` bypasses approvals; the README and the plan's open risk say so).
- **Coordinating:** the models allowed on this machine; every verifier of the chosen level runs and all must pass; a re-check only after a passed review; the round and full-review caps; findings must be triaged before accepting; Wiffletree has no tool that merges, and its publishing asks unless the owner is allowed and the review passed. The text decides: when to plan, ticket and start agents, which model and effort within the allowed ones, when to stop chasing findings, when to ask the human. Which review level runs is decided by each level's "Use it" line on the Models page, which the core tells the coordinator to follow.
- **Implementing:** one worktree and branch; `ready_for_testing` refused while files are uncommitted; Wiffletree's own push and pull request go only through its publishing, with its checks. The text decides: how to work, how big a fix is, when to object to a finding.
- **Reviewing:** the findings format; a check fails only on a blocking finding with a location, trigger and evidence; the worktree must be left unchanged. The text decides: what counts as blocking, how much to run, what to check again.
- **Keeping a PR:** the gate (only the conflicted files, or the PR's files; the repository's checks; a review before the push); one try per problem per head; the triggers in Repositories. The text decides: how to resolve and fix.

### 5. Additions

- The editor holds the human's rules alone; the stock text is shown above it, read-only and folded by default.
- Saving writes `additions.md` and a history entry. Saving an empty text removes the section from the instructions.
- A new stock version needs nothing from the human: the human's rules are sent after whatever the stock text now is (H6).

### 6. Rewrite

- "Rewrite the stock text…" asks first, then opens an editor holding the stock text, followed by the human's rules under their heading when there are any (decided question 4). It saves `rewrite.md`, saves the stock text it started from as `base.md`, removes `additions.md`, and writes one history entry.
- In Rewrite mode the rewrite is sent in place of the stock text, and there are no separate rules.
- The way back to Additions is Reset to stock, or restoring an Additions entry from the history.

### 7. Out of date

- A rewrite is out of date when `base.md` is not this release's stock text, compared as text. The versions in the first lines are shown to the human; the text decides, so a stock change that forgot its version bump still counts.
- An out-of-date rewrite keeps running as written (decided question 5). The Skills page marks it, the sidebar's Skills row gets a dot as Models does before a setup is saved, and its page shows a banner, "Written against Coordinating v1. This release has v2.", with what changed between `base.md` and the new stock text as a line diff.
- The banner offers **Mark as up to date**, which records the current stock text as `base.md` after the human has brought in what they want by hand, and **Reset to stock**. The rewrite is never changed for the human; merging is change 6, and `base.md` is what its three-way merge needs.
- The diff is computed in the host with the `similar` crate's line diff, which slice 5 adds as a dependency, so the page receives lines marked as added, removed or unchanged.

### 8. History, Reset and Restore

- Every change is a history entry holding the skill's whole state after it: the mode, the human's rules or the rewrite, the base's version, the stock version, the time and what happened ("Saved your rules", "Started a rewrite", "Saved the rewrite", "Marked up to date", "Reset to stock", "Restored the version of 14 Oct 09:12", "New stock version v2").
- "New stock version" entries are written by the host at start, for each skill whose stock version differs from the one its latest entry recorded. A skill with no history gets none.
- **Reset to stock** asks first, naming what stops applying and that it stays in the history. It removes `additions.md`, `rewrite.md` and `base.md`, and writes an entry.
- **Restore** writes the entry's state back as the current state and writes a new entry; it never deletes entries. Restoring a rewrite restores its base too, so it may be out of date at once, and says so.
- The history is listed newest first on the skill's page, each entry with "View" (its text, read-only) and "Restore".

### 9. Storage

```
<data folder>/skills/
  coordinating/
    additions.md          your rules, in Additions mode
    rewrite.md            your rewrite; present only in Rewrite mode
    base.md               the stock text the rewrite is based on; present with rewrite.md
    history/
      2026-10-15T14-03-22.481Z.json
  implementing/ reviewing/ keeping-a-pr/
```

- No folder, or no file, means stock with no rules: a fresh install and an update need no migration.
- The current state is plain text, one file per thing, so the human can read it and change 5's sync can tell when two machines changed the same file.
- The history is one file per entry, named by its time, so entries from two machines never edit the same file.
- All writes go through one `SkillStore` in a new `skills.rs`, rooted at one folder and written atomically like `save_settings`. Change 5 points the root at the settings repository's `skills/` folder; nothing else changes.
- The host reads the files when it builds each turn's instructions, so a saved change applies from each agent's next turn. A file it cannot read counts as missing, and the page says the skill's file could not be read, so a broken file degrades to stock instead of blocking turns.

### 10. Changing a skill

- New commands, sent only by the human's client: `SaveSkillRules`, `StartSkillRewrite`, `SaveSkillRewrite`, `MarkSkillUpToDate`, `ResetSkill` and `RestoreSkill`. No agent tool changes a skill.
- Each refuses a text over 32,000 characters (decided question 8), and nothing else: an empty rewrite is accepted (H9).
- The snapshot carries each skill's state: name, what it is for, stock text and version, mode, the human's text, the base's version, whether it is out of date, and whether its files could be read. The history and the diff are asked for when a skill's page opens, so the snapshot stays small.

### 11. Opening a pull request

The core's sentence stays: write the pull request's title and description "by the repository's and the human's own PR rules" (H3). The Implementing page's hint suggests putting PR rules in the human's own rules. The `ticket-publishing` requirement that the instructions name no PR skill now covers the bundled instructions only, so the human may name their own.

### 12. What the tests check

- The core's statements, today's assertions that name tools, formats, rules and messages, are checked on the instructions built with **every skill rewritten to an empty text**, so no edit can remove them.
- Each stock skill's own statements (the stop rule, the objection rule, the blocking definition, who runs the full suite) are checked with every skill at stock.
- With every skill at stock, each role's instructions contain every sentence they contain today, apart from the reworded ones slice 1 lists. They also contain the core's new sentence and, for testers and reviewers, the other role's paragraph.

## Risks / Trade-offs

- **A rule the human removes is gone.** Removing "ask the human about a product, security or data-loss waiver" from Coordinating is allowed (H9). The history shows it and can restore it.
- **The human's rules may contradict the core.** Wiffletree does not refuse them. The core says its tools and rules still apply, and the app refuses what it always refuses.
- **The skills folder is writable by agents' shells.** As with `settings.json`, an agent running with permission prompts bypassed can change it. Stated in the README (decided question 10).
- **Longer instructions cost context on every turn.** The 32,000-character limit bounds it.
- **A stock change under a rewrite goes unmerged until change 6.** The out-of-date mark and the diff make it visible.

## Mockup

See `design/mockups/current/skills.html`, in Tidal Light and Tidal Dark.
