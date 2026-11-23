## Context

`provider::configure` builds every provider launch. An evaluator returns before any credential is set, and a sorting run is built with every tool off. Every other launch, project agents, compactions and assistant threads, inherits the host's environment, whose `PATH` `login_env` read from the human's login shell. The host's own Git and GitHub calls go through `publish::Tools`, `runtime::git_output` and `github_access`, which run `git` and `gh` from the host's `PATH`, never an agent's.

`turn.helper` is the host binary, already started by providers as `agent-mcp` and as the job hook `job-bash-check`.

Publishing already treats GitHub's branch as replaceable when its head is a commit the ticket's branch once had here (`was_branch_tip`, from the branch's reflog). The PR keeper changes a ticket's branch only when the worktree's HEAD, the PR's head and the verified commit agree (`may_change_branch`), and it never pushes to undo anything.

## Goals / Non-Goals

**Goals:** refuse agents' GitHub writes and pushes other than an implementer's own branch, for both providers; let implementers push their branch; let coordinators create a branch; say exactly that in the instructions and on the Skills page.

**Non-Goals:** a sandbox. Removing the human's credentials from agents' shells, so that a full path to the real binary, `curl` with the token or a hook stops working too, is a follow-up. Assistant threads, job runs and evaluators keep their own restrictions.

## Decisions

### One binary, two names, first on PATH

At start the host writes `<data folder>/guard/` holding `git` and `gh` as symlinks to its own binary, replacing stale ones. `main.rs` checks the name it was started as before anything else: as `git` or `gh` it runs the guard and never the host. A symlink needs no quoting or interpreter, and the guard shares the host's types and tests.

`configure` prepends that folder to the launch's `PATH` for every launch after the evaluator's return, so project agents, compactions and threads all get it. Codex runs commands in a login shell, which rebuilds `PATH` from the human's profile and moves the guard folder behind others, so it also gets `-c allow_login_shell=false`, on `exec` and on the app-server; the inherited `PATH` is already the login shell's, which the host read at start.

Claude needs nothing more. The plan was to set `CLAUDE_ENV_FILE` as well, in case Claude's shell snapshot reordered `PATH`; the live check showed it does not, and Claude's own hooks write to that file, so sharing one file across sessions was left out.

**Live check, November 23, 2026.** With Claude Code 2.1.322 and codex-cli 0.160.0 on this Mac, each provider ran a turn in a scratch repository with the environment `agent_env` builds: the guard folder first on `PATH`, the real `git` and `gh`, and `WIFFLETREE_PUSH_BRANCH=wiffletree/fix-x`; Codex with `allow_login_shell=false`. In both, `command -v git` and `command -v gh` resolved into the guard folder; `git push origin main`, `git push --tags`, `git push -f origin HEAD:wiffletree/fix-x`, `gh pr close 1`, `gh pr merge 1` and `gh api -X POST …` were refused with their one-line messages; `git push origin HEAD:wiffletree/fix-x` and, after an amend, `git push --force-with-lease origin HEAD:wiffletree/fix-x` reached the scratch origin. Without `allow_login_shell=false`, Codex's shell put the human's profile folders before the guard folder. Without `CLAUDE_ENV_FILE`, Claude's Bash tool kept the guard folder first.

### What the guard is told, through the environment

At turn start the host sets three variables. Names avoid `KEY`, `SECRET` and `TOKEN`, which Codex strips from its shells by default.

| Variable | Value |
|---|---|
| `WIFFLETREE_REAL_GIT`, `WIFFLETREE_REAL_GH` | The host's own `git` and `gh`, resolved once on its `PATH`. Absent when not installed; the guard then says so and exits 127. |
| `WIFFLETREE_PUSH_BRANCH` | Set only for an implementer of a ticket, to the ticket's branch, when the branch is not protected (`ProtectedBranches::of(repository).protects`) and the repository's owner is one the human allowed to publish without asking, and only while the ticket has no open pull request: GitHub's pull request follows its branch, so a push then would add unreviewed commits to it, notifying reviewers and running CI. |

The guard never reads branch rules itself: the host decides whether any push is allowed and names the one branch, so pattern matching, rulesets and owner rules stay in one place. An absent variable means no push. Coordinators, verifiers, keeper sessions and threads never get it. The guard is a guardrail, so a variable the agent's shell could change is enough; the host's own checks behind `publish_ticket` and `merge_ticket` are what hold.

The real binaries are passed rather than searched for, so a shim elsewhere on `PATH` that calls `git` again cannot loop back into the guard, and agents run the same `git` and `gh` as the host.

### Parsing `git`

The guard skips global options to find the subcommand: `-C <dir>`, `-c <k=v>`, `--git-dir`, `--work-tree`, `--namespace`, `--config-env`, `--exec-path`, `--super-prefix` in both spellings, and flags such as `-P` and `--no-pager`. A subcommand that is not a Git command is resolved as an alias with the real `git config --get alias.<name>` under the same global options, repeatedly, so an alias of `push` is judged as a push. A `!` shell alias is refused: Git runs its shell with its own exec folder first on `PATH`, so the `git` inside it is the real one and the guard could not judge it.

Refused subcommands: `push` (unless allowed below), `send-pack` and `http-push`, which push without `push`. Everything else is executed as given.

A push is allowed only when all hold:

- `WIFFLETREE_PUSH_BRANCH` is set.
- The remote is literally `origin`, and there is at least one refspec. A bare `git push` or `git push origin` is refused, because `push.default` and `remote.origin.push` decide what it sends.
- Every refspec's destination is the ticket's branch: `<src>:<branch>`, `<src>:refs/heads/<branch>`, or `<branch>` alone. No `+`, no empty source (a delete), no `refs/tags/`.
- Every option is on an allow-list: `--force-with-lease[=…]`, `--force-if-includes`, `-u`/`--set-upstream`, `--no-verify`, `-q`, `-v`, `-n`/`--dry-run`, `--porcelain`, `--progress`, `--no-follow-tags`, `--atomic`. So `-f`, `--force`, `--tags`, `--follow-tags`, `--all`, `--mirror`, `-d`/`--delete`, `--prune`, `--repo` and `--recurse-submodules` are refused.

An allowed push runs with `--no-follow-tags --recurse-submodules=no` added after `push`, so configuration cannot send tags or submodules along, and with every destination and lease rewritten to `refs/heads/<branch>`: an unqualified `wiffletree/fix-x` would otherwise resolve to a tag of that name on origin. A lease that names another ref is refused.

### Parsing `gh`

`-R`/`--repo` in any position, and `--hostname`, are removed with their values. The first two remaining words decide, against an allow-list; anything not on it is refused, which covers writes, aliases and extensions:

| Allowed | |
|---|---|
| `pr` | `view`, `list`, `diff`, `checks`, `status` |
| `issue` | `view`, `list`, `status` |
| `run` | `view` (including `--log`), `list`, `watch` |
| `repo`, `release`, `workflow`, `label`, `ruleset` | `view`, `list` where they exist |
| `search` | any |
| `auth` | `status` |
| alone | `--version`, `--help`, `help` |
| `api` | only as a GET, below |

`auth git-credential` is allowed too: Git's credential helper, as `gh auth setup-git` configures it, runs it to read the sign-in.

`gh api` must come first, and its arguments are read as gh's flag parser (pflag) reads them: a valued option takes the next argument whatever it looks like (`--template -XGET` is a template), short switches cluster with the last letter taking the rest of the word (`-iXPOST` is `-i -X POST`), and an option `api` does not have is refused rather than guessed at. Its method is the last `-X`/`--method` so read. Without one it is POST when `-f`, `-F`, `--field`, `--raw-field` or `--input` is present, and GET otherwise. Only GET passes, and `gh api graphql` is refused, since it is always a POST and may hold a mutation.

### The refusal

One line on stderr and exit status 1: what was refused and the tool to use, for example "Wiffletree refused `gh pr merge`: only merge_ticket merges a pull request." or "Wiffletree refused this push: you may push only `origin HEAD:<branch>`, optionally with --force-with-lease; publish_ticket opens the pull request."

### create_branch

A coordinator tool taking `repository_id`, `branch` and an optional `start`, a branch on origin, defaulting to the branch the repository's base tracks. It follows `create_ticket`'s shape: checks on the host thread, Git off it, reply when done.

On the host thread it refuses a repository the project does not use, an owner the human has not allowed, a name `git check-ref-format --branch` rejects or that starts with `refs/`, and a protected name. Off it, in the repository's clone, it reads `origin` with `read_origin` and `ensure_not_rewritten` as publishing does, lists both refs with `ls-remote`, refuses when `branch` exists or `start` does not, fetches `start`, and pushes `<start commit>:refs/heads/<branch>` to the push URL with `--force-with-lease=refs/heads/<branch>:` (which fails if anything created it meanwhile), `--no-follow-tags` and `--recurse-submodules=no`. The reply is `{repository_id, branch, start, commit}`; `branch` goes straight into `create_ticket` as `base`.

The owner rule matches the implementer's push: both are writes to GitHub the human has allowed for that owner. Asking in the Inbox for other owners is left out until someone needs it.

### Publishing and the keeper after an agent's push

Publishing needs no change in behavior. GitHub's branch at the commit being published is `Current` (no push, the pull request opens or updates), and at an ancestor it is `Behind` (an ordinary push). A head the implementer pushed and later rewrote is `Replaceable`, because it was once the branch's tip here. Tests cover all three with a push from the guard.

The keeper changes one thing. When a PR's head is not in the worktree's HEAD, `fetch_due` fetches it and, if the two diverged, tells the coordinator. After an implementer pushes a WIP commit and then amends it locally, that notice would be wrong: the head is the branch's own earlier tip. `fetch_due` skips a head the ticket's branch reflog holds, as `was_branch_tip` does for publishing. The next publish replaces it with a lease.

Once a ticket's pull request is open, the host gives no push branch, so the guard refuses the implementer's push and only `publish_ticket` updates the pull request, after a review. A turn that started before the pull request opened keeps the branch it was given until it ends.

## Risks / Trade-offs

- **A guardrail, not a sandbox.** An agent can run `/usr/bin/git`, change the variables or use `curl` with the token. And any `git` that Git itself starts runs the real binary, since Git puts its own exec folder first on `PATH`: hooks, `rebase -x`, `submodule foreach` and `bisect run` can push unguarded. The page, the README and the contract say so; the follow-up removes the credentials.
- **Shell environments differ per provider version.** Mitigated by the live check during implementation and a test that the launch carries the settings.
- **`allow_login_shell=false` changes Codex's shells.** They no longer read the human's login files, but they inherit the same login `PATH` the host read at start.
- **Allow-lists refuse some harmless commands,** such as `gh pr checkout` or `gh api graphql` for a read. Agents read the same data with the allowed commands or `gh api` GETs; adding to the list later is cheap.
