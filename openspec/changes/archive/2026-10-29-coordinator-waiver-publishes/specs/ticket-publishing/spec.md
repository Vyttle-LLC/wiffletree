## ADDED Requirements

### Requirement: The coordinator's waiver publishes without asking
`publish_ticket`'s `waived` reason SHALL be accepted only from the project coordinator that owns the ticket. After a blocked cycle it SHALL be accepted only under the conditions `accept_ticket` uses for a waiver: no cycle is running, every verifier of the latest cycle checked, none having reported that it could not verify or never reported, and no ledger entry is untriaged; the host SHALL make this check with the same code for both tools. Once the ticket has used every full review the cycle cap allows, the waiver MAY also cover the commits past the verified one that it names by short SHA (see ticket-workspaces); below the cap, or for commits it does not name, nothing changes. After a passed cycle a waiver that covers no such commit SHALL be refused. The request SHALL record the reason and any commits it covers.

In an allowed owner's repository, a request carrying the coordinator's waiver SHALL run its steps without asking the human, except opening another pull request after an earlier one was closed or merged. Where the owner is not allowed, each step SHALL ask as any request does, and its item SHALL show the waiver's reason and any unreviewed commits. The tool's result SHALL say that the publish runs under the coordinator's waiver.

When such a request is made, the host SHALL record the waiver on the ticket with the commits it covers. A later publish of the same review, by the coordinator or the implementer, SHALL stand on the recorded waiver without passing `waived` again, and so SHALL the PR keeper's push. Starting a new verification cycle SHALL clear it.

#### Scenario: Allowed owner, waived blocked review
- **WHEN** the ticket's latest cycle ended blocked with F4 open, every verifier checked, nothing is untriaged, and its coordinator calls `publish_ticket` with `waived` "F4 needs a fetched base the host always has" in an allowed owner's repository
- **THEN** the host pushes the branch and opens the pull request, no Inbox item is raised, and the result says it is publishing under the coordinator's waiver

#### Scenario: Allowed owner, waived commits at the cap
- **WHEN** at the cap the coordinator calls `publish_ticket` with `waived` naming the one commit after the reviewed one, in an allowed owner's repository
- **THEN** the host publishes without asking, and the request and the ticket record that commit

#### Scenario: The implementer passes waived
- **WHEN** the ticket's implementer calls `publish_ticket` with `waived`
- **THEN** it is refused and nothing is sent

#### Scenario: A finding is untriaged
- **WHEN** the coordinator passes `waived` after a blocked cycle whose ledger has untriaged F6
- **THEN** it is refused, naming F6, and nothing is sent

#### Scenario: A verifier could not verify
- **WHEN** the coordinator passes `waived` after a blocked cycle in which a verifier reported that it could not verify
- **THEN** it is refused, saying a waiver covers only checked findings, and nothing is sent

#### Scenario: Owner not allowed
- **WHEN** the coordinator publishes a waived blocked review in a repository whose owner is not allowed
- **THEN** the push and the pull request each wait for their own approval, and each item shows the waiver's reason

#### Scenario: A later publish of the same review
- **WHEN** after a waived publish the implementer calls `publish_ticket` without `waived` to change the open pull request's description
- **THEN** the host publishes without asking

#### Scenario: A new cycle
- **WHEN** a new verification cycle starts on the ticket
- **THEN** the recorded waiver is cleared, and a publish after that cycle is decided on it alone

### Requirement: A blocked review without a waiver needs the human's approval
When the ticket's latest verification cycle ended blocked and the request carries no waiver of the coordinator's, the host SHALL NOT push or open a pull request without the human's approval in the Inbox, in every repository, including an allowed owner's. The item SHALL say that the review ended blocked, SHALL name any verifier that could not check, and SHALL list every open finding, which is every finding in the ticket's ledger that is neither fixed nor settled as follow-up or won't-fix, with its id, severity, verifier, location and summary. In an allowed owner's repository there SHALL be one such approval for the commit, naming everything that would be sent, and approving it SHALL run the commit's steps without a further item. Elsewhere the notice and the findings SHALL appear on each step's approval. The approval SHALL cover that commit only.

#### Scenario: Blocked review in an allowed owner's repository
- **WHEN** `publish_ticket` is called without `waived` for a ticket in an allowed owner's repository whose latest cycle ended blocked with open findings F4 and F6
- **THEN** nothing is sent, and the Inbox shows one approval saying the review ended blocked and listing F4 and F6 with their severity, location and summary

#### Scenario: Approved despite the blocked review
- **WHEN** the human approves that item for a ticket with no pull request
- **THEN** the host pushes the branch and opens the pull request without a second item

#### Scenario: Blocked review where the owner is not allowed
- **WHEN** `publish_ticket` is called for such a ticket in a repository whose owner is not allowed
- **THEN** the push approval and the pull request approval each say the review ended blocked and list the open findings

#### Scenario: A verifier could not check
- **WHEN** the blocked cycle includes a verifier that reported it could not verify
- **THEN** the item names that verifier as not having checked

#### Scenario: The next commit
- **WHEN** a later commit's cycle also ends blocked and `publish_ticket` is called without `waived`
- **THEN** the human is asked again for that commit

## MODIFIED Requirements

### Requirement: Only the ticket's coordinator or implementer may publish
`publish_ticket` SHALL be accepted only from the project coordinator that owns the ticket or from an implementer assigned to it. A tester, a reviewer, or an agent of another ticket SHALL be refused. Only the coordinator SHALL pass `waived` (see "The coordinator's waiver publishes without asking").

#### Scenario: Implementer publishes
- **WHEN** the ticket's implementer calls `publish_ticket` for its ticket
- **THEN** the call is accepted, subject to the other checks

#### Scenario: Verifier refused
- **WHEN** a reviewer on the ticket calls `publish_ticket`
- **THEN** it is refused and nothing is sent

### Requirement: Instructions state the publishing rules in Wiffletree's own words
The bundled instructions for every role, the fixed core with every skill at stock (see skills), SHALL NOT name an open-pr skill or any other skill for pull requests. The human's own rules and rewrites MAY name their own PR rules or skill, and the host SHALL NOT filter them. The fixed core SHALL tell agents never to push, or to create, edit, comment on, review, close or merge a pull request, from their shell; to publish with `publish_ticket` once the ticket's review has ended; that the workspace runs the push without asking when the human allowed the repository's owner and the review passed or the coordinator waived a blocked review, and otherwise only after the human approves each step; to write the pull request's title and description by the repository's and the human's own PR rules; and that only the human merges. The bundled instructions SHALL NOT say that a coordinator may merge into a plan's feature branch. The coordinator's core SHALL say when to publish, that after a blocked cycle it publishes with `waived` under the same rules as accepting with a waiver, and that publishing without `waived` hands the decision to the human. The implementer's core SHALL tell it to write the title and description when asked to publish.

#### Scenario: No pull request skill
- **WHEN** the host builds the instructions for any role with every skill at stock
- **THEN** they do not contain "open-pr" and they name `publish_ticket`

#### Scenario: The human's own PR rules
- **WHEN** the human's rules for Implementing say "Follow my open-pr skill"
- **THEN** implementers get that rule as written, and the core's publishing rules are unchanged

#### Scenario: Merging
- **WHEN** the host builds the instructions for the project coordinator
- **THEN** the core says only the human merges and gives the coordinator no case in which it may merge

### Requirement: The host may publish for the PR keeper
The host SHALL be able to start a publish of a ticket itself, for the PR keeper, without a `publish_ticket` call. Such a publish SHALL only push the ticket's branch to its open pull request; it SHALL NOT open a pull request and SHALL take no title or description. It SHALL be subject to every check, at every moment, that a publish requested with `publish_ticket` is: the ticket is open, reviewed and has no cycle running; the worktree is on the ticket's branch at the reviewed commit or a patch-equivalent one; `origin` is one checked place on GitHub that the base tracks; the frozen request is compared again before anything is sent; the owner is confirmed with GitHub; exactly one ref is sent to the frozen URL; and GitHub's branch is replaced only when it is the commit the host last pushed or was once this branch's tip. No rule SHALL be relaxed for the keeper. It SHALL carry the waiver recorded on the ticket, as a later `publish_ticket` call does.

A keeper publish SHALL be started only for a ticket whose publish would run without asking. It SHALL never raise an Inbox item: when a step would need the human's approval, as when the owner was unticked or a later review ended blocked, the request SHALL end with nothing sent. Its result SHALL go to the keeper's record of the ticket and SHALL NOT be sent to any agent as a publish result. While its step runs, `publish_ticket` for the ticket SHALL be refused as for any running step. A keeper publish that was running when the host stopped SHALL be ended at the next start with nothing more sent, and the next pass SHALL decide anew.

#### Scenario: A rebased branch is pushed for the keeper
- **WHEN** the keeper has moved a patch-equivalent rebase onto a ticket's branch in an allowed owner's repository whose latest cycle passed, and GitHub's branch is at the commit the host last pushed
- **THEN** the host replaces GitHub's branch with the rebased commit under a lease on that commit, raises no Inbox item, and sends no agent a publish result

#### Scenario: A rebase of a waived blocked review
- **WHEN** the keeper has moved a patch-equivalent rebase onto a ticket's branch in an allowed owner's repository whose latest cycle ended blocked and whose coordinator published it with a waiver
- **THEN** the host pushes the rebased commit and raises no Inbox item

#### Scenario: GitHub's branch has someone else's commit
- **WHEN** GitHub's branch is at a commit the host did not push and this branch never had
- **THEN** the host refuses, pushes nothing, and records the refusal for the keeper

#### Scenario: A commit the review did not check
- **WHEN** a keeper publish is started while the worktree's HEAD has changes the ticket's latest cycle did not check
- **THEN** it is refused and nothing is sent

#### Scenario: The owner was unticked
- **WHEN** the human unticks the repository's owner after the keeper's rebase and before its push
- **THEN** nothing is pushed and no Inbox item is raised

#### Scenario: A blocked review
- **WHEN** the ticket's latest cycle ended blocked and the ticket records no waiver
- **THEN** no keeper publish starts

#### Scenario: Transferred repository
- **WHEN** GitHub reports the repository under another owner when the keeper's push is about to run
- **THEN** nothing is pushed, as for any publish

#### Scenario: Only the ticket's branch
- **WHEN** Git is configured to push tags and all branches
- **THEN** the keeper's push creates or updates exactly one ref, the ticket's branch

#### Scenario: An agent publishes meanwhile
- **WHEN** the ticket's implementer calls `publish_ticket` while the keeper's push is running
- **THEN** the call is refused, saying a publish step for this ticket is running

#### Scenario: Interrupted by a restart
- **WHEN** the host stops while a keeper push is running and starts again
- **THEN** the request is ended, no agent is messaged, and a later pass pushes again only if the checks still pass

## REMOVED Requirements

### Requirement: A blocked review always needs the human's approval
**Reason**: The coordinator's waiver of a blocked review now publishes without asking in an allowed owner's repository.
**Migration**: See "A blocked review without a waiver needs the human's approval" and "The coordinator's waiver publishes without asking".

### Requirement: A waiver of unreviewed commits always needs the human's approval
**Reason**: A waiver of unreviewed commits at the cap is the coordinator's call, as a waiver of open findings is.
**Migration**: See "The coordinator's waiver publishes without asking".
