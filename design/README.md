# Design studies

## Current direction

[Open the current mockup](mockups/current/index.html), served locally as described in the root README. Its editable fragment is [source.html](mockups/current/source.html).

The UI leads with the main orchestrator's conversation. The native hierarchy is project/main coordinator → repository coordinator → tickets → agents. The HTML study still contains simulated examples from the earlier task-orchestrator model; the current PRD and native app define the updated workflow. Glyphs distinguish working, blocked, done, and paused; human escalation appears on the main orchestrator. A scoped inspector rail stays at the right edge. Selecting a tool opens a panel naming the selected project, task or agent; project selection hides task-only Git tools. Sidebar and inspector widths are adjustable. Context files remain separated by project, task and conversation scope.

**Wiffletree** is the product name, selected October 5, 2026; **Tidal** remains the selected brand color palette as of October 2, 2026. The current study uses Wiffletree branding and paired Tidal light and dark modes. Celadon remains its sample project; it is no longer the product's theme family. See [Brand colors](brand.md) for the palette, semantic roles, and usage rules. The [brand kit](brand/README.md) pairs the selected red Interlock open-joint symbol with the original two-tone wordmark; [preview both appearances](brand/png/preview.png).

## Prototype boundaries

- Chat submission, permissions, Git actions, PR data, timers, and usage are local simulations.
- Context selection and sent file-reference chips are interactive. File imports retain only names and sizes in the session; no durable upload or provider delivery is implemented.
- Some presentation choices and drafts persist in browser storage. Sample conversations, created agents, approvals, and imported file metadata may reset on reload.
- The file-picker flow was not verified end to end in the sandboxed in-app browser. Selection of sample context files, sent references, task/agent creation, and approval routing were exercised.
- The responsive layout was checked at desktop and 320 px widths. The application is intended for a desktop-first native implementation.
- These are HTML studies, not the GPUI implementation or evidence of native performance.

## Earlier studies

These preserve the design progression and may contain superseded repository-first layouts or palettes.

| Study | Browser-ready version | Editable source |
| --- | --- | --- |
| Initial agent workspace | [Preview](mockups/archive/agent-workspace/index.html) | [Source](mockups/archive/agent-workspace/source.html) |
| Git and PR workspace | [Preview](mockups/archive/agent-git-workspace/index.html) | [Source](mockups/archive/agent-git-workspace/source.html) |
| Project studio layouts | [Preview](mockups/archive/project-studio/index.html) | [Source](mockups/archive/project-studio/source.html) |
| Earlier color study | [Preview](mockups/archive/project-studio-theme/index.html) | [Source](mockups/archive/project-studio-theme/source.html) |
| Celadon overview study | [Preview](mockups/archive/celadon-studio/index.html) | [Source](mockups/archive/celadon-studio/source.html) |

## References and provenance

- `themes/wiffletree-tidal.json` is the canonical, versioned Tidal token file. The mockup builder embeds its paired light and dark variants in the current study's marked palette block; edit the JSON rather than its generated copy.
- The Celadon, Powder, Jade, and Sky JSON snapshots in `themes/` support historical studies, from [Celadon Theme](https://celadontheme.com/palette) and its [JSON ports](https://github.com/celadon-theme/celadon-theme/tree/main/ports/json). They do not define the current product colors.
- Tidal follows the user-selected stone, deep teal, and rust reference from the branding discussion. The semantic variants adapt it for readable light and dark interfaces; see `brand.md`.
- `runtime/` preserves the standalone preview wrapper exported from the conversation, including its sandbox, CSP, state bridge, icon and tooltip utilities. It is preview support only, not proposed application infrastructure.
- The initial PRD and studies were migrated on October 2, 2026 from the original planning page. Future requirements live in this repository.

Run `python3 scripts/build_mockups.py` after editing a source fragment. This only rebuilds local HTML exports; it does not deploy a website.
