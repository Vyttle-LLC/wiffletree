## MODIFIED Requirements

### Requirement: Compile runs start on schedule, on demand and after a ruling
Brain compile SHALL be a built-in host job of the scheduler (see `scheduled-jobs`, "Built-in host jobs"), owned by its brain, with one job per brain registered on this Mac, and SHALL take its due times, catch-up, Run now and run records from that scheduler. Only on the compiler host SHALL it be due: each day at the config's compile time in the config's timezone. A brain whose config has no compile time SHALL never be due, and SHALL compile only on Run now or a merged ruling. After sleep, downtime or a quit that missed one or more due times, it SHALL start exactly one catch-up run. Due times SHALL be computed in the config's timezone, so a daylight-saving change never yields two runs for one date. The human SHALL be able to start a run with Run now on the Brains page or the brain's Status view, which SHALL say why when a run is in progress or a compile PR is open. The compiler host SHALL start a run when a fetched `main` adds an entry whose `state` begins `Ruling on conflict`.

#### Scenario: Asleep at 2:00
- **WHEN** Michael's Mac sleeps from 1:00 to 7:00 and the compile time is 02:00
- **THEN** one run starts on wake, and none is started twice for that date

#### Scenario: A ruling merges
- **WHEN** a ruling entry from Oscar's install merges on `main` of `sagehaven-docs`
- **THEN** Michael's Mac starts a compile run on its next fetch

#### Scenario: No compile time
- **WHEN** the config of `sagehaven-docs` names Michael's Mac as compiler and has no `compile`
- **THEN** no run is ever due, Run now and a merged ruling still start one, and the Brains page says so
