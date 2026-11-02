## 1. Theme files

- [ ] 1.1 `themes.rs`: parse a theme file with lines for bad values, resolve a side over Tidal, list the folder per side, name the problems.
- [ ] 1.2 `palette.rs`: a runtime store of the light and dark looks, `ui_font()` and `mono_font()` for the side on show.
- [ ] 1.3 Tests: complete, light-only, dark-only, partial fallback, invalid colour with its line, neither variant, size out of range, unknown keys, Tidal's own file, list filtering per side, a deleted chosen file, a one-sided mismatch.

## 2. Desktop

- [ ] 2.1 `preferences.rs`: `light_theme` and `dark_theme`, unknown values read as Tidal; tests for the round-trip and an older file.
- [ ] 2.2 `main.rs`: `apply_theme` sets the fonts and size; reload at launch, on a choice and when a window becomes active.
- [ ] 2.3 Every `.font_family(".System…Font")` call site uses the theme's font.
- [ ] 2.4 Settings › Appearance: Mode row, the two menus, Open themes folder, New theme from Tidal, the hint, error and warning banners with Use Tidal.

## 3. Docs

- [ ] 3.1 `ui/mockups.html` approved, `design/README.md`, `docs/PRD.md`.
