## Context

`palette(dark)` read Tidal once into a `OnceLock` and is called in about 36 render functions, most without the theme or a window at hand. `apply_theme` copies the palette onto gpui-component's `Theme` and is already called on every Mode change and macOS appearance change. GPUI panics on the first line it lays out in a font family it cannot find, so a font named by a user file must be checked before it reaches any element.

## Decisions

- **`palette(dark)` keeps its signature.** It reads a process-wide store holding the resolved light and dark looks, which reloading replaces. None of its callers change, and Mode keeps choosing a side as before.
- **Fonts follow the look on show.** `apply_theme` records which side shows, and `ui_font()` and `mono_font()` return that side's families. Views call these instead of naming `.SystemUIFont` or `.SystemMonoFont`, so views without a window or `cx` (such as `ui::mono`) need no new parameter.
- **Tidal is parsed by the same reader.** The compiled-in `wiffletree-tidal.json` goes through the theme file parser, and a test holds that it has every colour in both variants. A user file's variant fills its gaps from it.
- **Lines come from serde_json.** Colours and the font size are checked while deserialising, so serde_json attaches the line of the bad value to the error; the banner shows that line and the key with its value. Whole-file problems (neither variant, the side it was chosen for missing, the file gone) have no line.
- **Installed fonts are listed once and listed again only on a miss.** Listing costs about 100 ms on macOS. A theme naming a font missing from the cached list lists again once, so installing the font and coming back to the app picks it up.
- **Reloading is cheap and idempotent.** It reads the folder's `*.json` files for the menus and resolves the two choices. It refreshes the component theme only when a look changed, and the windows when the menus or problems changed.
- **The choice is a file name.** `light_theme` and `dark_theme` hold a bare file name ending in `.json`, or `tidal`. Anything else, including a path, reads as Tidal.
- **New theme from Tidal writes Tidal's 14 colours** in the contract's order, with the name "My theme" (or "My theme 2" beside `my-theme-2.json`), rather than copying the token file with its `brand`, `muted` and `cyan` keys that a theme ignores.
- **One banner per file and kind.** When both sides chose the same broken file, one banner names both sides, and Use Tidal sets both back.

## Risks / Trade-offs

- **The text sizes the views set in pixels do not scale with `size`.** `size` sets gpui-component's base size, which the components' relative sizes follow; the app's own fixed sizes stay, as the study's caption allows ("the text that is sized relative to it").
- **Font names are matched exactly** against the families macOS lists. A misspelt or localised name reads as not installed, which the warning banner says.
