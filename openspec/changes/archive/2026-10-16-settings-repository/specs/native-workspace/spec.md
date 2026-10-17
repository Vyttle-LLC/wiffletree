## MODIFIED Requirements

### Requirement: Check-in thresholds in Settings
The Settings window SHALL have a Check-ins section under the workspace folder with two preset selectors that save at once. "Check in on an agent every" SHALL offer 10, 25, 50, 100 and 250, followed by "turns", with the caption "The coordinator hears about an agent that has worked this many turns without a message from it. Nothing pauses." "Check in with you every" SHALL offer 50, 100, 250, 500 and 1000, followed by "coordinator turns", with the caption "You get an inbox note when the coordinator has worked this many turns without hearing from you. Nothing pauses." The saved value SHALL be highlighted; a saved value outside the presets SHALL highlight none. A refused save SHALL show the host's error in the section and keep the saved values.

#### Scenario: Choose an agent threshold
- **WHEN** the human clicks 50 beside "Check in on an agent every"
- **THEN** the host saves `checkin_child_turns` 50 with `checkin_human_turns` unchanged, and 50 is highlighted

#### Scenario: Hand-edited value
- **WHEN** the shared folder's `settings/check-ins.json` holds `checkin_human_turns` 300
- **THEN** the section opens with no coordinator preset highlighted, and the host uses 300
