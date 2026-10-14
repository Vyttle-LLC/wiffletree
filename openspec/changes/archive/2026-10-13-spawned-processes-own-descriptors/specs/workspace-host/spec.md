## ADDED Requirements

### Requirement: A started process holds only its own descriptors
A process the host starts, whether a provider turn, a compaction, a provider probe or a Git or `gh` call, SHALL hold no file descriptor of the host's other than its own standard input, output and error. This SHALL hold when other threads are starting processes at the same moment, so that neither the process nor anything it starts can keep another process's pipe open. A provider SHALL still receive its three pipes and the `WORKSPACE_SOCKET` and `WORKSPACE_TOKEN` environment.

#### Scenario: Providers started together
- **WHEN** the host starts several providers at the same moment while it also runs a Git query
- **THEN** each provider holds its standard input, output and error and no other descriptor of the host's

#### Scenario: A provider leaves a process running
- **WHEN** two providers start together and one leaves a long-lived process behind
- **THEN** the other provider still sees the end of its prompt, because that process holds neither end of its input pipe
