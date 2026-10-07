//! Project memory: captured decisions and observations, and the attached Markdown brain.
use super::*;
use gpui_component::button::ButtonVariants;
use ui::{age, card, empty_state, hint, icon, mono, pill, section, segment};

impl Workspace {
    fn project_brain(&self) -> Option<String> {
        let project = self.project_id();
        self.snapshot
            .iter()
            .flat_map(|s| &s.projects)
            .find(|p| Some(&p.id) == project.as_ref())
            .and_then(|p| p.brain.clone())
    }

    fn choose_brain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the Markdown brain folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.first()
            {
                let path = path.to_string_lossy().into_owned();
                let _ = cx.update(|window, cx| {
                    this.update(cx, |view, cx| view.attach_brain(path, window, cx))
                });
            }
        })
        .detach();
    }

    fn attach_brain(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(project_id) = self.project_id()
            && !path.trim().is_empty()
        {
            self.request(Command::AttachBrain { project_id, path }, window, cx);
        }
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

    fn brain_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let brain = self.project_brain();
        let attached = brain.is_some();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("MARKDOWN BRAIN", p))
            .child(match brain {
                Some(path) => div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("folder").text_color(p.subtle))
                    .child(mono(path, p).flex_1()),
                None => hint(
                    "Attach a docs repository so memories are exported as ordinary Markdown.",
                    p,
                ),
            })
            .child(Input::new(&self.brain_path).w_full())
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("attach-brain")
                            .label(if attached { "Change" } else { "Attach" })
                            .on_click(cx.listener(|v, _, w, c| {
                                let path = v.brain_path.read(c).value().to_string();
                                v.attach_brain(path, w, c);
                            })),
                    )
                    .child(
                        button("choose-brain")
                            .ghost()
                            .icon(icon("folder"))
                            .label("Choose folder…")
                            .on_click(cx.listener(|v, _, w, c| v.choose_brain(w, c))),
                    ),
            )
            .when(attached, |d| {
                d.child(
                    div().flex().child(
                        button("export")
                            .outline()
                            .icon(icon("arrow-up"))
                            .label("Export pending memories")
                            .on_click(cx.listener(|v, _, w, c| {
                                if let Some(project_id) = v.project_id() {
                                    v.request(Command::ExportLogs { project_id }, w, c);
                                }
                            })),
                    ),
                )
            })
    }

    pub(super) fn memory_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut entries = div().flex().flex_col().gap_2().child(section(
            format!("SHARED ACROSS {}", self.project_name().to_uppercase()),
            p,
        ));
        match serde_json::from_value::<Vec<WorkLog>>(self.panel_data.clone()) {
            Err(_) => entries = entries.child(hint("Loading memory…", p)),
            Ok(logs) if logs.is_empty() => {
                entries = entries.child(empty_state(
                    "memory",
                    "Keep the decisions that matter",
                    "Agents and you can record decisions and observations so the next session starts with context.",
                    p,
                ));
            }
            Ok(logs) => {
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
                            .child(div().font_weight(FontWeight::MEDIUM).child(log.input.title))
                            .child(hint(log.input.changed, p)),
                    );
                }
            }
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
