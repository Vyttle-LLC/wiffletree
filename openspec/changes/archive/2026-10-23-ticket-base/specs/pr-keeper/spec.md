## ADDED Requirements

### Requirement: A PR is rebased onto its own base
A kept PR SHALL be rebased onto its base branch on GitHub, which for a ticket created with `base` is that branch, and whether the rebase is the same change SHALL be judged against the ticket's base as publishing judges it.

#### Scenario: A ticket on another base
- **WHEN** a kept PR opens into `release` for a ticket created on `release`, and `release` moves on
- **THEN** the rebase is onto `origin/release`, is the same change, and is pushed

### Requirement: A deleted base is reported, not rebased onto
When a ticket's base branch is no longer on its remote, the host SHALL rebase nothing onto it and SHALL move and push nothing, and SHALL tell the ticket's coordinator once, in a message from Wiffletree naming the ticket and the base. The open-branch check, which fetches every open ticket's base, SHALL find it whether or not a rebase is due and whether or not the ticket has a pull request, and SHALL name the open tickets on that base even when a pruned remote-tracking ref leaves Git nothing to compare. A keeper rebase whose fetch finds the base gone SHALL send the same message, so the coordinator hears once whichever finds it first. A fetch that fails for another reason, such as an unreachable remote, SHALL NOT count as a deleted base. A coordinator that is archived SHALL be told nothing.

#### Scenario: The base is deleted before a rebase
- **WHEN** a kept PR's base `release` is deleted from GitHub while a rebase onto it is due
- **THEN** nothing is rebased, moved or pushed, and the coordinator is told once that `origin/release` is no longer on its remote

#### Scenario: The tracking ref was pruned
- **WHEN** a base is deleted on its remote and `git fetch --prune` removed its remote-tracking ref
- **THEN** the open-branch check still names the ticket on it, and the coordinator is told once across later checks

#### Scenario: An unreachable remote
- **WHEN** the remote cannot be reached
- **THEN** the fetch fails without counting as a deleted base

#### Scenario: A long branch name
- **WHEN** the deleted base's name is 255 characters long
- **THEN** the coordinator is still told

#### Scenario: An archived coordinator
- **WHEN** the base of a ticket whose coordinator is archived is deleted
- **THEN** no message is sent and nothing fails
