## Purpose

Boards is the library beside the assistant of every output a thread or job saves, and the place to open, step back through and share them.

## ADDED Requirements

### Requirement: The Boards library
Boards SHALL list every output saved by a thread or job on this Mac or pulled from a Space's storage, each with its title, Space, the job that wrote it or "From a thread", its last update, a preview and where it is kept: "Shared in ‹Space›" or "Only on this Mac". It SHALL filter by Space and job, search titles, and sort by last update. A job's output SHALL show "Runs on ‹name›'s Mac" for a shared job, and "Stale" when two scheduled runs passed without a new version. Boards SHALL also list the jobs, with their schedule, next run, last outcome and runner.

#### Scenario: A mixed library
- **WHEN** the user has a shared Sage dashboard, a Personal brief and a chart saved from a thread
- **THEN** Boards lists all three with their Space, source, last update and where each is kept

### Requirement: Outputs open by type in Wiffletree
Clicking an output SHALL open it in Wiffletree. Markdown SHALL render natively with the app's Markdown rendering. HTML SHALL open in an embedded web view. Images and PDFs SHALL open in Quick Look. Any other file SHALL offer "Open in default app". Every output SHALL also offer Reveal in Finder and Open in default app.

#### Scenario: A Markdown brief
- **WHEN** the user opens the morning brief, a Markdown file
- **THEN** it renders in Wiffletree with headings, lists and links, and no web view is created

#### Scenario: A spreadsheet
- **WHEN** the user opens an `.xlsx` output
- **THEN** Wiffletree offers Open in default app

### Requirement: HTML boards run without network or navigation
An HTML output SHALL open in a WKWebView embedded in the Wiffletree window. Its scripts SHALL run. It SHALL load only files inside its own output folder, through Wiffletree's own scheme, and SHALL have no network access: requests, fetches and sockets to anything else SHALL fail. It SHALL NOT navigate away, open windows, download files or use the camera, microphone, location or notifications. A link to a web address SHALL offer "Open in browser" instead of navigating. Its storage and cookies SHALL NOT survive closing it. The assistant's instructions SHALL tell threads and job runs to inline any chart library in the HTML they save.

#### Scenario: An inlined chart
- **WHEN** a dashboard inlines its chart library and draws a chart with a script
- **THEN** the chart draws in the board

#### Scenario: A script that calls out
- **WHEN** a board's script fetches `https://example.com/data.json`
- **THEN** the fetch fails and nothing leaves the Mac

#### Scenario: A link
- **WHEN** the user clicks a link to `https://sentry.io/…` in a board
- **THEN** the board stays where it is and Wiffletree offers Open in browser

### Requirement: The web view sits inside the GPUI window
The embedded web view SHALL follow the bounds of the board pane as the window resizes and the panes move, SHALL be hidden whenever the board pane is not shown, and SHALL never draw over a Wiffletree menu, popover, sheet or notice: while one is open over the board pane, the pane SHALL show a still image of the board. Window shortcuts SHALL reach Wiffletree while the board has focus. A closed board SHALL hold no web view.

#### Scenario: A menu over a board
- **WHEN** the user opens the Space menu while an HTML board is open
- **THEN** the menu draws above a still image of the board, and the live board returns when the menu closes

### Requirement: Every output keeps its history
Every version of an output SHALL be a Git commit in the repository that holds it: the Space's storage for shared outputs, and this Mac's private board store for the rest. An open output SHALL offer its versions with their time and the job run or thread that wrote them, and opening one SHALL show that version without changing the latest.

#### Scenario: Last week's dashboard
- **WHEN** the user picks last Monday's version of the release dashboard
- **THEN** that version opens, and the job's next run still replaces the latest

### Requirement: Sharing an output is the user's choice
An output that is only on this Mac SHALL offer "Share to Space…", which SHALL copy it into a Space's storage only after a confirmation naming the Space and who can read its storage. No thread or job run SHALL move an output between Spaces or from this Mac to a storage repository.

#### Scenario: A thread's chart
- **WHEN** a Sage thread saves a chart
- **THEN** it is listed as only on this Mac until the user chooses Share to Space… and confirms
