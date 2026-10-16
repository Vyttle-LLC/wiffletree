## MODIFIED Requirements

### Requirement: Check-in thresholds are host settings
The shared folder's `settings/check-ins.json` SHALL hold `checkin_child_turns` and `checkin_human_turns` (see settings-repository). With no file, or a file without them, the host SHALL use 25 and 100. A `set_check_ins` command SHALL save both; it SHALL refuse a child threshold outside 5–500 or a human threshold outside 10–1000, keeping the saved values, and SHALL keep other settings and unknown fields. A saved change, made on this machine or pulled from another, SHALL apply from the next turn that starts, without a restart.

#### Scenario: Settings saved before this change
- **WHEN** `settings.json` has a workspace folder and verification but no check-in thresholds, and Wiffletree starts after an update
- **THEN** the host reads 25 and 100, and the verification setting is unchanged

#### Scenario: Lower child threshold
- **WHEN** the human saves `checkin_child_turns` 10 and an implementer then starts its 10th turn since its parent last messaged it
- **THEN** its parent receives a turn-count check-in naming "10 turns"

#### Scenario: Out of range
- **WHEN** `set_check_ins` is called with a child threshold of 4 or a human threshold of 1001
- **THEN** it is refused and the saved thresholds stay in effect

#### Scenario: Pulled from another machine
- **WHEN** a pass pulls `checkin_child_turns` 50 saved on another machine
- **THEN** the next turn that starts on this machine uses 50
