## ADDED Requirements

### Requirement: Dropped and pasted files are copied when attached
When files are dropped on a conversation, the desktop SHALL copy each one into its staging folder before attaching it, and the draft SHALL attach the copy. A file that cannot be copied SHALL be refused at the drop with its name and reason, and the rest SHALL still be attached. Cmd+V in the composer SHALL attach files copied in Finder as a drop does, and SHALL save an image on the clipboard as a PNG named `Pasted image <date> at <time>.png`, numbered when the draft already holds that name, and attach it as a dropped image. A paste with neither SHALL insert its text as before. Attached copies SHALL be checked against the existing limits. A staged copy SHALL be removed once no draft or send uses it: when its draft is sent, when it is removed from the draft and no send is reading it, and when its window closes, and the staging folder SHALL be emptied when the app starts.

#### Scenario: A screenshot thumbnail is dragged in
- **WHEN** the human drags a screenshot's floating thumbnail into the composer and sends the message
- **THEN** the message is delivered with the image, even though the original file is gone by then

#### Scenario: A file that cannot be read
- **WHEN** a dropped file cannot be copied
- **THEN** it is refused at the drop with "Could not copy <name>: <reason>", and Send never fails on it

#### Scenario: A screenshot is pasted
- **WHEN** the clipboard holds a screenshot and the human presses Cmd+V in the composer
- **THEN** an image chip with a thumbnail appears, and Send delivers the image as a dropped one

#### Scenario: Text is pasted
- **WHEN** the clipboard holds text and the human presses Cmd+V in the composer
- **THEN** the text is inserted and nothing is attached

#### Scenario: Copies do not outlive their draft
- **WHEN** a draft with staged files is sent, a staged file is removed, the window closes, or the app restarts
- **THEN** those staged copies are deleted
