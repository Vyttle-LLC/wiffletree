## 1. Host

- [ ] 1.1 `compile_schedule`, the one reading of a compile, used by `refresh_compile_jobs`, Set up and `set_brain_compile_time`.
- [ ] 1.2 `set_up_brain` takes and writes `compile`; `BrainsPage.this_timezone`.
- [ ] 1.3 `Command::SetBrainCompileTime`, human only, with `docs/host-protocol.md`.
- [ ] 1.4 `Job::space_may_set`, `validate_space_rows`, `SpaceModels::ignored` and `for_space` dropping the two jobs.
- [ ] 1.5 `set_space_model_settings` and `check_space_models` use `validate_space_rows`; Effective settings and `workspace_context` leave the two jobs out.
- [ ] 1.6 Tests: compile time written at Set up and refused when unreadable, a merged Set up scheduled, the config PR keeping unknown keys, a Space save refusing a new row while allowing an unrelated save with an old one that breaks the table's rules, Effective settings leaving them out.

## 2. Desktop

- [ ] 2.1 Brains page: the Set up form, the no-time banner, Change compile time, Not scheduled in the warning colour.
- [ ] 2.2 Space page: "Change on Brains →", the warning colour, the `brain_facts` comment, the row count and the ignored note.
- [ ] 2.3 Models page: both rows and the placeholder gone at a Space, the banner with Remove them, the line on This Mac, draft errors from `validate_space_rows`.
- [ ] 2.4 New Space wording.
- [ ] 2.5 `PageScroll`, and the Settings sidebar's groups, names and card destinations.
- [ ] 2.6 Behaviors: the additions match, the offer, Delete this line, Keep it remembered. This records task 3.3 of the archived `settings-cascade` change.
- [ ] 2.7 Tests: the ignored-row note, `space_rows`, sidebar destinations, the additions match and its Keep it memory.

## 3. Docs

- [ ] 3.1 `ui/mockups.html` approved, `design/README.md`, `docs/PRD.md`.
