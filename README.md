# Wiffletree

A native workspace for orchestrating Claude and Codex agents across tasks and repositories.

A Rust/GPUI desktop app with durable agent messaging, repository coordinators, isolated ticket worktrees, configurable role models, native Markdown conversations, account-quota indicators and daily usage graphs across coordinators. The host runs Codex through `codex exec --json` and Claude through `claude -p --output-format stream-json`, resuming each provider conversation between assignments. Live Claude/Codex execution, exact-session resume and a mixed-provider ticket handoff have been verified. This is a local development build; the full PRD release gates remain open.

Wiffletree is the product name. The GitHub repository is [Vyttle-LLC/wiffletree](https://github.com/Vyttle-LLC/wiffletree).

## Install

Wiffletree runs on Apple Silicon Macs with macOS 13 or later. Download the latest `Wiffletree-<version>-macos-arm64.zip` from [Releases](https://github.com/Vyttle-LLC/wiffletree/releases/latest), unzip it and move `Wiffletree.app` to Applications. You also need the [Claude Code](https://code.claude.com) and/or [Codex](https://github.com/openai/codex) CLIs, signed in; see [provider setup](docs/DEVELOPMENT.md#provider-setup).

Installed releases check for updates hourly and download them in the background. To check right away, click the version number at the bottom of the sidebar or choose **Check for Updates…** from the Wiffletree menu. When one is ready, a card at the bottom of the sidebar shows what's new, with a link to the changelog and **Restart to update**. Quitting also installs it.

Agents run with their provider's permission prompts bypassed so they can work unattended. Attach only repositories you are comfortable letting them change.

## Build from source

Run the native preview on macOS:

```sh
cargo build --locked --release -p workspace-host -p workspace-desktop
./target/release/workspace-desktop \
  --data-dir /tmp/wiffletree-preview --demo-repository "$PWD"
```

Build a clickable development app with `bash scripts/package_macos.sh`, or `--beta` for a separate Wiffletree Beta that runs beside it with its own store. See [development and morning handoff](docs/DEVELOPMENT.md) for implemented capabilities, model configuration, headless commands, validation, design choices and remaining work. [Measured host timings](docs/performance/host-baseline.md) are separate from the unverified desktop and provider performance gates.

## Start here

- [Product requirements](docs/PRD.md) — current scope, architecture, performance budgets, project storage, and phased acceptance criteria.
- [Current mockup](design/mockups/current/index.html) — chat-first visual study; some simulated task terminology predates the current repository-team workflow.
- [Design guide and earlier studies](design/README.md) — what is current, reference assets, and prototype limitations.
- [Brand colors](design/brand.md) — the selected Tidal palette and paired light and dark tokens.

GitHub shows HTML source rather than running the mockup. Preview locally:

```sh
python3 -m http.server 8000 --bind 127.0.0.1
```

Open `http://127.0.0.1:8000/design/mockups/current/index.html` in a browser. Icons and optional display utilities use public CDNs; the preview does not send messages or file contents to agent providers.

## Direction

- Rust orchestration host; GPUI client on macOS first. Tauri is a fallback if feasibility work demonstrates a concrete blocker.
- Project/main coordinator → repository coordinators → tickets with implementation, test, and review agents. A repository coordinator persists across its project’s tickets; each worker belongs to one ticket.
- The main orchestrator consolidates questions and approvals. Workers report blockers to their coordinator.
- Independent provider sessions communicate through durable application messaging.
- Headless execution and remote clients; one authoritative host per project initially.
- Project storage outside code repos, with an attached Markdown brain.
- Tidal brand palette: deep teal, stone, and rust, with paired light and dark modes. Celadon remains the example project in the mockups, not the product name or current theme.

## Editing

Edit `docs/PRD.md` for requirements and `design/mockups/current/source.html` for the active UI study. Regenerate browser-ready mockups with:

```sh
python3 scripts/build_mockups.py
```

Keep requirements and the current design aligned. Archived studies are historical references and do not supersede the PRD. No package install or application build is needed for these static prototypes.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow and [SECURITY.md](SECURITY.md) to report a vulnerability privately.

## License

Wiffletree is released under the [MIT License](LICENSE). The bundled Instrument Sans subset used for the wordmark is under the [SIL Open Font License](design/brand/source/OFL.txt).
