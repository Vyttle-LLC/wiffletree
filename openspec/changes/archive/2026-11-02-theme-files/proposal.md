## Why

Tidal is the only look the desktop app has. Its 14 colours come from one compiled-in token file and its fonts are written into about two dozen views, so nobody can change either without rebuilding the app. Michael wants to use Celadon's themes, which have one light and three dark variants, and to choose what light and dark each show.

The human approved the study in `ui/mockups.html` on November 2, 2026.

## What Changes

- **Theme files.** A theme is a JSON file in `<data dir>/themes/`, in Tidal's shape: a `name`, `variants` with `light`, `dark` or both, Tidal's 14 colour names as `#RRGGBB`, and an optional `fonts` block with `ui`, `mono` and `size` (11 to 20). A colour or font left out comes from Tidal's same variant. Unknown keys are ignored, so Tidal's own file is a valid theme.
- **A light theme and a dark theme.** Settings › Appearance gains Light theme and Dark theme menus under Mode. Each lists Tidal (built in) first, then each file with that variant, sorted by name, with its file name on the right. Mode still decides which shows. Open themes folder and New theme from Tidal sit under them.
- **Per Mac.** `desktop-preferences.json` gains `light_theme` and `dark_theme`; a missing or unknown value reads as Tidal and keeps the rest of the file.
- **The palette and fonts switch at runtime.** Every view that reads the palette follows the theme on show, and the hard-coded system fonts become the theme's UI and mono fonts, as do gpui-component's font family, mono font and base size.
- **Re-read without a restart.** The chosen files are read again when a theme is picked and whenever a Wiffletree window comes to the front.
- **Never broken by a file.** A broken, deleted or one-sided file shows Tidal's variant on that side, keeps the choice, and puts an error banner with Use Tidal in Settings › Appearance. A font this Mac lacks falls back to the system font with a warning banner there. Nothing else warns.
- **Not included:** the sidebar's Appearance menu keeps only System, Light and Dark; no Celadon files ship with Wiffletree (Celadon's own port supplies them); no heading font.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
- `native-workspace`: the desktop draws colours and fonts from a chosen light and dark theme file, with Tidal as the built-in default and fallback.

## Impact

- `workspace-desktop`: new `themes.rs` (file format, listing, choosing); `palette.rs` becomes a runtime store of the two looks; `preferences.rs` (`light_theme`, `dark_theme`); `settings.rs` (Appearance); `main.rs` (`apply_theme`, reloading on activation); every `.font_family(".System…Font")` call site.
- Docs: `docs/PRD.md`, `design/README.md`, the study in `ui/mockups.html`.
