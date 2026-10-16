## Why

Every agent Wiffletree starts gets instructions bundled in the app: a communication contract and one role file (`crates/workspace-host/skills/*.md`). The human cannot read them in the app or change them, and each file mixes two kinds of sentence. Some describe the app: tool names, the report and findings formats, what the workspace refuses. Others are a way of working: when to create tickets, how small a fix should be, what counts as a blocking finding. Changing the first kind would break the app. Changing the second is a matter of taste, and today only a new release can do it.

The human decided on October 12, 2026 that "a skill is a named way of working Wiffletree runs, with a stock version the user can read and alter", that there are four, that the communication rules are a fixed core and not a skill, and that editing has two modes, Additions and Rewrite, each with Reset to stock and a history. This is change 4 of 6 in the plan `plan-pr-keeper-and-skills.md`. It builds on review levels (change 2) and the PR keeper (change 3), and prepares for the settings repository (change 5) and for merging stock updates into a rewrite (change 6).

## What Changes

- **A fixed core and four skills.** Today's seven files are split by one rule: a sentence the app depends on goes into the **fixed core**, and a sentence about a way of working goes into a **skill**. The four skills are **Coordinating** (when to create tickets and start agents, which model and effort, when to ask the human), **Implementing**, **Reviewing** and **Keeping a PR**. design.md places every paragraph of today's files, and every sentence of the coordinator's, in one or the other. The split changes almost no behavior: with every skill at stock, each role is told what it is told today, except that testers and reviewers each also get the other role's paragraph.
- **Authority stays in the app.** The fixed core holds every tool name, report and findings format, and every rule about messages, publishing and the worktree. What Wiffletree's own tools allow does not depend on skill text. As today, an agent's shell is not sandboxed: it can still run `git push` whatever any text says. Where the app enforces one policy, such as how many review rounds run or that its publishing asks first, editing a skill changes nothing, and the skill's page says so.
- **Additions, the default.** The stock text stays untouched, and the human's own rules sit after it, under a heading that says they win where the two disagree. A new stock version never clashes with them.
- **Rewrite, on purpose.** The human may replace a skill's stock text with their own. Wiffletree keeps the stock text the rewrite started from. When a new release changes that stock text, the skill is marked **out of date**, its page shows what changed, and the rewrite keeps running exactly as written until the human resets it or marks it up to date. Merging stock changes into a rewrite is change 6.
- **Reset to stock and a history for every skill.** Reset asks first, then returns the skill to stock with no rules of the human's own. Every save, switch, reset and new stock version is a history entry, and any entry can be restored. Nothing the human wrote is lost by a reset.
- **A Skills page** in the sidebar's Workspace section, after Models. It lists the four skills, each with its state ("Stock", "Stock and 3 of your rules", "Rewritten · out of date"), and the fixed core, which can be read but not edited. Mockup: `design/mockups/current/skills.html`.
- **Edits apply from each agent's next turn.** Running agents pick up a saved change at their next turn. Only the human's client can change a skill; no agent tool can.
- **Wiffletree does not judge an edit.** Any text up to the size limit is accepted, including an empty rewrite, since the core reaches the agent either way.
- **Opening a pull request is not a skill.** The core keeps telling agents to write a pull request by the repository's and the human's own PR rules, which the human can put in their own rules. The bundled instructions name no PR skill, but the human's own rules may.
- **Stored to move later.** Skills live in a `skills` folder in Wiffletree's data folder, one folder per skill holding plain text files and one file per history entry, so change 5 can move that folder into the settings repository unchanged. No sync is built.
- **Not included:** merging stock updates into a rewrite (change 6), the settings repository and its sync (change 5), skills per project or per repository, new skills or skills the human creates, editing the fixed core, editing the comment-sorting instructions, moving the model guide or review levels' "Use it" lines out of Models, and recording which skill version each run used.

## Capabilities

### New Capabilities
- `skills`: the fixed core and the four skills; what each role receives and in what order; Additions and Rewrite; when an edit applies; Reset to stock; the history and restoring from it; a rewrite out of date against a new stock version; where skills are stored; the Skills page; the stated limits.

### Modified Capabilities
- `ticket-publishing`: "Instructions state the publishing rules in Wiffletree's own words" now holds for the bundled instructions, the fixed core with every skill at stock. The human's own rules may name their own PR rules or skill.

## Impact

- `workspace-host/skills/`: the seven files become `core/` (communication, coordinator, implementer, verifier, keeper) and `stock/` (coordinating, implementing, reviewing, keeping-a-pr), each with its own version.
- `workspace-host`: `provider::skill` composes core, then the skill as stored, for each role; a new `skills.rs` reads and writes the skills folder and its history; the turn and the context probe get their instructions from the host instead of from `include_str!` alone.
- `workspace-core`: skill states on the snapshot, and commands to save, rewrite, reset, restore and mark up to date.
- `workspace-desktop`: a new `skills_view.rs` and `Page::Skills` in the sidebar's Workspace section.
- Stored data: a new `skills/` folder in the data folder. No table and no migration; with no folder, every skill is at stock.
- Tests: `tests/contracts.rs` checks the core's statements with any skill text, and each stock skill's own statements.
- Docs: `README.md`, `docs/PRD.md` ("Agent communication and shared context") and `docs/DEVELOPMENT.md`.
- Built in six slices, each its own PR of about 300 production lines or less; see tasks.md.

## Decided questions

The human approved the proposal on October 16, 2026 with every recommended answer:

1. **The Skills page** is its own page in the sidebar's Workspace section, after Models.
2. **The model guide** stays in Models; Coordinating's stock text says to follow it, and its page links to it.
3. **The review levels' "Use it" lines** stay in Models, shown read-only on the Reviewing page with a link.
4. **Starting a rewrite** copies the human's own rules to its end, under their heading.
5. **An out-of-date rewrite** keeps running as written, marked on the Skills page and on the sidebar's Skills row.
6. **A stock change** writes a history entry for every skill; an out-of-date rewrite also gets its mark and the sidebar's dot. No Inbox item.
7. **One Reviewing skill** for testers and reviewers, with a paragraph for each role, so each now also gets the other role's paragraph.
8. **The size limit** is 32,000 characters for the human's rules and for a rewrite, each.
9. **All history** is kept, until the human resets it, which this change does not offer.
10. **Edits made outside the app** are stated as a limit and not detected.
11. **Slice 1 starts after #79 merges.** It did: slice 1 splits `keeper.md` as main has it.
