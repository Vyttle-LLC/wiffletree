# Wiffletree brand colors

**Selected: Tidal · October 2, 2026 · Product owner: Michael.**

Tidal combines deep teal, stone, and rust. It supersedes the Celadon theme family as the product default. Omneth and Omnith are retired naming candidates; the product name **Wiffletree** and its logo were selected on October 5, 2026 (see [Logo and typography](#logo-and-typography) and the [brand kit](brand/README.md)). The native baseline uses the macOS system family with 13 px navigation, 14 px conversation text, 18 px semibold titles and 10–12 px metadata. Line icons are bundled original vectors.

## Core palette

| Color | Hex | Brand use |
| --- | --- | --- |
| Deep teal | `#1A262D` | Dark foundation and light-mode text |
| Blue teal | `#1C3742` | Supporting depth and artwork |
| Tide | `#2D5A5A` | Supporting accents |
| Rust | `#902818` | Interface accent |
| Signal red | `#D63C35` | Logo joint |
| Stone | `#D1D1CD` | Neutral foundation and artwork |

The selected reference is the teal, stone, and rust palette supplied in the branding discussion; it is inspiration rather than a production image asset. Colors are adapted by semantic role rather than directly inverted between modes.

## Paired interface tokens

[wiffletree-tidal.json](themes/wiffletree-tidal.json) is the canonical machine-readable source. This table documents the selected values; update it alongside that file.

| Semantic role | Light | Dark |
| --- | --- | --- |
| Canvas (`base`) | `#F3F2ED` | `#1A262D` |
| Surface | `#FFFDF7` | `#22373E` |
| Raised / selected (`overlay`) | `#E4E9E3` | `#2A434A` |
| Primary text | `#1A262D` | `#F3F2ED` |
| Secondary text (`subtle`) | `#526865` | `#AFBFBC` |
| Decorative muted | `#748781` | `#829B99` |
| Separator (`edge`) | `#C4CFCA` | `#496063` |
| Primary accent | `#902818` | `#E6977E` |
| Text on accent (`on-accent`) | `#FFFDF7` | `#1A262D` |
| Focus / information (`cyan`) | `#2D5A5A` | `#86C1BC` |
| Success / additions (`green`) | `#326348` | `#A0CBB0` |
| Error / deletions (`red`) | `#A3213E` | `#FFA6B8` |
| Warning / attention (`yellow`) | `#77551C` | `#E4C17A` |
| Chart / branch blue | `#365E78` | `#9AC4D8` |
| Chart / branch magenta | `#765074` | `#CFADD0` |

## Usage

- Default to **System**, resolving to Tidal Light or Tidal Dark. Explicit Light and Dark selections persist on each client independently.
- Use stone or deep teal for most of the interface. Apply rust sparingly to primary actions, selection, and brand details. Dark mode uses a lighter warm accent of the same family.
- Keep brand accent separate from success, warnings, and errors. Rust is not the destructive-action token; successful checks and diff additions remain green. Pair every status with text or a glyph.
- In the project tree, warning yellow means *needs you* and nothing else. Working uses the focus color; Blocked, Done, Ready and Paused stay in secondary text so they never compete with it.
- Use `on-accent` for filled primary-action labels. Never assume those labels are always white or always the canvas color.
- Use secondary text for readable metadata; decorative muted colors are not normal-text tokens. Subtle separators are not focus indicators. Use the focus token for keyboard focus and essential control outlines.
- Keep the sample project named Celadon independent of product branding. Preserve archived studies and their original palettes.

## Validation and boundaries

The selected normal-text, secondary-text, accent-label, focus, and status-text combinations are checked against canvas, surface, and raised backgrounds. Targets remain 4.5:1 for normal text and 3:1 for essential controls and focus indicators. Diff and selected-state background mixes need their own checks in the actual UI.

The current HTML study demonstrates these colors and appearance switching. It does not implement native theme storage, custom theme import/export, provider operations, or GPUI rendering. Native accessibility and contrast validation remain release requirements in the PRD.

## Logo and typography

**Selected: October 5, 2026.** The full kit, with files, usage rules and the generator, lives in [`brand/`](brand/README.md).

- **Wordmark:** `wiffletree`, lowercase, Instrument Sans SemiBold (600), −0.03em tracking. The default is two-tone: “wiffle” in deep teal and “tree” in Tide on light backgrounds; off-white and Sea glass on dark backgrounds. The native font's unaccented i dot replaces the earlier colored squoval.
- **Symbol:** the selected Interlock open-joint W, with Signal red (`#D63C35`) at the central connection. The red is identical in both appearances and stays within the symbol. Single-color symbols are supplied for monochrome use.
- **Icon:** the Interlock symbol on a rounded deep teal, stone, Sea glass, or Tide tile. The old w-and-dot icon is superseded.
- **Typeface:** Instrument Sans (SIL OFL) for the logo and brand surfaces (site, docs headings, marketing). The native app UI keeps the macOS system family per the baseline above. The logo is supplied as outlined artwork, never live text.

Signal red is a logo color; this selection does not change the interface's existing rust/ember action tokens or error colors. See the [light/dark brand preview](brand/png/preview.png).
