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
    /// A fresh store waits for its first suggestion before anything can change.
    fn models_editable(&self) -> bool {
        models::editable(&self.saved_model_selection, self.suggested.as_ref())
    }

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

    fn suggestion_untouched(&self, suggested: &Setup, cx: &App) -> bool {
        models::untouched(
            suggested,
            &self.model_selection,
            self.verifiers.read(cx).verification(),
        )
    }

    fn show_suggestion(&mut self, setup: &Setup, window: &mut Window, cx: &mut Context<Self>) {
        self.model_selection = setup.selection.clone();
        self.guide_input.update(cx, |input, cx| {
            input.set_value(setup.selection.guide.clone(), window, cx)
        });
        self.verifiers.update(cx, |editor, cx| {
            editor.hold_draft(setup.verification.clone(), cx)
        });
    }

    /// On a store with no saved setup, suggests one from the last detection. An untouched
    /// suggestion follows a new detection; one the human edited is kept.
    pub(super) fn offer_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.is_none() || self.catalog_pending > 0 {
            return;
        }
        let Some(offer) = models::offer(
            &self.saved_model_selection,
            self.snapshot.as_ref().is_some_and(|s| s.guide_saved),
            self.suggested.as_ref(),
            &self.model_selection,
            self.verifiers.read(cx).verification(),
            &self.detection,
        ) else {
            return;
        };
        if offer.replaces_draft {
            self.show_suggestion(&offer.suggested, window, cx);
        }
        self.suggested = Some(offer.suggested);
    }

    /// The first-run notice: what was suggested, or why nothing could be.
    fn suggestion_banner(&self, suggested: &Setup, p: Palette) -> Div {
        let found = suggested.selection.any_enabled();
        let (color, title, note) = if found {
            (
                p.accent,
                "Suggested setup · not saved yet",
                "We filled this in from the CLIs signed in on this machine. Change anything you like, then Save. Agents cannot start until a setup is saved.",
            )
        } else {
            (
                p.yellow,
                "No signed-in CLI found",
                "Install Claude Code or Codex and sign in, then press Refresh. Wiffletree uses your existing CLI sign-in and needs at least one provider.",
            )
        };
        div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(color)
            .bg(color.opacity(0.08))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(hint(note, p))
    }

    pub(super) fn card_title(title: &'static str, note: &'static str, p: Palette) -> Div {
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
        let detected = self.detection.get(&provider);
        let status = match detected {
            Some(found) => {
                let label = models::status_label(found);
                match (found.installed, found.signed_in) {
                    (true, true) => hint(format!("● {label}"), p).text_color(p.green),
                    (true, false) => hint(format!("● {label}"), p).text_color(p.yellow),
                    (false, _) => hint(format!("○ {label}"), p),
                }
            }
            None => hint("Checking CLI…", p),
        };
        // A suggested setup marks what it enabled and says how to add what it left out.
        let suggested = models::tagged_suggested(self.suggested.as_ref(), selection, provider);
        let editable = self.models_editable();
        let fix = self
            .suggested
            .as_ref()
            .and(detected)
            .and_then(|found| models::fix_hint(provider, found));
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
                    .disabled(!enabled || !editable)
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
                            .disabled(!enabled || !editable)
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
                .disabled(!enabled || !editable || unlisted.is_empty())
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
            .disabled(!enabled || !editable || added.is_none())
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
                            .disabled(!editable)
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
                    .when(suggested, |d| {
                        d.child(
                            div()
                                .px_2()
                                .rounded_full()
                                .border_1()
                                .border_color(p.accent)
                                .text_xs()
                                .text_color(p.accent)
                                .child("Suggested"),
                        )
                    })
                    .when(!enabled && fix.is_none(), |d| {
                        d.child(hint("Disabled: coordinators may not choose its models.", p))
                    })
                    .children(fix.map(|fix| hint(fix, p))),
            )
            // A first-run page lists models only under the providers in use.
            .when(enabled || self.suggested.is_none(), |d| d.child(rows))
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
            let enabled = self.model_selection.is_enabled(provider) && self.models_editable();
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
                "who reviews a ticket, and how thoroughly",
                p,
            ))
            .child(if !self.models_editable() {
                hint("Available once the CLIs on this machine are checked.", p).into_any_element()
            } else if self.suggested.is_some() && !self.model_selection.any_enabled() {
                hint("Available once a provider is enabled.", p).into_any_element()
            } else {
                self.verifiers.clone().into_any_element()
            })
    }

    /// One setting: the PR keeper's model, from the models allowed on this machine, or none.
    fn keeper_card(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let chosen = self.model_selection.keeper.clone();
        let choices = models::keeper_choices(&self.model_selection);
        let catalog = self.model_catalog.clone();
        let label = chosen
            .as_ref()
            .map_or("No model chosen".to_owned(), |profile| {
                models::profile_label(&catalog, profile)
            });
        let weak = cx.weak_entity();
        let picker = button("keeper-model")
            .small()
            .label(label)
            .dropdown_caret(true)
            .disabled(!self.models_editable() || choices.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                let options = choices.iter().cloned().map(Some).chain([None]);
                for option in options {
                    let label = option.as_ref().map_or("No model".to_owned(), |profile| {
                        models::profile_label(&catalog, profile)
                    });
                    let checked = option == chosen;
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        move |_, _, c| {
                            let _ = weak.update(c, |v, c| {
                                v.model_selection.keeper = option.clone();
                                c.notify();
                            });
                        },
                    ));
                }
                menu
            });
        let fact = |term: &'static str, value: Div| {
            div()
                .flex()
                .gap_3()
                .text_size(px(12.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_none()
                        .text_color(p.subtle)
                        .child(term),
                )
                .child(value.flex_1().min_w_0())
        };
        card(p)
            .gap_3()
            .child(Self::card_title(
                "PR keeper",
                "the small model that keeps pull requests current",
                p,
            ))
            .child(fact(
                "Model",
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(picker)
                    .child(hint("one of the models allowed on this machine", p)),
            ))
            .child(fact(
                "Used for",
                div().child("One try at a conflict or a failed check, sorting new comments, and merging the passages of a skill update that you also changed in your rewrite. Watching pull requests and rebasing use no model."),
            ))
            .child(fact(
                "Reviews",
                div().child("A fix it makes is reviewed at the level chosen in Repositories, by the models the coordinator last chose for those verifiers on that ticket."),
            ))
            .child(hint(
                "Sorting comments and merging skill updates need a Claude model: they run with every tool turned off, and Codex cannot be started that way.",
                p,
            ))
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
            .child(
                Textarea::new(&self.guide_input)
                    .w_full()
                    .disabled(!self.models_editable()),
            )
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
            .children(
                self.suggested
                    .as_ref()
                    .map(|suggested| self.suggestion_banner(suggested, p)),
            )
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
            .child(self.keeper_card(p, cx))
            .child(self.guide_card(p))
    }

    pub(super) fn models_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        // A suggested setup is unsaved from the start: Save needs no edit first.
        let untouched = self
            .suggested
            .as_ref()
            .map(|suggested| self.suggestion_untouched(suggested, cx));
        let dirty = untouched.is_some() || self.model_selection_dirty();
        let invalid = self.model_selection.validate().err();
        let loading = self.catalog_pending > 0;
        let no_provider = !self.model_selection.any_enabled();
        let can_save = models::can_save(
            self.models_editable(),
            untouched.is_some(),
            self.model_selection_dirty(),
            &self.model_selection,
        );
        let state = match (&invalid, untouched) {
            (Some(_), Some(_)) if no_provider => {
                hint("Enable at least one provider to save", p).text_color(p.red)
            }
            (Some(error), _) if dirty => hint(error.to_string(), p).text_color(p.red),
            (None, Some(true)) => hint("Suggested setup · not saved yet", p),
            _ if dirty => hint("Unsaved changes", p),
            _ if loading && !self.saved_model_selection.is_saved() => {
                hint("Checking the CLIs on this machine…", p)
            }
            _ => hint("Saved", p),
        };
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
            .child(state.flex_1().min_w_0())
            .when(dirty && untouched != Some(true), |d| {
                d.child(
                    button("discard-models")
                        .ghost()
                        .label("Discard")
                        .on_click(cx.listener(|v, _, w, c| {
                            // Before the first save there is only the suggestion to go back to.
                            if let Some(suggested) = v.suggested.clone() {
                                v.show_suggestion(&suggested, w, c);
                            } else {
                                let saved = v.saved_model_selection.clone();
                                v.model_selection = v.saved_model_selection.clone();
                                v.sync_model_selection(&saved, w, c);
                            }
                            c.notify();
                        })),
                )
            })
            .child(
                button("save-models")
                    .primary()
                    .label("Save")
                    .disabled(!can_save)
                    .on_click(cx.listener(|v, _, w, c| v.save_model_selection(w, c))),
            )
    }
}
