<p align="center">
  <img src="docs/images/banner.svg" alt="Wiffletree">
</p>

<p align="center">
  <strong>A native Mac workspace where Claude and Codex agents work as a team.</strong><br>
  Give one coordinator a goal. It plans the work, staffs each repository, and reports back with tested branches.
</p>

<p align="center">
  <a href="https://github.com/Vyttle-LLC/wiffletree/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/Vyttle-LLC/wiffletree?color=2D5A5A"></a>
  <img alt="macOS 13+ on Apple Silicon" src="https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20Silicon-1A262D">
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-86C1BC"></a>
</p>

![The Tide alerts coordinator reporting an accepted ticket, under a progress strip that reads 1 of 3 tickets accepted](docs/images/coordinator.png)

<table>
  <tr>
    <td width="50%"><img src="docs/images/tickets.png" alt="The Crew panel beside the coordinator's chat, listing three tickets by stage with their agents, models and progress"></td>
    <td width="50%"><img src="docs/images/models.png" alt="The Models page with the Claude and Codex models allowed on this machine, the providers for each role and the Full and Light review levels"></td>
  </tr>
  <tr>
    <td align="center"><sub>Every ticket gets its own branch, implementer and independent reviewers.</sub></td>
    <td align="center"><sub>You allow exact models; the coordinator picks one for each assignment.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/images/usage.png" alt="The Usage page with Claude and Codex 5-hour and weekly account limits above a week of daily Claude and Codex tokens"></td>
    <td width="50%"><img src="docs/images/context.png" alt="An implementer's Context card showing how full its context window is, by category, with a Compact button"></td>
  </tr>
  <tr>
    <td align="center"><sub>Account limits from the providers, above the tokens your agents used each day.</sub></td>
    <td align="center"><sub>See what fills an agent's context, and compact it when you choose.</sub></td>
  </tr>
</table>

## Install

Wiffletree runs on Apple Silicon Macs with macOS 13 or later. Install the latest release with one command:

```sh
curl -fsSL https://raw.githubusercontent.com/Vyttle-LLC/wiffletree/main/scripts/install.sh | bash
```

The script downloads the signed and notarized app from [Releases](https://github.com/Vyttle-LLC/wiffletree/releases/latest), checks it with Gatekeeper, moves it to `/Applications` and opens it. Run it again any time to reinstall. To install somewhere else, set `WIFFLETREE_INSTALL_DIR=~/Applications`. If you prefer to do it by hand, download `Wiffletree-<version>-macos-arm64.zip`, unzip it and drag `Wiffletree.app` to Applications.

Wiffletree drives the agent CLIs you already use, so install [Claude Code](https://code.claude.com), [Codex](https://github.com/openai/codex) or both, and sign in:

```sh
claude auth login
codex login
```

The first time you open Wiffletree, a welcome screen lists the CLIs it found signed in and offers a model setup suggested from them. Review it on the Models page and press **Save**; no agent starts until a setup is saved. Then press **⌘N**, add a repository, and tell the coordinator what you want built. [Your first project](docs/DEVELOPMENT.md#first-project) walks through it step by step.

**Updates are automatic.** The app checks hourly, downloads new versions in the background and shows a card in the sidebar with what changed. Click **Restart to update**, or just quit. To check right away, click the version number at the bottom of the sidebar.

> [!WARNING]
> Agents run with their provider's permission prompts bypassed so they can work unattended. Add only repositories you are comfortable letting them change. Wiffletree itself pushes a ticket's branch and opens its pull request only through its own checked operation: without asking in repositories whose GitHub owner you allow, and after your approval everywhere else. It never approves, merges, closes or comments on a pull request. If you turn on the PR keeper, Wiffletree also rebases a ticket's open pull request when its base moves and replaces the branch on GitHub, which changes every commit id on the pull request. Agents can still run any command on your Mac, including a push with your credentials, which Wiffletree does not prevent. That includes the PR keeper's model, which gets one try at a conflict when "Try to resolve conflicts" is ticked, and at failed checks when "Try to fix failed checks" is: Wiffletree checks only what the try leaves behind, which files changed and whether your checks command passes, and has that reviewed before pushing it. Your checks command runs as you wrote it.

## Why we built this

Coding agents got good enough that the bottleneck moved. The hard part is no longer getting one agent to write code. It is keeping five of them pointed at the same goal across three repositories, and still knowing what is going on.

The tools we had were terminal managers organized around repositories. They show you a lot, but the work we care about is a feature, a bug or an outcome, and those cut across repositories. Coordination meant pasting text into terminals, so nobody could say for sure whether a message had arrived, been read or been acted on. When something crashed, the plan lived only in our heads.

We wanted to hand off a goal the way you would to a strong team lead, then check in when it mattered. So we built a workspace around that idea instead of around terminals.

## How work gets done

Wiffletree has opinions about how agent work should be organized.

**Projects are outcomes, not repositories.** A project is a feature, a bug or any goal you want reached. Repositories are attached to it, and one repository can serve many projects at once.

**You talk to one coordinator.** Each project has a coordinator that owns the goal and the conversation with you. It plans, asks you the questions that matter and reports the result. You do not have to manage the agents underneath it.

**Each ticket is its own workspace.** The coordinator breaks the work into tickets, one repository each, with its own branch and worktree. It assigns an implementer, then verifies with one call that runs every tester and reviewer in the chosen review level at once on the same commit, and accepts the result.

**Tickets are small, isolated and tested by someone else.** Every ticket gets its own Git worktree and branch. An implementer builds it, and a separate tester checks it against the acceptance criteria. "The turn ended" never counts as "it works."

**You decide how thorough a review is.** A review level is a named set of testers and reviewers, with a line telling the coordinator when to use it. A full review can be a ticket's only review. A re-check, such as the suggested "Light", runs only on a ticket whose last review passed, for example after a rebase or a small fix. You also cap the rounds in one review and the full reviews per ticket.

**Blocked is not the same as needs-you.** Workers report blockers to the coordinator, which resolves most of them. Only the coordinator escalates to you, and it gathers its questions in one Inbox so you answer them once.

**Any model can play any role, and the coordinator picks it.** Claude can coordinate Codex implementers, and Codex can coordinate Claude. On the Models page you list the exact models and efforts allowed on this machine, choose which providers each role may use, and write a short guide on picking a model by the size and risk of the work. For every assignment and every reviewer, the coordinator picks one allowed model and effort and gives a one-line reason, which is shown with that agent's conversation, just above the message box. Wiffletree refuses a choice outside the list and never falls back to another model. You can still change the model of any single agent yourself.

**You can change how agents work, not what they may do.** The Skills page, after Models in the sidebar, lists four skills: Coordinating, Implementing, Reviewing and Keeping a PR. Each is a stock text bundled with Wiffletree, and you can add your own rules after it. Agents are told that your rules win where the two disagree. A saved change applies from each agent's next turn. A Codex agent gets it from its next new session, because a resumed Codex session keeps the instructions it started with. You can also rewrite the stock text itself, after a confirmation: your rewrite is sent in place of it, and new releases don't update it. When a release changes the stock text, the skill is marked out of date, with a dot on the Skills row and what changed line by line, and your rewrite keeps running as written until you mark it up to date or reset it. Reset to stock removes your rules or rewrite after asking, and a skill's History lists every change, newest first, with View and Restore, so nothing you wrote is lost. Every agent also gets a fixed core that you can read there but not edit: its workspace tools, how it reports, and the rules the app enforces whatever a skill says. Each skill's page lists what the app enforces and what the text decides. Authority is not in the text. An agent's shell is not sandboxed, so it can still run `git push` with your credentials, and it can write the skills folder in the data folder, as it can write the settings file.

**Messages are durable.** Agents talk through the app's message queue, not by typing into each other's terminals. Every message is saved before it is acknowledged, and an idle recipient wakes the moment one arrives. After a restart, each agent resumes its exact provider session, and the app never silently retries work that might have had side effects.

**Software does the routine work; models do the thinking.** Git state, status, polling and usage tracking are ordinary deterministic code. Models are woken only for decisions that need reasoning, so coordinators sleep between events instead of polling.

**You can see how far along the work is.** The app derives the progress bars from each agent's checklist, each ticket's stage and its review results. The coordinator's strip shows the project's phase, one segment per ticket and how many tickets are accepted. Its Crew panel lists every ticket with its agents, their models and their progress.

**You can see what fills an agent's context.** Each agent's Overview has a Context card that shows how full its context window is and with what, measured after every turn without a model call. Claude reports its own categories; for Codex they are estimates. **Compact…** on the same card replaces a Claude or Codex agent's conversation with a summary when you choose, and for Claude you can say what to keep.

**You can stop what agents leave running.** A dev server or watcher an agent starts keeps running between its turns. The Processes panel lists what each agent has running, or every agent's from the coordinator, with how long it has run and its CPU, and stops one or all of them. Wiffletree also stops them when an agent is stopped or archived and when the app quits.

**Usage and limits stay in view.** The sidebar shows each provider's 5-hour and weekly limits, and the Usage page puts both accounts side by side. Those percentages come only from the providers' own telemetry: the Codex app server, and the rate-limit events Claude sends during live turns, so a Claude window reads **Unavailable** when Claude reports no percentage. Wiffletree never scrapes account pages or spends tokens to check. Below the limits, it charts the tokens its agents used by day and ranks them by coordinator, with project and provider filters.

**Humans own integration.** Agents deliver tested branches. Once a ticket's review has ended, Wiffletree pushes its branch and opens its pull request itself: without asking for the GitHub owners you allow on the Repositories page, and only after you approve each step everywhere else. It never approves, merges, closes or comments on a pull request, so merging stays yours.

**The PR keeper keeps pull requests current, if you ask it to.** It is off until you tick "Rebase when the base branch moves" on the Repositories page, and it acts only in repositories that open pull requests without asking. Once a pull request's base has been quiet for a couple of minutes, Wiffletree rebases the branch itself, with no model and no review, and replaces the branch on GitHub with a lease, only when the rebased change is the same as the reviewed one. Every commit id on the pull request changes. A rebase whose patch changed, because the base moved lines the change touches, is pushed only with "Try to resolve conflicts" ticked, a re-check level chosen for it and a checks command on the repository's row: the command, such as `cargo test`, runs on the rebased commit, stopped after its limit (20 minutes unless you set 1–120), then the commit is reviewed at that level before it is pushed. If either fails, nothing is pushed, the branch is left as it was before the rebase and the coordinator is told. A rebase that conflicts gets one try by the PR keeper's model at each pull request head, with the same three settings and a model chosen for the PR keeper in Models. The model works in the keeper's own checkout, never the ticket's worktree, and its result is thrown away unless it changes only the files Git could not merge, passes the checks and passes the review; only then is it pushed. A try that is thrown away leaves the branch and the ticket's review as they were, and the coordinator is told why. The pull request's line reads "keeper resolving" while the try runs and "with the coordinator" after one was thrown away. With "Try to fix failed checks" ticked too, a pull request whose checks failed on GitHub gets one try the same way, once no rebase is due for it: Wiffletree first runs your checks command on its head, and if it passes here, no model runs and the coordinator is told the failure could not be reproduced. Otherwise the model's fix must add commits on top of the head, change only files the pull request already changes and pass the same checks and review; the line reads "keeper fixing" meanwhile. Without all of these, a changed patch, a conflict or failed checks are left to the coordinator. With "Sort new comments" ticked and a Claude model chosen for the PR keeper in Models, it also sorts new comments from people who can push to the repository into needs a change, a question or no action, with a model run that has every tool and hook turned off, and sends the coordinator links to those that need something, never their text. It never replies, resolves or reacts on GitHub. It works only while Wiffletree is running.

**It should feel instant.** Wiffletree is a native Rust app built on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), not a web view, and it stays light when idle.

## Status

Wiffletree is early software, and it changes quickly. Live Claude and Codex runs, exact session resume and mixed-provider handoffs work today. Everything runs on your Mac, and closing the app stops active turns until you resume. See [current limits](docs/DEVELOPMENT.md#current-limits) and the [product requirements](docs/PRD.md) for what comes next, including a headless host you can drive from another machine.

## Build from source

You need macOS on Apple Silicon, a recent stable Rust toolchain and Xcode with its Metal toolchain.

```sh
bash scripts/package_macos.sh
open target/Wiffletree.app
```

Use `--beta` to build a separate **Wiffletree Beta** that runs beside the installed app with its own store. To try it against a scratch store without packaging:

```sh
cargo build --locked --release -p workspace-host -p workspace-desktop
./target/release/workspace-desktop \
  --data-dir /tmp/wiffletree-preview --demo-repository "$PWD"
```

On a fresh store, save the suggested setup on the Models page and relaunch to get the demo project.

[Development notes](docs/DEVELOPMENT.md) cover model configuration, headless commands, validation and releases. [Measured host timings](docs/performance/host-baseline.md) are kept separate from the desktop and provider performance targets, which are not yet verified.

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow and the checks every pull request must pass. For anything larger than a small fix, open an issue first so we can agree on the approach.

Useful places to start reading:

- [Product requirements](docs/PRD.md): the source of truth for scope, architecture and performance budgets.
- [Design guide](design/README.md): the current interactive mockup, earlier studies and their limits.
- [Brand kit](design/brand/README.md) and [colors](design/brand.md): the Interlock mark and the Tidal palette.

To report a vulnerability, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## License

Wiffletree is released under the [MIT License](LICENSE). The bundled Instrument Sans subset used for the wordmark is under the [SIL Open Font License](design/brand/source/OFL.txt).
