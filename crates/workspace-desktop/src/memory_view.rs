//! Project memory: the capture form, which logs as the coordinator's `log_work` does, the
//! project's entries with any that are unsent or held, and its Space's brain.
use super::*;
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

    /// Where the project's Space keeps its brain on this Mac, a local folder or a GitHub brain's
    /// clone.
    fn local_brain_path(&self, space: &Space) -> Option<String> {
        let index = self.shared.index.as_ref()?;
        index
            .snapshot()
            .space_locals
            .get(&space.id)?
            .brain_path
            .clone()
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

    /// A held entry's Fix and Release.
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

    /// The brain of the project's Space, chosen in that Space's settings, or "No brain" with the
    /// way to name one. The brain itself opens from the Brains page.
    fn brain_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let space = self.project_space();
        let brain = space.and_then(|s| s.brain.as_ref());
        let local = space
            .filter(|_| brain.is_some())
            .and_then(|s| self.local_brain_path(s));
        let mut links = div().flex().flex_wrap().gap_2().child(
            button("open-brains")
                .outline()
                .icon(icon("brain"))
                .label("Open Brains")
                .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Brains)))),
        );
        if let Some(space) = space.filter(|_| brain.is_none()) {
            let id = space.id.clone();
            links = links.child(
                button("open-space-settings")
                    .ghost()
                    .label("Space settings")
                    .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::OpenSpace(id.clone())))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("BRAIN", p))
            .child(match brain {
                None => card(p)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("brain").text_color(p.subtle))
                            .child(div().font_weight(FontWeight::MEDIUM).child("No brain")),
                    )
                    .child(hint(no_brain_line(space.map(|s| s.name.as_str())), p)),
                Some(brain) => div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(hint(
                        format!(
                            "The {} Space's brain, shared by its projects.",
                            space.map_or("", |s| s.name.as_str())
                        ),
                        p,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("brain").text_color(p.subtle))
                            .child(mono(brain_name(brain, local.as_deref()), p).flex_1()),
                    ),
            })
            .child(links)
    }

    pub(super) fn memory_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut entries = div().flex().flex_col().gap_2().child(section(
            format!(
                "SHARED ACROSS {}",
                self.shared.project_name().to_uppercase()
            ),
            p,
        ));
        match &self.panel_data {
            PanelData::Memory(logs) if logs.is_empty() => {
                entries = entries.child(empty_state(
                    "memory",
                    "Keep the decisions that matter",
                    "Agents and you can record decisions and observations so the next session starts with context.",
                    p,
                ));
            }
            PanelData::Memory(logs) => {
                for log in logs {
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
            }
            _ => entries = entries.child(hint("Loading memory…", p)),
        }
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.capture_form(p, cx))
            .child(entries)
            .child(self.brain_section(p, cx))
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
    log.is_unsent().then(|| {
        (
            "Unsent: it waits for this project's Space to name a brain.".into(),
            true,
        )
    })
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

/// Why a project has no brain, and where one is named.
fn no_brain_line(space: Option<&str>) -> String {
    match space {
        Some(space) => format!(
            "The {space} Space names no brain, so this project has none. Name one in the Space's settings; the Brains page lists the brains your Spaces name."
        ),
        None => "This project is in no Space, so it has no brain.".into(),
    }
}

/// The brain as the panel names it: its repository, or where a local brain is on this Mac.
fn brain_name(brain: &SpaceBrain, local: Option<&str>) -> String {
    match &brain.repository {
        BrainRepository::GitHub { owner, name, .. } => format!("{owner}/{name}"),
        BrainRepository::Local { .. } => local.unwrap_or("A folder not found on this Mac").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{brain_name, log_status, no_brain_line};
    use workspace_core::*;

    #[test]
    fn an_entry_says_when_it_is_held_released_or_waiting_for_a_brain() {
        let mut log: WorkLog = serde_json::from_value(serde_json::json!({
            "sequence": 1, "id": "01j", "project_id": "p", "task_id": null, "repository": "app",
            "created_at": 0, "input": {"session_id": "s", "kind": "decision", "title": "T",
                "changed": "c", "why": "w", "state": "s"}
        }))
        .unwrap();
        let (line, warn) = log_status(&log).unwrap();
        assert!(line.starts_with("Unsent") && warn);
        log.batch_id = Some("b".into());
        assert_eq!(log_status(&log), None, "on its way to the brain");
        log.held = Some("its ref is not one the entry contract allows".into());
        let (line, warn) = log_status(&log).unwrap();
        assert!(line.contains("its ref is not one") && line.contains("release") && warn);
        log.held = None;
        log.released = true;
        assert!(!log_status(&log).unwrap().1);
    }

    #[test]
    fn a_project_without_a_brain_says_so_and_where_one_is_named() {
        let line = no_brain_line(Some("Vyttle"));
        assert!(line.contains("Vyttle Space names no brain") && line.contains("Space's settings"));
        assert!(line.contains("Brains page"));
        assert!(no_brain_line(None).contains("no brain"));
    }

    #[test]
    fn a_github_brain_is_named_by_its_repository_and_a_local_one_by_its_folder() {
        let brain = |repository| SpaceBrain {
            repository,
            folder: "second-brain".into(),
            extra: Extra::new(),
        };
        let github = brain(BrainRepository::GitHub {
            owner: "Sage-Social".into(),
            name: "sagehaven-docs".into(),
            extra: Extra::new(),
        });
        assert_eq!(
            brain_name(&github, Some("/elsewhere")),
            "Sage-Social/sagehaven-docs"
        );
        let local = brain(BrainRepository::local());
        assert_eq!(brain_name(&local, Some("/Users/m/notes")), "/Users/m/notes");
    }
}
