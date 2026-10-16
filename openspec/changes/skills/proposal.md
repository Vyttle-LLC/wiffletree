## Why

Every agent Wiffletree starts gets instructions bundled in the app: a communication contract and one role file (`crates/workspace-host/skills/*.md`). The human cannot read them in the app or change them, and each file mixes two kinds of sentence. Some describe the app: tool names, the report and findings formats, what the workspace refuses. Others are a way of working: when to create tickets, how small a fix should be, what counts as a blocking finding. Changing the first kind would break the app. Changing the second is a matter of taste, and today only a new release can do it.

The human decided on October 12, 2026 that "a skill is a named way of working Wiffletree runs, with a stock version the user can read and alter", that there are four, that the communication rules are a fixed core and not a skill, and that editing has two modes, Additions and Rewrite, each with Reset to stock and a history. This is change 4 of 6 in the plan `plan-pr-keeper-and-skills.md`. It builds on review levels (change 2) and the PR keeper (change 3), and prepares for the settings repository (change 5) and for merging stock updates into a rewrite (change 6).

## What Changes

- **A fixed core and four skills.** Today's seven files are split by one rule: a sentence the app depends on goes into the **fixed core**, and a sentence about a way of working goes into a **skill**. The four skills are **Coordinating** (when to create tickets and start agents, which model and effort, when to ask the human), **Implementing**, **Reviewing** and **Keeping a PR**. design.md places every paragraph of today's files, and every sentence of the coordinator's, in one or the other. The split changes no behavior: with every skill at stock, each role is told what it is told today.
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

## Open questions

Each has a recommendation; "approve with the recommended answers" settles all of them.

1. **Where does the Skills page sit?** Recommended: (a) its own page in the sidebar's Workspace section, after Models. Options: (b) a card on the Models page; (c) a tab in the app's preferences window. *Why (a):* skills are as central as models and repositories, and the page needs room for long text.
2. **Where does the model guide live?** Today the coordinator reads which model and effort to pick from "the guide" on the Models page. Recommended: (a) leave it in Models; Coordinating's stock text says to follow it, and its page links to it. Options: (b) move the guide into the Coordinating skill. *Why (a):* the guide is written for the models allowed on this machine, and the plan already lists it as its own item for change 5.
3. **Where do the review levels' "Use it" lines live?** The plan says the Reviewing text says when to use each level; change 2 built that as each level's "Use it" line on the Models page. Recommended: (a) leave them there, and show them read-only on the Reviewing page with a link. Options: (b) move them into the Reviewing skill text. *Why (a):* each line belongs to the level it describes, and removing a level should remove its line.
4. **What happens to the human's own rules when they start a rewrite?** Recommended: (a) they are copied to the end of the rewrite, under their heading, so nothing is lost and one text is edited. Options: (b) keep them separate and still add them after the rewrite. *Why (a):* two texts on top of each other, both the human's, is one more thing to reason about.
5. **What runs while a rewrite is out of date?** Recommended: (a) the rewrite, as written, with the skill marked out of date on the Skills page and the sidebar's Skills row. Options: (b) the new stock text, until the human marks the rewrite up to date; (c) hold that role's agents until the human decides. *Why (a):* the human chose to rewrite, the core still reaches every agent, and (c) would stop work over a text change.
6. **Is the human told when a stock skill changes?** Recommended: (a) a history entry for every skill, and for an out-of-date rewrite also its mark on the Skills page and a dot on the sidebar's Skills row. No Inbox item. Options: (b) an Inbox item for every stock change; (c) an Inbox item only for an out-of-date rewrite. *Why (a):* Additions never clash with a new stock version, so only a rewrite needs the human, and it keeps running meanwhile.
7. **One Reviewing skill for testers and reviewers?** Recommended: (a) one text, given to both, with a paragraph for each role as today. Options: (b) two skills, "Testing" and "Reviewing". *Why (a):* the plan names four skills, and most of the text applies to both.
8. **Size limit.** Recommended: 32,000 characters for the human's rules and for a rewrite, each. Options: a different limit, or none. *Why:* the text goes on every turn's command line and into every turn's context.
9. **How much history?** Recommended: (a) every entry, kept until the human resets the history, which this change does not offer. Options: (b) the last 50 entries per skill. *Why (a):* entries are small text files, and change 5 will hold them in Git anyway.
10. **Edits made outside the app.** The skills folder is a plain folder, and an agent's shell can write it as it can write `settings.json`. Recommended: (a) state it as a limit and detect nothing. Options: (b) mark a skill "changed outside Wiffletree" when its files differ from its latest history entry. *Why (a):* change 5's sync will write the same files from another machine, so detection belongs with it.
11. **Order against the PR keeper stack.** Slice 1 splits `keeper.md` as PR keeper slices 4b and 5 (#77–#79) leave it. Recommended: (a) start slice 1 after #79 merges. Options: (b) start now from main and split the keeper's try text when #77–#79 land. *Why (a):* one split of one text.
