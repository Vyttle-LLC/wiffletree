//! Project memory: the summary of the project's brain (OpenSpec change `brain-browser`), with
//! its sync, the project's held entries, the open conflicts and the newest entries in the
//! project's scope, which open the brain browser; the project's rows that are unsent or held,
//! with Fix and Release; and the capture form, which logs as the coordinator's `log_work` does.
use super::*;
use brain_browser_view::{BrainOpening, BrainTarget, BrainView};
use gpui_component::button::ButtonVariants;
use ui::{card, empty_state, hint, icon, mono, pill, section, segment};
use wording::age;

impl Inspector {
    /// The project's Space, which a project from a host older than Spaces has none of.
    fn project_space(&self) -> Option<&Space> {
        let project = self.shared.project_id()?;
        let index = self.shared.index.as_ref()?;
        let space = &index.project(&project)?.space;
        index.snapshot().spaces.iter().find(|s| &s.id == space)
    }

    fn capture_form(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut kinds = div().flex().gap_1();
        for (kind, label) in [
            (LogKind::Decision, "Decision"),
            (LogKind::Observation, "Observation"),
        ] {
            kinds = kinds.child(segment(label, label, self.memory_kind == kind).on_click(
                cx.listener(move |v, _, _, c| {
                    v.memory_kind = kind.clone();
                    c.notify();
                }),
            ));
        }
        let fixing = self.memory_fixing.as_ref();
        let mut form = card(p).child(
            section(
                if fixing.is_some() {
                    "FIX HELD ENTRY"
                } else {
                    "CAPTURE"
                },
                p,
            )
            .child(kinds),
        );
        if let Some(held) = fixing {
            form = form
                .child(hint(held_line(held), p))
                .child(Input::new(&self.memory_folder).w_full())
                .child(Input::new(&self.memory_ref).w_full());
        }
        let mut buttons = div().flex().gap_2();
        if fixing.is_some() {
            buttons = buttons.child(
                button("cancel-fix")
                    .ghost()
                    .label("Cancel")
                    .on_click(cx.listener(|v, _, w, c| v.clear_capture(w, c))),
            );
        }
        form.child(Input::new(&self.memory_title).w_full())
            .child(Textarea::new(&self.memory_body).w_full())
            .child(Input::new(&self.memory_why).w_full())
            .child(Input::new(&self.memory_state).w_full())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        hint(
                            if self.memory_kind == LogKind::Decision {
                                "A commitment later work must respect. One terse entry; never child personal information or secrets."
                            } else {
                                "A note about the current state. One terse entry; never child personal information or secrets."
                            },
                            p,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        buttons.child(
                            button("capture-memory")
                                .primary()
                                .label("Save")
                                .on_click(cx.listener(|v, _, w, c| v.capture_memory(w, c))),
                        ),
                    ),
            )
    }

    fn held_actions(&self, log: &WorkLog, cx: &mut Context<Self>) -> Div {
        let (fix, release) = (log.clone(), log.id.clone());
        div()
            .flex()
            .gap_2()
            .child(
                button(SharedString::from(format!("fix-{}", log.id)))
                    .outline()
                    .label("Fix")
                    .on_click(cx.listener(move |v, _, w, c| v.fix_held(fix.clone(), w, c))),
            )
            .child(
                button(SharedString::from(format!("release-{}", log.id)))
                    .ghost()
                    .label("Release")
                    .tooltip("Keeps it in this project's memory and never sends it to the brain.")
                    .on_click(cx.listener(move |v, _, w, c| {
                        let log_id = release.clone();
                        v.request(Command::ReleaseHeldLog { log_id }, w, c)
                    })),
            )
    }

    /// The repository of the project's brain, its Space's.
    fn project_brain(&self) -> Option<String> {
        Some(brain_browser_view::space_brain(self.project_space()?))
    }

    /// Reads what the summary shows of the project's brain: its sync, the open conflicts and the
    /// newest entries in the project's scope.
    pub(super) fn load_memory_summary(
        &mut self,
        project_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repository) = self.project_brain() else {
            return;
        };
        if ui::now() - self.memory_summary.asked_at < FRESH_MS {
            return;
        }
        self.memory_summary.asked_at = ui::now();
        let shown = move |view: &Self, project: &str| {
            view.shared.panel == Some(Panel::Memory)
                && view.shared.project_id().as_deref() == Some(project)
        };
        let project = project_id.clone();
        self.ask(ask::brains(), window, cx, move |view, read, _, _| {
            if shown(view, &project) {
                view.memory_summary.brains = Some(read);
            }
        });
        let project = project_id.clone();
        let conflicts = ask::brain_conflicts(repository.clone(), Some(project_id.clone()));
        self.ask(conflicts, window, cx, move |view, read, _, _| {
            if shown(view, &project) {
                view.memory_summary.conflicts = Some(read.map(|c| c.len()));
            }
        });
        let project = project_id.clone();
        let logs = ask::brain_logs(
            repository,
            Some(project_id),
            BrainLogFilter::default(),
            BrainLogsFrom::Newest,
        );
        self.ask(logs, window, cx, move |view, read, _, _| {
            if shown(view, &project) {
                view.memory_summary.recent = Some(read.map(newest_entries));
            }
        });
    }

    fn open_brain(&self, target: BrainTarget, cx: &mut Context<Self>) {
        if let Some(repository) = self.project_brain() {
            cx.emit(Intent::OpenBrain(BrainOpening {
                repository,
                project: self.shared.project_id(),
                target,
            }));
        }
    }

    /// The brain card: its name and sync, Open brain browser and Search.
    fn brain_card(&self, repository: &str, p: Palette, cx: &mut Context<Self>) -> Div {
        let listing = self.memory_summary.listing();
        let brain = listing.and_then(|l| {
            (l.brains.iter()).find(|b| b.repository.eq_ignore_ascii_case(repository))
        });
        let mut title = div()
            .flex()
            .items_center()
            .gap_2()
            .child(icon("brain").text_color(p.subtle))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(repository.to_owned()),
            );
        if let Some((sync, tone)) = brain.map(brains_view::sync_state) {
            title = title.child(pill(sync, tone.color(p)));
        }
        let space = self.project_space().map_or("", |s| s.name.as_str());
        let search = repository.to_owned();
        card(p)
            .child(title)
            .child(hint(
                format!("The {space} Space's brain, shared by its projects."),
                p,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button("open-brain-browser")
                            .primary()
                            .label("Open brain browser")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.open_brain(BrainTarget::View(BrainView::Status), cx)
                            })),
                    )
                    .child(
                        button("search-brain")
                            .ghost()
                            .icon(icon("search"))
                            .label("Search")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                cx.emit(Intent::SearchBrain {
                                    repository: search.clone(),
                                    project: view.shared.project_id(),
                                })
                            })),
                    ),
            )
    }

    /// The project's entries a scan held on this Mac, whose cards are in its Inbox.
    fn held_entries(&self, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let project = self.shared.project_id()?;
        let notes = self.shared.workspace_notes().iter();
        let held: Vec<&HeldEntry> = notes
            .filter_map(|note| match note {
                WorkspaceNote::HeldEntry(entry) => Some(entry),
                _ => None,
            })
            .filter(|entry| entry.project_id.as_deref() == Some(project.as_str()))
            .collect();
        if held.is_empty() {
            return None;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("HELD ON THIS MAC", p));
        for (at, entry) in held.into_iter().enumerate() {
            list = list.child(
                card(p)
                    .border_color(p.yellow.opacity(0.55))
                    .child(hint(held_entry::found_line(entry), p))
                    .child(mono(entry.path.clone(), p))
                    .child(
                        button(SharedString::from(format!("held-card-{at}")))
                            .link()
                            .label("Open its card")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(Intent::ShowPanel(Panel::Attention))
                            })),
                    ),
            );
        }
        Some(list)
    }

    fn recent_entries(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(section("RECENT ENTRIES", p));
        let entries = match &self.memory_summary.recent {
            None => return list.child(hint("Reading the brain…", p)),
            Some(Err(error)) => {
                return list.child(hint(format!("Could not read the brain: {error}"), p));
            }
            Some(Ok(entries)) => entries,
        };
        if entries.is_empty() {
            return list.child(hint("None yet.", p));
        }
        for entry in entries {
            let target = BrainTarget::Entry {
                id: entry.id.clone(),
                pending: entry.pending,
            };
            let mut meta = vec![
                entry.date.clone(),
                entry.kind.name().into(),
                entry.author.clone(),
            ];
            meta.push(if entry.pending {
                "pending".into()
            } else {
                entry.verification.state.name().into()
            });
            list = list.child(
                div()
                    .id(SharedString::from(format!("recent-{}", entry.id)))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|d| d.bg(p.overlay))
                    .on_click(
                        cx.listener(move |view, _, _, cx| view.open_brain(target.clone(), cx)),
                    )
                    .child(div().text_ellipsis().child(entry.title.clone()))
                    .child(hint(meta.join(" · "), p)),
            );
        }
        list
    }

    /// The project's own rows: every row while it has no indexed brain, otherwise only those
    /// not simply on their way to the brain, with Fix and Release for a held one.
    fn log_rows(&self, unsent_only: bool, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let mut entries = div().flex().flex_col().gap_2().child(section(
            if unsent_only {
                "UNSENT".to_owned()
            } else {
                format!(
                    "SHARED ACROSS {}",
                    self.shared.project_name().to_uppercase()
                )
            },
            p,
        ));
        let logs = match &self.panel_data {
            PanelData::Memory(logs) => logs,
            _ => return Some(entries.child(hint("Loading memory…", p))),
        };
        let shown: Vec<&WorkLog> = (logs.iter())
            .filter(|log| !unsent_only || log_status(log).is_some())
            .collect();
        if shown.is_empty() {
            return (!unsent_only).then(|| {
                entries.child(empty_state(
                    "memory",
                    "Keep the decisions that matter",
                    "Agents and you can record decisions and observations so the next session starts with context.",
                    p,
                ))
            });
        }
        for log in shown {
            let decision = log.input.kind == LogKind::Decision;
            let mut status = None;
            if let Some((line, warn)) = log_status(log) {
                let color = if warn { p.yellow } else { p.subtle };
                status = Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(hint(line, p).text_color(color).flex_1().min_w_0())
                        .children(log.held.is_some().then(|| self.held_actions(log, cx))),
                );
            }
            entries = entries.child(
                card(p)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(if decision {
                                pill("Decision", p.accent)
                            } else {
                                pill("Observation", p.focus)
                            })
                            .child(hint(
                                format!("{} · {}", log.repository, age(log.created_at)),
                                p,
                            )),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(log.input.title.clone()),
                    )
                    .child(hint(log.input.changed.clone(), p))
                    .children(status),
            );
        }
        Some(entries)
    }

    pub(super) fn memory_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let panel = div().flex().flex_col().gap_5();
        let Some(repository) = self.project_brain() else {
            return panel
                .children(self.log_rows(false, p, cx))
                .child(self.capture_form(p, cx));
        };
        let conflicts = (self.memory_summary.conflicts.as_ref())
            .and_then(|c| c.as_ref().ok().copied())
            .filter(|&n| n > 0)
            .map(|n| {
                button("memory-conflicts")
                    .link()
                    .label(format!(
                        "{n} {} waiting",
                        wording::plural(n, "conflict", "conflicts")
                    ))
                    .tooltip("Opens them in the brain browser's Conflicts")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.open_brain(BrainTarget::View(BrainView::Conflicts), cx)
                    }))
            });
        panel
            .child(self.brain_card(&repository, p, cx))
            .children(self.held_entries(p, cx))
            .children(conflicts)
            .child(self.recent_entries(p, cx))
            .children(self.log_rows(true, p, cx))
            .child(self.capture_form(p, cx))
    }
}

const RECENT: usize = 5;

/// The page's newest entries, this Mac's pending ones among those on main by id.
fn newest_entries(page: BrainLogPage) -> Vec<BrainEntry> {
    let mut entries: Vec<BrainEntry> = page.pending.into_iter().chain(page.entries).collect();
    entries.sort_by(|a, b| b.id.cmp(&a.id));
    entries.truncate(RECENT);
    entries
}
/// How long the summary's reading stands before a change to the workspace asks again.
const FRESH_MS: i64 = 10_000;

/// What the Memory panel read of the project's brain.
#[derive(Default)]
pub(super) struct MemorySummary {
    /// When the summary was last asked for; the panel resets it with the selection.
    asked_at: i64,
    brains: Option<Result<BrainsPage, String>>,
    conflicts: Option<Result<usize, String>>,
    recent: Option<Result<Vec<BrainEntry>, String>>,
}
impl MemorySummary {
    fn listing(&self) -> Option<&BrainsPage> {
        self.brains.as_ref()?.as_ref().ok()
    }
}

/// Where an entry stands when it is not simply on its way to the brain, and whether that needs
/// the human.
fn log_status(log: &WorkLog) -> Option<(String, bool)> {
    if let Some(rule) = &log.held {
        return Some((
            format!("Held, not sent: {rule}. Fix it, or release it to keep it here only."),
            true,
        ));
    }
    if log.released {
        return Some(("Released: kept here, never sent.".into(), false));
    }
    if log.in_legacy_file {
        return Some((
            "In a legacy log file; the legacy import brings it over.".into(),
            false,
        ));
    }
    log.is_unsent()
        .then(|| ("Unsent: it lands with the next sync.".into(), false))
}

/// What the fix form says about the held entry it is fixing.
fn held_line(held: &WorkLog) -> String {
    let rule = held
        .held
        .as_deref()
        .unwrap_or("it breaks the entry contract");
    format!(
        "Held because {rule}. Edit it so it fits; the folder and ref must be ones the entry contract allows."
    )
}

#[cfg(test)]
mod tests {
    use super::{RECENT, log_status, newest_entries};
    use workspace_core::*;

    #[test]
    fn recent_entries_are_the_newest_pending_or_on_main() {
        let entry = |id: &str, pending: bool| BrainEntry {
            id: id.into(),
            path: format!("logs/{id}.md"),
            repository: "app".into(),
            author: "mhohlios".into(),
            source: "wiffletree".into(),
            date: "2026-10-31".into(),
            title: id.into(),
            kind: BrainKind::Decision,
            reference: String::new(),
            changed: String::new(),
            why: String::new(),
            state: String::new(),
            anchors: RefAnchors::default(),
            verification: RefVerification {
                state: RefState::Unverified,
                reason: None,
            },
            pull_request: EntryPullRequest::None,
            origin: None,
            pending,
        };
        let page = BrainLogPage {
            entries: ["09", "07", "06", "05", "04", "03"]
                .map(|id| entry(id, false))
                .into(),
            pending: ["08", "01"].map(|id| entry(id, true)).into(),
            ..BrainLogPage::default()
        };
        let ids: Vec<String> = newest_entries(page).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["09", "08", "07", "06", "05"]);
        assert_eq!(ids.len(), RECENT);
    }

    #[test]
    fn an_entry_says_when_it_is_held_released_or_waiting_to_land() {
        let mut log: WorkLog = serde_json::from_value(serde_json::json!({
            "sequence": 1, "id": "01j", "project_id": "p", "task_id": null, "repository": "app",
            "created_at": 0, "input": {"session_id": "s", "kind": "decision", "title": "T",
                "changed": "c", "why": "w", "state": "s"}
        }))
        .unwrap();
        let (line, warn) = log_status(&log).unwrap();
        assert!(line.starts_with("Unsent") && !warn);
        log.written = true;
        assert_eq!(log_status(&log), None, "in its Space's folder");
        log.held = Some("its ref is not one the entry contract allows".into());
        let (line, warn) = log_status(&log).unwrap();
        assert!(line.contains("its ref is not one") && line.contains("release") && warn);
        log.held = None;
        log.released = true;
        assert!(!log_status(&log).unwrap().1);
    }
}
