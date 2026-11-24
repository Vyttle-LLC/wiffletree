## ADDED Requirements

### Requirement: Agents' git and gh are guarded
Every provider launch that receives the workspace's credentials, on Claude and Codex, including compactions and assistant threads, SHALL find Wiffletree's `git` and `gh` first on its shells' `PATH`. The guard SHALL run the real binary, with the arguments unchanged except as a requirement below says, for every call it allows, and SHALL refuse every other call with one line on stderr naming what was refused and the Wiffletree tool to use instead, and exit status 1, running nothing. An evaluator's run and a sorting run SHALL be launched as before. The host's own Git and GitHub calls, for publishing, merging, the PR keeper, sync and the brain, SHALL NOT go through the guard.

#### Scenario: A Claude implementer's shell
- **WHEN** an implementer on Claude runs `command -v git` in its Bash tool
- **THEN** the path is in Wiffletree's guard folder

#### Scenario: A Codex implementer's shell
- **WHEN** an implementer on Codex runs `command -v gh` in a shell command
- **THEN** the path is in Wiffletree's guard folder

#### Scenario: The host publishes
- **WHEN** the host pushes a ticket's branch for `publish_ticket`
- **THEN** it runs its own `git`, and the guard is not involved

#### Scenario: An evaluator
- **WHEN** a UI evaluator run starts
- **THEN** its launch has no guard folder on `PATH` and no guard variables

### Requirement: The host tells the guard what an agent may push
At each turn's launch the host SHALL give the guard the real `git` and `gh` it uses itself and, only for an implementer session of a ticket, that ticket's branch, through the launch's environment. It SHALL give the branch only when the branch is not protected for the ticket's repository (see protected-branches), the human allowed the repository's owner to publish without asking, and the ticket has no open pull request, which a push would update with unreviewed commits. With no branch given, the guard SHALL refuse every push. The guard SHALL NOT decide protection or owners itself.

#### Scenario: The pull request is open
- **WHEN** a published ticket's review fails and its implementer runs `git push origin HEAD:<its branch>` to save a fix
- **THEN** it is refused, saying no agent pushes a branch whose pull request is open, and the pull request is unchanged until `publish_ticket` pushes the reviewed commit

#### Scenario: A verifier
- **WHEN** a verifier of a ticket runs `git push origin HEAD:<the ticket's branch>`
- **THEN** it is refused, saying this agent may not push

#### Scenario: A coordinator
- **WHEN** a coordinator runs `git push origin feature/x`
- **THEN** it is refused, naming `create_branch` for a new branch and `publish_ticket` for a ticket's

#### Scenario: An owner the human has not allowed
- **WHEN** an implementer's ticket is in a repository whose owner the human has not allowed
- **THEN** its push is refused, naming `publish_ticket`

#### Scenario: gh is missing
- **WHEN** `gh` is not installed and an agent runs `gh pr view 1`
- **THEN** the guard says `gh` is not installed and exits 127

### Requirement: git passes everything but a push outside the agent's branch
The guard SHALL find `git`'s subcommand after its global options, `-C`, `-c`, `--git-dir`, `--work-tree`, `--namespace`, `--config-env`, `--exec-path` and `--super-prefix` with their values in either spelling, and flags such as `-P` and `--no-pager`. A subcommand that is an alias SHALL be judged as what it expands to, with the same global options, and a `!` shell alias SHALL be refused, since the `git` its shell runs is the real one. `send-pack` and `http-push` SHALL be refused. A `push` SHALL be allowed only when the agent may push (see above), its remote is `origin`, it names at least one refspec, every refspec's destination is the agent's branch, as `<src>:<branch>`, `<src>:refs/heads/<branch>` or `<branch>`, with no `+` and a source, and every option is one of `--force-with-lease` with or without a value, `--force-if-includes`, `-u`, `--set-upstream`, `--no-verify`, `-q`, `-v`, `-n`, `--dry-run`, `--porcelain`, `--progress`, `--no-follow-tags` and `--atomic`, where a lease names the agent's branch if it names a ref. An allowed push SHALL run with `--no-follow-tags --recurse-submodules=no`, and with each destination and lease spelled `refs/heads/<branch>`, so a tag of the same name is never reached. Every other `git` call SHALL run unchanged.

#### Scenario: A tag with the branch's name
- **WHEN** origin has the tag `wiffletree/fix-x` and no such branch, and the implementer runs `git push origin HEAD:wiffletree/fix-x`, or the same with `--force-with-lease=refs/tags/wiffletree/fix-x:<sha>`
- **THEN** the first creates the branch `refs/heads/wiffletree/fix-x` and leaves the tag as it was, and the second is refused

#### Scenario: The implementer pushes its branch
- **WHEN** an implementer whose ticket branch is `wiffletree/fix-x` runs `git push origin HEAD:wiffletree/fix-x`
- **THEN** the real `git` pushes it

#### Scenario: With a lease, from another folder
- **WHEN** it runs `git -C ../fix-x push --force-with-lease origin HEAD:refs/heads/wiffletree/fix-x`
- **THEN** the real `git` pushes it

#### Scenario: The default branch
- **WHEN** it runs `git push origin main` or `git -c push.default=current push origin HEAD:main`
- **THEN** it is refused, saying it may push only `origin HEAD:wiffletree/fix-x`

#### Scenario: Tags, deletes and everything
- **WHEN** it runs `git push --tags`, `git push origin --follow-tags HEAD:wiffletree/fix-x`, `git push origin :wiffletree/fix-x`, `git push --delete origin wiffletree/fix-x`, `git push --all origin` or `git push --mirror origin`
- **THEN** each is refused

#### Scenario: Plain force
- **WHEN** it runs `git push -f origin HEAD:wiffletree/fix-x` or `git push origin +HEAD:wiffletree/fix-x`
- **THEN** it is refused, saying to use `--force-with-lease`

#### Scenario: A bare push
- **WHEN** it runs `git push` with its branch checked out
- **THEN** it is refused, naming `git push origin HEAD:wiffletree/fix-x`

#### Scenario: Another remote
- **WHEN** it runs `git push https://github.com/o/r HEAD:wiffletree/fix-x`
- **THEN** it is refused

#### Scenario: An alias
- **WHEN** `alias.p` is `push` and it runs `git p origin main`
- **THEN** it is refused as a push to `main`

#### Scenario: A shell alias
- **WHEN** `alias.sh` is `!git push origin main` and any agent runs `git sh`
- **THEN** it is refused, saying the guard cannot check a shell alias

#### Scenario: Reads and local work
- **WHEN** any agent runs `git status`, `git -C dir log`, `git fetch origin`, `git commit` or `git rebase origin/main`
- **THEN** the real `git` runs them unchanged

### Requirement: gh passes reads only
The guard SHALL remove `-R`/`--repo` and `--hostname` with their values wherever they appear, and judge `gh` by its first two remaining words. It SHALL allow only `pr view|list|diff|checks|status`, `issue view|list|status`, `run view|list|watch`, `repo view`, `release view|list`, `workflow view|list`, `label list`, `ruleset view|list`, `search`, `auth status`, `auth git-credential`, which Git's credential helper runs to read the sign-in, `--version`, `--help` and `help`, and `api` as a GET, named first. `api`'s arguments SHALL be read as gh's flag parser reads them: an option's value after `=` or as the next argument whatever it looks like, and short switches clustered with the last letter taking the rest of the word as its value; an option `api` does not have SHALL be refused. The method SHALL be the last `-X` or `--method` so read; without one, it SHALL be POST when `-f`, `-F`, `--field`, `--raw-field` or `--input` is given and GET otherwise. Only GET SHALL pass, and `api graphql` SHALL be refused. Everything else, including aliases and extensions, SHALL be refused. A refused pull request write SHALL name `publish_ticket` for creating or updating a pull request, `merge_ticket` for merging, and asking the coordinator, or for the coordinator the human, for anything else.

#### Scenario: Pull request writes
- **WHEN** an agent runs `gh pr close 1`, `gh pr merge 1`, `gh -R o/r pr close 1`, `gh pr create`, `gh pr comment 1 -b hi`, `gh pr review 1 --approve`, `gh pr edit 1`, `gh pr ready 1` or `gh pr reopen 1`
- **THEN** each is refused, and the merge's line names `merge_ticket`

#### Scenario: API writes
- **WHEN** an agent runs `gh api -X POST repos/o/r/issues`, `gh api --method=PATCH repos/o/r`, `gh api repos/o/r/issues -f title=x` or `gh api graphql -f query=…`
- **THEN** each is refused

#### Scenario: Options that hide a method
- **WHEN** an agent runs `gh api repos/o/r --template -XGET -f title=x` or `gh api repos/o/r -iXPOST`
- **THEN** each is refused, since gh reads the first as a template and a POST and the second as `-i` and `-X POST`

#### Scenario: Git's credential helper
- **WHEN** Git runs `gh auth git-credential get` as its credential helper for an HTTPS GitHub origin
- **THEN** the real `gh` answers with the sign-in

#### Scenario: API reads
- **WHEN** an agent runs `gh api repos/o/r/pulls/1/comments` or `gh api -X GET search/issues -f q=x`
- **THEN** the real `gh` runs them

#### Scenario: Other writes
- **WHEN** an agent runs `gh issue create`, `gh release create v1`, `gh repo delete`, `gh secret set X`, `gh variable set X`, `gh workflow run ci` or `gh alias set x 'pr close'`
- **THEN** each is refused

#### Scenario: Reads
- **WHEN** an agent runs `gh pr view 1 --json title`, `gh -R o/r pr checks 1`, `gh run view 5 --log` or `gh search prs foo`
- **THEN** the real `gh` runs them unchanged
