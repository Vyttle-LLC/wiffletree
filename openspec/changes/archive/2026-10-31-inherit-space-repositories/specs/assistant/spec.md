## MODIFIED Requirements

### Requirement: Make this a project
A thread SHALL offer Make this a project. It SHALL ask the thread to write a project brief from the conversation and show it to the user, who SHALL be able to edit it and SHALL confirm it. Confirming SHALL create a Wiffletree project in the thread's Space, which uses that Space's repositories as every project does (see `spaces`), whose first human message is the brief with a link back to the thread, and whose conversation files are copies of the scratch files the user ticks. The thread SHALL stay open and read-only, and SHALL show a link to the project.

#### Scenario: A thread becomes a project
- **WHEN** the user confirms Make this a project in a Sage thread about a sign-in bug
- **THEN** a project is created in the Sage Space with the brief as its first message, its coordinator logs to the Sage brain, and the thread links to it
