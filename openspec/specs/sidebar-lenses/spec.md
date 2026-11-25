# sidebar-lenses Specification

## Purpose
The sidebar groups the same projects by outcome, app, layer or what needs the human, without renaming them.

## Requirements

### Requirement: A lens control under the Projects heading
Under the PROJECTS header, the sidebar SHALL show a segmented control with four lenses: Outcome, App, Layer and Me, in that order, with Outcome the default. ⌘1, ⌘2, ⌘3 and ⌘4 SHALL select them while the main window is focused, including while a composer has focus. The selected lens SHALL be saved with the desktop's other view choices in `desktop-preferences.json`, and a file without it SHALL read as Outcome. Changing lens SHALL change only how the list is drawn: never the stored order, a parent link, the selection or which conversation is open. The Me segment SHALL carry a badge counting the Me lens's entries across all Spaces (see "The Me lens lists what waits on the human"), whichever lens is selected, and none when there are none.

#### Scenario: Switching with the keyboard
- **WHEN** the Outcome lens is shown and the human presses ⌘3 while typing in a composer
- **THEN** the Layer lens is shown, the draft is unchanged and the same conversation stays open

#### Scenario: The last lens is remembered
- **WHEN** the human selects App and relaunches the app
- **THEN** the App lens is shown

#### Scenario: An older preferences file
- **WHEN** `desktop-preferences.json` was saved before lenses existed
- **THEN** the Outcome lens is shown and the file's other choices are kept

#### Scenario: The Me badge in another lens
- **WHEN** the App lens is selected, Celadon's Me lens has three entries and Vyttle's one
- **THEN** the Me segment's badge reads "4"

### Requirement: Spaces stay the top level in every lens
Every lens SHALL draw a section for each Space that has something to show in it, in the order of Settings › Spaces, and "No Space" last, as the Space sections do (see native-workspace). A lens SHALL never list a project outside its Space's section. Which sections and which sections' inner rows are collapsed SHALL be kept in memory for each lens, so switching lens and back keeps them, and an app restart starts them as each lens defines.

#### Scenario: Sections in the App lens
- **WHEN** the App lens is shown and Celadon and Vyttle each have projects
- **THEN** the list shows the CELADON section, then VYTTLE, each with its own app buckets

### Requirement: The Outcome lens draws the parent tree
The Outcome lens SHALL list, in each Space's section, its loose projects and its parents in the stored order, and under each expanded parent its children in the stored order, indented, with a branch icon instead of the project icon. A standing project SHALL show a "standing" pill. A child SHALL show a hollow badge for its questions with its parent (see parent-projects). There SHALL be no group rows: first-word groups SHALL NOT be drawn in any lens.

#### Scenario: Parents, children and loose projects
- **WHEN** Celadon's stored order is "Accessible contrast", "Contrast warnings", "Light themes", "Recipes" and "Contrast warnings" is a child of "Accessible contrast"
- **THEN** the section reads "Accessible contrast" with "Contrast warnings" indented under it, then "Light themes", then "Recipes"

#### Scenario: A child stored above its parent
- **WHEN** the stored order lists the child "Contrast warnings" above its parent "Accessible contrast"
- **THEN** "Contrast warnings" is still drawn under "Accessible contrast", where the parent sits

#### Scenario: No groups
- **WHEN** two loose projects in Celadon are named "Pickers: restore highlights" and "Pickers: generated palette check"
- **THEN** both are ordinary rows with their full names, and no group row is drawn

### Requirement: A parent row collapses and rolls up its children
A parent row SHALL show a chevron, and activating the chevron SHALL collapse or expand its children. Collapsed, the row SHALL show "+N", where N is its number of shown children, and the most urgent state among the parent and its shown children: the number of items that need the human when there are any, otherwise the number of stuck agents as the stuck pill when there are any, otherwise blocked when a row would show blocked, otherwise working when a row would show working, and otherwise nothing. Parents SHALL start expanded at app start. Selecting a child inside a collapsed parent, such as from the Inbox, the Agents panel or a notification, SHALL expand it.

#### Scenario: Collapsed with a question
- **WHEN** "Accessible contrast" is collapsed with two children, and one child has a question with the human
- **THEN** its row shows "+2" and the needs-you count "1"

#### Scenario: Questions with the parent do not count
- **WHEN** a collapsed parent's only waiting questions are with the parent itself, and one child is working
- **THEN** its row shows the working state, not a needs-you count

#### Scenario: Selecting reveals a child
- **WHEN** a child of a collapsed parent is selected from the Agents panel
- **THEN** the parent expands and the child's row shows selected

### Requirement: Dragging in the Outcome lens reorders and files
Only the Outcome lens SHALL let rows be dragged. The rules of dragging within a Space's section (see native-workspace) SHALL apply, with parents in place of groups: a parent SHALL drag with all its shown children as one block among the loose projects and parents of its section, and a child SHALL reorder among its siblings. Dropping a loose project with no children on the middle of a loose project or a parent in the same section, or on an insertion line among a parent's children, SHALL ask to move it under that project; dropping a child on an insertion line among loose projects and parents SHALL ask to make it loose. The confirmation SHALL name the project and its new parent, or say it becomes loose, and confirming SHALL make the change and place the project where it was dropped. Cancelling SHALL change nothing. A drop the host would refuse, such as a project with children onto another project or any project onto a child, SHALL show no target and SHALL do nothing.

#### Scenario: Filing by drag
- **WHEN** the human drops the loose "Redo sponsor page" on "Sponsorships" and confirms
- **THEN** "Redo sponsor page" is drawn as a child of "Sponsorships"

#### Scenario: Cancelled
- **WHEN** the human drops a loose project on a parent and cancels the confirmation
- **THEN** no link or order changes

#### Scenario: Unfiling by drag
- **WHEN** the human drags the child "Generated palette check" to an insertion line between two loose projects and confirms
- **THEN** it is loose and sits between them

#### Scenario: A parent moves as a block
- **WHEN** "Light themes", with two children, is dropped above "Accessible contrast"
- **THEN** "Light themes" and both children sit above "Accessible contrast", children in the order they had

#### Scenario: Nothing to drop on
- **WHEN** the human drags "Accessible contrast", which has children, over "Sponsorships"
- **THEN** "Sponsorships" shows no target, and a drop there moves nothing

#### Scenario: Other lenses are read-only
- **WHEN** the App, Layer or Me lens is shown and the human presses on a row and moves the pointer
- **THEN** no drag starts, and a release on the same row selects it as a click does

### Requirement: The App and Layer lenses list projects under each tag
The App lens SHALL list, in each Space's section, one bucket for each app of the Space's vocabulary that a shown project has, in the vocabulary's order, then "no app" and then "not started", each only when it holds a project; the Layer lens SHALL do the same with layers and "no layer". Each bucket row SHALL show the value as a chip and its project count. A project SHALL be listed under every bucket its tags put it in (see spaces), each time in the stored order, whether it is loose, a parent or a child. A child's row SHALL have a muted line beneath it naming its parent. A bucket SHALL collapse from its row; collapsed, it SHALL show its first three project names and "+N" for the rest. Buckets SHALL start expanded at app start.

#### Scenario: A project in two apps
- **WHEN** "Readable diff colours", a child of "Accessible contrast", has tickets in `nvim` (nvim) and `google-chrome` (chrome)
- **THEN** the App lens lists it under "nvim" and under "chrome", each with the line "Accessible contrast" beneath it

#### Scenario: No app and not started
- **WHEN** "DNS: website Terraform" has tickets only in `celadon-infra`, which has no app, and "Recipes" has no tickets
- **THEN** the App lens lists "DNS: website Terraform" under "no app" and "Recipes" under "not started"

#### Scenario: A collapsed bucket
- **WHEN** the human collapses the "frontend" bucket holding six projects
- **THEN** its row shows the first three names and "+3"

### Requirement: The Me lens lists what waits on the human
The Me lens SHALL list, in each Space's section, up to four buckets in this order, each only when it holds an entry, and each entry SHALL be a project row with a muted reason line beneath it:
- **Questions:** projects with an Inbox item with the human. The reason SHALL be the item's prompt, or "N questions" for several, followed by " · via <parent>" when a parent escalated them.
- **Merge on GitHub:** projects with a ticket whose pull request is open, whose latest review passed, and whose base is a protected branch the coordinator cannot merge into. The reason SHALL read "PR #N into <base> passed review".
- **Going quiet:** unarchived projects, not ready to close, where no session of the project, nor of its unarchived children, has started a turn for 5 days or more. An archived child's turns SHALL NOT count toward its parent's latest turn. A project none of whose sessions has ever started a turn SHALL NOT be listed. Standing projects and projects with no tickets SHALL be included. The reason SHALL read "No turn for N days", or "No turn for N days, not started" for a project with no tickets.
- **Ready to close:** unarchived projects that are not standing, have at least one ticket, every ticket accepted or closed, every pull request of its tickets merged, and every child archived. A ticket with no pull request SHALL not prevent it; a pull request open or closed without merging SHALL. The reason SHALL read "Every ticket accepted or closed, PRs merged".

A project MAY be an entry in several buckets. The section of the selected project's Space SHALL start expanded and the others collapsed, each collapsed section showing its entry count. Selecting an entry SHALL open the project's coordinator. The Me lens SHALL be built from the host's records with no agent behind it, and there SHALL be no separate Needs you page.

#### Scenario: An escalated question
- **WHEN** "Accessible contrast" escalated two questions from "Contrast warnings"
- **THEN** the Me lens lists "Contrast warnings" under Questions with the reason "2 questions · via Accessible contrast"

#### Scenario: A pull request only the human can merge
- **WHEN** "Redo sponsor page" has PR #41 into `main`, a protected branch, whose review passed
- **THEN** it is listed under Merge on GitHub with "PR #41 into main passed review"

#### Scenario: Going quiet
- **WHEN** "Colour-blind palettes" has an open ticket and none of its sessions has started a turn for six days
- **THEN** it is listed under Going quiet with "No turn for 6 days"

#### Scenario: A standing project going quiet
- **WHEN** "Platform" is standing and none of its or its children's sessions has started a turn for five days
- **THEN** it is listed under Going quiet with "No turn for 5 days"

#### Scenario: Quiet before it started
- **WHEN** "Recipes" has no tickets and its coordinator has not started a turn for seven days
- **THEN** it is listed under Going quiet with "No turn for 7 days, not started"

#### Scenario: An archived child's turns do not count
- **WHEN** "Light themes" has not started a turn for six days, and its archived child "Generated palette check" started one yesterday
- **THEN** "Light themes" is listed under Going quiet with "No turn for 6 days"

#### Scenario: Never started a turn
- **WHEN** none of the sessions of "Recipes" has ever started a turn
- **THEN** "Recipes" is not under Going quiet

#### Scenario: Ready to close
- **WHEN** every ticket of "CI runners" is accepted, its PRs are merged and it is not standing
- **THEN** it is listed under Ready to close with "Every ticket accepted or closed, PRs merged"

#### Scenario: A pull request closed without merging
- **WHEN** every ticket of "Recipes" is closed, it is not standing and has no open child, but one ticket's PR was closed without merging
- **THEN** "Recipes" is not under Ready to close

#### Scenario: A parent with an open child is not ready
- **WHEN** every ticket of "Light themes" is accepted but its child "Generated palette check" is unarchived
- **THEN** "Light themes" is not under Ready to close

#### Scenario: Other Spaces start collapsed
- **WHEN** a Celadon project is selected and Vyttle has one entry
- **THEN** the Me lens shows Celadon expanded and Vyttle collapsed showing "1"

### Requirement: Tag chips in a project's header open a lens
The header of a project's coordinator conversation SHALL show one chip for each app of the project, then one for each layer, and none for a project with no tags. Activating a chip SHALL select the App or Layer lens, expand the project's Space section and that tag's bucket, and scroll the list so the bucket shows. It SHALL NOT change the selected conversation.

#### Scenario: A layer chip
- **WHEN** the header of "Contrast warnings" shows "nvim", "frontend" and "backend", and the human activates "backend"
- **THEN** the sidebar shows the Layer lens with Celadon's "backend" bucket expanded and in view, and "Contrast warnings" stays open

#### Scenario: Not started
- **WHEN** a project has no tickets
- **THEN** its header shows no chips
