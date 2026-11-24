## MODIFIED Requirements

### Requirement: The coordinator's issue tools
The project coordinator SHALL have `list_issues` over its project's Space's tracker, with filters for state, assignee, label, repository and text; `read_issue`, returning the issue with its comments and events; and `link_issue`, taking an issue ID and optionally one of its own tickets, which links the issue to the coordinator's project or to that ticket on this Mac. `link_issue` SHALL append a `work` event and set the derived state, and SHALL NOT change any other part of the issue. `read_issue` and `link_issue` SHALL refuse an issue of another tracker. No other role but issue threads and the Assistant SHALL have issue tools (see `assistant`, "Issue threads" and "The Assistant writes issues directly"). No agent tool SHALL create, edit, comment on or move an issue, except the Assistant's `file_issue`, `edit_issue` and `comment_issue`, which write to the tracker of the Space they name, as the human with `assistant_for` set to the human. Coordinators and issue threads SHALL keep only the tools named here and in "Issue threads".

#### Scenario: A coordinator links a ticket
- **WHEN** a coordinator whose project covers CEL-142 and CEL-139 calls `link_issue` for CEL-139 with its ticket "Settings header"
- **THEN** CEL-139 follows only that ticket's PR, and CEL-142 is unchanged

#### Scenario: An implementer asks for issues
- **WHEN** an implementer calls `read_issue`
- **THEN** the host refuses it as a tool its role does not have

#### Scenario: The Assistant comments
- **WHEN** the Assistant calls `comment_issue` on CEL-142 in Celadon
- **THEN** the comment is written to Celadon's tracker at once, authored by the human and marked as written by their assistant

#### Scenario: A coordinator tries to comment
- **WHEN** a coordinator, or an issue thread, tries to comment on CEL-142 without the human applying a proposal
- **THEN** no tool lets it, and the issue is unchanged
