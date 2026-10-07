# Security policy

Please report vulnerabilities privately through [GitHub's private vulnerability reporting](https://github.com/Vyttle-LLC/wiffletree/security/advisories/new) rather than in a public issue. Include the steps to reproduce, the affected version (shown in Finder's Get Info for `Wiffletree.app`) and the impact you expect.

We aim to acknowledge reports within a week. Fixes ship as a normal release, which installed copies download automatically.

## Scope

Wiffletree deliberately runs Claude and Codex with their permission prompts bypassed, inside the repositories you attach. An agent changing files or running commands in an attached repository is expected behavior, not a vulnerability. Reports we especially want include:

- Ways for a provider turn to act outside its intended project or session identity, such as forging another agent's messages over the host socket.
- Ways to reach the local host from another user account or over the network.
- Problems with update verification: Wiffletree should install only a release whose digest matches GitHub's and whose code signature matches the installed app's team.
