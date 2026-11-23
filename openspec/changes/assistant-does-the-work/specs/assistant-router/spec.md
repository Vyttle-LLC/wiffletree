## REMOVED Requirements

### Requirement: The router answers typed questions
**Reason**: The Assistant is one conversation that infers each request's Space itself and asks inline only when unsure (see assistant, "The Assistant infers each request's Space"). There are no threads to route between.
**Migration**: None. A message held by the router at the upgrade is shown in the conversation as the human's unsent draft.

### Requirement: The router sees only what it needs
**Reason**: No router runs.
**Migration**: None.

### Requirement: Host rules act on the answers
**Reason**: No router answers to act on, and no confidence threshold. Follow-ups to work carry a tag with Change instead (see assistant, "Follow-ups go to the work they are about").
**Migration**: The threshold in `settings/assistant.json` is kept as an unknown field and no longer read.

### Requirement: Unsure asks with one-click choices
**Reason**: The Assistant asks inline itself when it cannot tell a request's Space.
**Migration**: None.

### Requirement: Changing a routed message
**Reason**: Replaced by Change on a message's work tag (see assistant, "Follow-ups go to the work they are about").
**Migration**: The messages of earlier threads stay in their history; the lines Change and the router left in them are no longer shown, and their records stay in the store, unread.

### Requirement: The router never writes
**Reason**: No router runs.
**Migration**: None.

### Requirement: Router providers behind one interface
**Reason**: No router runs, so the interface and its Claude provider are deleted.
**Migration**: The Router row is dropped from Who runs what (see model-routing).
