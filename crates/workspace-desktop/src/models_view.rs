//! The machine's model selection: providers and allowed models, providers per role, the
//! verifiers and the guide coordinators read.
use super::*;
use gpui_component::{
    Disableable, Sizable,
    button::ButtonVariants,
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
    switch::Switch,
};
use ui::{card, effort_label, hint, icon};

impl Workspace {
    fn model_selection_dirty(&self) -> bool {
        self.model_selection != self.saved_model_selection
    }

    /// Unsaved edits survive refreshes; an untouched editor follows the saved selection.
    pub(super) fn sync_model_selection(
        &mut self,
        saved: &ModelSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_selection == self.saved_model_selection {
            self.model_selection = saved.clone();
            self.guide_input.update(cx, |input, cx| {
                input.set_value(saved.guide.clone(), window, cx)
            });
        }
        self.saved_model_selection = saved.clone();
    }

    fn card_title(title: &'static str, note: &'static str, p: Palette) -> Div {
        div()
            .flex()
            .items_baseline()
            .gap_2()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(hint(note, p))
    }

    fn provider_section(&self, provider: Provider, p: Palette, cx: &mut Context<Self>) -> Div {
        let selection = &self.model_selection;
        let enabled = selection.is_enabled(provider);
        let status = match self.cli_found.get(&provider) {
            Some(true) => hint("● CLI found", p).text_color(p.green),
            Some(false) => hint("○ CLI not found", p),
            None => hint("Checking CLI…", p),
        };
        let rows = models::model_rows(&self.model_catalog, provider, selection);
        let allowed = models::allowed_models(selection, provider);
        let row_of = |model: &str| rows.iter().find(|r| r.model == model);
        let mut list = div().flex().flex_col().gap_1();
        for (model, ticked) in &allowed {
            let mut efforts = div().flex().flex_wrap().gap_3();
            let supported = row_of(model).map_or(ticked.clone(), |r| r.efforts.clone());
            for effort in supported {
                let (model, value) = (model.clone(), effort.clone());
                efforts = efforts.child(
                    Checkbox::new(SharedString::from(format!(
                        "allow-{provider:?}-{model}-{effort}"
                    )))
                    .label(effort_label(&effort))
                    .checked(ticked.contains(&effort))
                    .disabled(!enabled)
                    .on_click(cx.listener(move |v, _: &bool, _, c| {
                        models::toggle_effort(&mut v.model_selection, provider, &model, &value);
                        c.notify();
                    })),
                );
            }
            let removed = model.clone();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(150.))
                            .flex_none()
                            .text_ellipsis()
                            .child(row_of(model).map_or(model.clone(), |r| r.label.clone())),
                    )
                    .child(efforts.flex_1().min_w_0())
                    .child(
                        button(SharedString::from(format!("remove-{provider:?}-{model}")))
                            .ghost()
                            .xsmall()
                            .label("Remove")
                            .disabled(!enabled)
                            .on_click(cx.listener(move |v, _, _, c| {
                                models::remove_model(&mut v.model_selection, provider, &removed);
                                c.notify();
                            })),
                    ),
            );
        }
        // Listed models are left out, so a model is never added twice.
        let unlisted: Vec<_> = rows
            .iter()
            .filter(|r| !allowed.iter().any(|(model, _)| *model == r.model))
            .map(|r| (r.model.clone(), r.label.clone()))
            .collect();
        let draft = self
            .model_drafts
            .get(&provider)
            .filter(|m| unlisted.iter().any(|(model, _)| model == *m))
            .cloned()
            .or_else(|| unlisted.first().map(|(model, _)| model.clone()));
        let weak = cx.weak_entity();
        let model_menu = {
            let current = draft.clone();
            button(SharedString::from(format!("draft-model-{provider:?}")))
                .small()
                .disabled(!enabled || unlisted.is_empty())
                .label(
                    draft
                        .as_ref()
                        .and_then(|d| row_of(d))
                        .map_or("Every model is listed".to_owned(), |r| r.label.clone()),
                )
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for (model, label) in &unlisted {
                        let weak = weak.clone();
                        let model = model.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label.clone())
                                .checked(current.as_deref() == Some(model.as_str()))
                                .on_click(move |_, _, c| {
                                    let _ = weak.update(c, |v, c| {
                                        v.model_drafts.insert(provider, model.clone());
                                        c.notify();
                                    });
                                }),
                        );
                    }
                    menu.scrollable(true).max_h(px(320.))
                })
        };
        let added = draft.and_then(|d| row_of(&d)).cloned();
        let add = button(SharedString::from(format!("add-model-{provider:?}")))
            .small()
            .icon(icon("plus"))
            .label("Add")
            .disabled(!enabled || added.is_none())
            .on_click(cx.listener(move |v, _, _, c| {
                if let Some(row) = &added {
                    models::add_model(&mut v.model_selection, provider, row);
                }
                c.notify();
            }));
        let rows = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(model_menu)
                    .child(add),
            )
            .child(list);
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(0.4))
            .bg(p.surface)
            .when(!enabled, |d| d.opacity(0.6))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Switch::new(SharedString::from(format!("enable-{provider:?}")))
                            .checked(enabled)
                            .on_click(cx.listener(move |v, on: &bool, _, c| {
                                models::set_enabled(&mut v.model_selection, provider, *on);
                                c.notify();
                            })),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(provider.label()),
                    )
                    .child(status)
                    .when(!enabled, |d| {
                        d.child(hint("Disabled: coordinators may not choose its models.", p))
                    }),
            )
            .child(rows)
    }

    fn provider_ticks(
        &self,
        id: &str,
        checked: impl Fn(Provider) -> bool,
        toggle: impl Fn(&mut ModelSelection, Provider) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut ticks = div().flex().gap_4();
        for provider in PROVIDERS {
            let enabled = self.model_selection.is_enabled(provider);
            let toggle = toggle.clone();
            ticks = ticks.child(
                Checkbox::new(SharedString::from(format!("{id}-{provider:?}")))
                    .label(provider.label())
                    .checked(checked(provider))
                    .disabled(!enabled)
                    .on_click(cx.listener(move |v, _: &bool, _, c| {
                        toggle(&mut v.model_selection, provider);
                        c.notify();
                    })),
            );
        }
        ticks
    }

    fn roles_card(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut rows = div().flex().flex_col().gap_2();
        for role in PROVIDER_ROLES {
            let providers = self
                .model_selection
                .role_providers
                .get(&role)
                .cloned()
                .unwrap_or_default();
            let ticks = self.provider_ticks(
                &format!("role-{role:?}"),
                |provider| providers.contains(&provider),
                move |selection, provider| models::toggle_role_provider(selection, role, provider),
                cx,
            );
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w(px(180.)).flex_none().child(role.label()))
                    .child(ticks)
                    .when(providers.is_empty(), |d| {
                        d.child(hint("Choose an enabled provider", p).text_color(p.red))
                    }),
            );
        }
        card(p)
            .gap_3()
            .child(Self::card_title(
                "Roles",
                "which providers coordinators may assign to each role",
                p,
            ))
            .child(rows)
    }

    fn review_card(&self, p: Palette) -> Div {
        card(p)
            .gap_3()
            .child(Self::card_title(
                "Review",
                "who verifies each ticket when the coordinator calls verify_ticket",
                p,
            ))
            .child(self.verifiers.clone())
    }

    fn guide_card(&self, p: Palette) -> Div {
        let bytes = self.model_selection.guide.len();
        card(p)
            .gap_3()
            .child(Self::card_title(
                "Guide",
                "how coordinators pick the model and effort within a provider",
                p,
            ))
            .child(Textarea::new(&self.guide_input).w_full())
            .child(
                hint(
                    format!(
                        "{:.1} / {} KB",
                        bytes as f32 / 1024.,
                        MAX_GUIDE_BYTES / 1024
                    ),
                    p,
                )
                .text_right()
                .when(bytes > MAX_GUIDE_BYTES, |d| d.text_color(p.red)),
            )
    }

    pub(super) fn models_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(hint(
                "Settings apply to this machine only. Coordinators may only use enabled providers, the providers set for each role, and allowed models. Your own choices are never limited.",
                p,
            ));
        if !self.catalog_notice.is_empty() {
            body = body.child(hint(self.catalog_notice.clone(), p).text_color(p.yellow));
        }
        let mut providers = card(p).gap_3().child(Self::card_title(
            "Providers",
            "the subscriptions on this machine",
            p,
        ));
        for provider in PROVIDERS {
            providers = providers.child(self.provider_section(provider, p, cx));
        }
        providers = providers.child(hint(
            "Model lists come from the installed CLIs, plus saved models. Removing a model affects new assignments only. Disabling a provider holds its coordinator-chosen agents at their next turn and tells their coordinator.",
            p,
        ));
        body.child(providers)
            .child(self.roles_card(p, cx))
            .child(self.review_card(p))
            .child(self.guide_card(p))
    }

    pub(super) fn models_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dirty = self.model_selection_dirty();
        let invalid = self.model_selection.validate().err();
        let loading = self.catalog_pending > 0;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                button("refresh-models")
                    .ghost()
                    .icon(icon("refresh"))
                    .loading(loading)
                    .disabled(loading)
                    .tooltip("Refresh model lists from the installed CLIs")
                    .on_click(cx.listener(|v, _, w, c| v.refresh_models(w, c))),
            )
            .child(
                match (&invalid, dirty) {
                    (Some(error), true) => hint(error.to_string(), p).text_color(p.red),
                    (_, true) => hint("Unsaved changes", p),
                    _ => hint("Saved", p),
                }
                .flex_1()
                .min_w_0(),
            )
            .when(dirty, |d| {
                d.child(
                    button("discard-models")
                        .ghost()
                        .label("Discard")
                        .on_click(cx.listener(|v, _, w, c| {
                            let saved = v.saved_model_selection.clone();
                            v.model_selection = v.saved_model_selection.clone();
                            v.sync_model_selection(&saved, w, c);
                            c.notify();
                        })),
                )
            })
            .child(
                button("save-models")
                    .primary()
                    .label("Save")
                    .disabled(!dirty || invalid.is_some())
                    .on_click(cx.listener(|v, _, w, c| v.save_model_selection(w, c))),
            )
    }
}
