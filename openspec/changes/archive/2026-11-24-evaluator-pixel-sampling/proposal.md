## Why

The UX loop's rubric asks evaluators for sampled pixel hex codes and WCAG 2.x contrast ratios. Evaluators have no shell, so today they can only estimate colours by eye from a screenshot, and a model reading an image gives approximate hex codes and unreliable ratios. Michael approved a tool that measures them exactly on November 24, 2026.

## What Changes

- **A `sample_pixels` tool in both drivers**, app and desktop, through one shared module so the two cannot drift. It takes the name of a PNG the screenshot tool saved in the run folder, up to 64 points in that image's pixels, and optionally up to 64 pairs of point indexes.
- **It returns** each point's sRGB hex and WCAG relative luminance; each pair's contrast ratio, rounded to two decimals, with whether it passes AA for normal text (4.5) and for large text and UI components (3.0), judged on the unrounded ratio; and the image's pixel size, so the model can check its coordinates.
- **It is read only**: it reads that one file by the same containment rules as saving a screenshot and, as a read, refuses a link; it writes nothing, uses no network and opens no other file. A point outside the image is refused with a message naming it and the image's size.
- **It is one more tool name** on the same server, in the drivers' tool lists and so in the Claude allow list, Codex's enabled tools and both providers' start and tool-call checks.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ui-evaluators`: the app and desktop drivers' tool lists gain `sample_pixels`, and a new requirement says what it reads, returns and refuses.
