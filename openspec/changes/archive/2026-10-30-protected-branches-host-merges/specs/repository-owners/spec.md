## MODIFIED Requirements

### Requirement: The Repositories page lists owners to allow
The Repositories page SHALL show, above the repository list, a block titled "Open pull requests without asking". It SHALL list each owner with a tick box, whether it is an organization or a user, how many workspace repositories it owns and whether they are private or public. It SHALL list each fork separately, with its own tick box, its parent and its visibility. It SHALL list repositories that are not on GitHub without a tick box. It SHALL state in one line that Wiffletree never approves, closes or comments on a pull request and never merges into a protected branch, and SHALL say that the list stays on this machine. The page SHALL call a ticked owner "allowed" and SHALL NOT call it approved or the behavior automatic. Each repository row SHALL say which rule applies to it: without asking, asks first, or not on GitHub.

#### Scenario: Owners with counts
- **WHEN** the workspace has 29 Sage-Social repositories, all private, and one public Vyttle-LLC repository
- **THEN** the block shows "Sage-Social", an organization with 29 private repositories, and "Vyttle-LLC" with 1 public repository, each with a tick box

#### Scenario: A fork is listed on its own
- **WHEN** the workspace has `MHohlios/gpui-component`, a fork of `longbridge/gpui-component`
- **THEN** it has its own row and tick box naming its parent, and it is not counted under MHohlios

#### Scenario: A repository that is not on GitHub
- **WHEN** a repository has no GitHub remote
- **THEN** it is listed without a tick box and its row says it is not on GitHub

#### Scenario: Repository row
- **WHEN** `Vyttle-LLC` is allowed and `celadon-theme` is not
- **THEN** the wiffletree row says pull requests are opened without asking and the celadon-website row says it asks first

#### Scenario: The line about what Wiffletree never does
- **WHEN** the human opens the Repositories page
- **THEN** the block says that Wiffletree never approves, closes or comments on a pull request and never merges into a protected branch
