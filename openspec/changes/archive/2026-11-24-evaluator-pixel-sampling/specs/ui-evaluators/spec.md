## ADDED Requirements

### Requirement: An evaluator can measure exact colours and contrast

Both drivers SHALL offer `sample_pixels`, through one shared module, which reads only a PNG the screenshot tool saved in the run folder and writes, sends and opens nothing else. Its input SHALL be `screenshot`, the saved file's name; `points`, one to 64 whole-pixel `{x, y}` in that image's pixels from its top-left corner; and optionally `pairs`, up to 64 `[i, j]` indexes into `points`. It SHALL return each point's sRGB hex as `#RRGGBB` and its WCAG 2.x relative luminance; for each pair the contrast ratio rounded to two decimals, and whether it passes AA for normal text (4.5) and for large text and user-interface components (3.0), judged on the unrounded ratio; and the image's pixel size. Its description SHALL say that coordinates are image pixels and that, for a native-scale screenshot, the model divides by the reported scale to get points. It SHALL refuse a name outside the run folder, a path through a link, a file that is not a regular `.png` file or does not decode as a PNG, a point outside the image, naming it and the image's size, a point that is not a whole non-negative number, more than 64 points or pairs, and a pair naming a point that is not listed.

#### Scenario: Known colours

- **WHEN** the evaluator samples a point of #1A262D and a point of #D1D1CD in a saved screenshot, and pairs them
- **THEN** the tool returns `#1A262D` and `#D1D1CD` with their luminances, a contrast of 10.09 that passes both AA thresholds, and the image's size

#### Scenario: A pair that fails normal text

- **WHEN** the evaluator pairs a #777777 point with a white one
- **THEN** the contrast is 4.48, which fails AA for normal text and passes it for large text and UI components

#### Scenario: A point outside the image

- **WHEN** the evaluator samples (2, 1) in a 2 × 2 pixel screenshot
- **THEN** the tool refuses it, naming the point and the image's size

#### Scenario: A file it may not read

- **WHEN** the evaluator names a file outside its folder, a link, a text file or a file that is not a PNG
- **THEN** the tool refuses it and reads nothing else


## MODIFIED Requirements

### Requirement: The app driver reaches only the copy's window

An app run's only MCP server SHALL be the app driver, connected to its copy's socket, with the tools screenshot, click, double_click, right_click, move, type_text, key, scroll, drag, resize_window, window and sample_pixels, in window points that match a screenshot at scale 1. A screenshot SHALL be saved as a PNG inside the run folder under the name the model gives and returned as an image with its pixel size and the window's scale. resize_window SHALL report the size reached. Because Claude downsizes an image over 2000 pixels on its long edge, the screenshot tool SHALL tell the model to sample pixels from a shot at one pixel per point, or a native shot small enough, rather than a native shot of a large window. The app SHALL capture its own window, at one pixel per point or the display's scale, without Screen Recording permission; on macOS 27.0.1 this was measured from a launch with no grant.

#### Scenario: A screenshot

- **WHEN** the evaluator calls screenshot with the name `screenshots/models-dark` and scale native
- **THEN** `screenshots/models-dark.png` in its folder holds the window at the display's scale, and the tool returns that image with its size and scale

#### Scenario: A name outside the folder

- **WHEN** the evaluator names a screenshot `../x.png`
- **THEN** the tool refuses it and writes nothing

#### Scenario: Another server or tool

- **WHEN** a Claude app run starts with any MCP server but the connected driver, or a tool beyond its files and the driver's, or a Codex app run calls another server's tool
- **THEN** the run is stopped

### Requirement: A full-desktop run sees and touches only the apps it names

`start_evaluation` in desktop mode SHALL take `apps`, one to ten bundle IDs, and no target, and SHALL refuse an empty list, a malformed ID and Wiffletree Evaluator itself. Nothing SHALL be allowed that the coordinator does not name. The run's only MCP server SHALL be the desktop driver, named `desktop`, for Claude and Codex alike, with the tools screenshot, click, double_click, right_click, move, type_text, key, scroll, drag, open_app, list_windows and sample_pixels, in screen points; every screenshot SHALL report its scale and be saved inside the run folder by the app driver's rules. The helper, not the model, SHALL enforce:

- a screenshot draws only the allowed apps' windows, each captured alone, at their screen frames, so its pixels divided by its scale are screen points; never the whole display;
- a click, move, scroll or drag is refused unless the topmost on-screen window at its point, at that moment, belongs to an allowed app;
- keys and text are refused unless an allowed app is frontmost, and go to that app's process alone;
- open_app opens only an allowed app, and list_windows lists only their windows.

#### Scenario: Input over another app

- **WHEN** an evaluator allowed only Calculator clicks a point where a notification banner covers Calculator
- **THEN** the click is refused, and the reply does not name the other app

#### Scenario: Keys while another app is frontmost

- **WHEN** Terminal is frontmost and the evaluator types text
- **THEN** nothing is typed and the tool says no allowed app is frontmost

#### Scenario: No apps

- **WHEN** a coordinator starts a desktop run with no apps
- **THEN** the call is refused and no run is created
