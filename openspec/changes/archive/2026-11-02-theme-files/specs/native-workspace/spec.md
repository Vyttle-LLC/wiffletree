## ADDED Requirements

### Requirement: Theme files change the desktop's colours and fonts
The desktop SHALL read themes from `.json` files directly in the `themes` folder of its data directory, honouring `--data-dir`; the folder MAY be missing. A theme file SHALL hold a required `name`, `variants` with `light`, `dark` or both, and an optional `fonts` block. A variant's keys SHALL be Tidal's 14 colour names (`base`, `surface`, `overlay`, `text`, `subtle`, `edge`, `accent`, `on-accent`, `focus`, `green`, `red`, `yellow`, `blue`, `magenta`), each `#RRGGBB` in either case, and a colour a variant leaves out SHALL come from Tidal's same variant. `fonts` MAY name `ui` and `mono` font families and a base `size` in points from 11 to 20; each one left out SHALL be Tidal's, the system UI and mono fonts at 14. Unknown keys anywhere SHALL be ignored, so Tidal's own token file is a valid theme. A file with neither variant, a colour that is not `#RRGGBB`, a size outside 11 to 20, or JSON that does not parse SHALL be an error naming the line where it is known.

#### Scenario: A partial dark theme
- **WHEN** `rust-accent.json` holds only `variants.dark.accent` of `#FF7A59` and is chosen for dark
- **THEN** dark mode shows Tidal's dark colours with that accent, in the system fonts at 14

#### Scenario: A bad colour
- **WHEN** line 13 of `celadon-jade.json` reads `"accent": "#ffaee"`
- **THEN** the file is an error at line 13 saying `"accent": "#ffaee"` is not a color like #1A262D

### Requirement: A light theme and a dark theme are chosen in Settings
Settings › Appearance SHALL show Mode (System, Light, Dark), then a Light theme menu and a Dark theme menu. Each menu SHALL list Tidal, marked built in, first, then every readable theme file with that variant, sorted by its name, with its file name on the right, and SHALL check the current choice. Choosing SHALL apply at once in every window and save. Mode SHALL decide which side shows: System the side macOS is on, Light and Dark that side whatever macOS does. Open themes folder SHALL create the folder if it is missing and open it. New theme from Tidal SHALL write `my-theme.json`, or the first free `my-theme-N.json` from 2, with both of Tidal's variants, choose it on both sides and open the folder. The choices SHALL be saved per Mac as `light_theme` and `dark_theme` in `desktop-preferences.json`; a missing or unknown value SHALL read as Tidal without losing the file's other choices. The sidebar's Appearance menu SHALL keep only System, Light and Dark.

#### Scenario: One-sided themes
- **WHEN** Celadon Sky (light only) is the light theme, Celadon Jade (dark only) the dark theme, and Mode is System
- **THEN** Celadon Sky is absent from the Dark theme menu, and the app shows Celadon Sky while macOS is light and Celadon Jade, fonts included, once macOS turns dark

#### Scenario: An older preferences file
- **WHEN** `desktop-preferences.json` has no `light_theme`, or has `"dark_theme": 7`
- **THEN** that side is Tidal and the file's appearance and other choices load unchanged

### Requirement: Every view draws the theme on show
Every colour the desktop takes from the Tidal palette, and gpui-component's theme colours, SHALL come from the theme on show. The UI and mono fonts SHALL be the theme on show's, in every view and in gpui-component's font family, mono font and base font size; no view SHALL name a system font of its own. The chosen files SHALL be read again when a theme is chosen and whenever a Wiffletree window becomes active, and the windows redrawn when anything they show changed.

#### Scenario: Editing a theme
- **WHEN** the human edits the accent in the chosen dark file, saves it, and clicks back into Wiffletree
- **THEN** the new accent shows without a restart

### Requirement: A theme file never breaks the app
A chosen file that is missing, cannot be read or parsed, or lacks the variant it was chosen for SHALL draw Tidal's variant on that side while the choice stays. Settings › Appearance SHALL show an error banner naming the file, the line where known and the reason, saying which side shows Tidal and what the other side still shows, with Open themes folder and Use Tidal, which sets that side, or both sides when both chose the file, back to Tidal. A deleted file SHALL read "<file> is gone". A font this Mac does not have SHALL fall back to the system font of its kind, the theme's colours and other font still applying, with a warning banner in Settings › Appearance. No file SHALL panic the app, and nothing outside Settings › Appearance SHALL warn.

#### Scenario: A broken dark theme
- **WHEN** the dark theme's file has a bad colour and the light theme is Celadon Sky
- **THEN** dark mode shows Tidal, the banner says "celadon-jade.json can't be used." with the line and reason and "Dark mode shows Tidal until the file is fixed; light mode still shows Celadon Sky.", and the Dark theme menu still shows Celadon Jade

#### Scenario: A font that is not installed
- **WHEN** the theme on show names Figtree, which this Mac lacks, and JetBrains Mono, which it has
- **THEN** UI text uses the system font, code uses JetBrains Mono, the colours apply, and a warning says "Figtree isn't installed."
