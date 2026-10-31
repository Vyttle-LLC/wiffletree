//! Project memory: captured decisions and observations, and its Space's Markdown brain.
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
        card(p)
            .child(section("CAPTURE", p).child(kinds))
            .child(Input::new(&self.memory_title).w_full())
            .child(Textarea::new(&self.memory_body).w_full())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        hint(
                            if self.memory_kind == LogKind::Decision {
                                "A commitment later work must respect."
                            } else {
                                "A note about the current state."
                            },
                            p,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        button("capture-memory")
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|v, _, w, c| v.capture_memory(w, c))),
                    ),
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
                            .child(hint(log.input.changed.clone(), p)),
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
    use super::{brain_name, no_brain_line};
    use workspace_core::*;

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
