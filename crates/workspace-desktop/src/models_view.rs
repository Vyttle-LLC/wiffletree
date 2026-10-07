//! Workspace-wide role defaults: each role's default provider and approved Big/Small profiles.
use super::*;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    menu::{DropdownMenu, PopupMenuItem},
    tab::{Tab, TabBar},
};
use models::{PROVIDERS, ROLES};
use ui::{card, effort_label, hint, icon, segment};

/// Addresses one editable profile: a role's Big or Small profile for a provider.
#[derive(Clone, Copy)]
struct ProfileField {
    role_index: usize,
    provider: Provider,
    small: bool,
}

impl Workspace {
    fn role_defaults_dirty(&self) -> bool {
        self.role_defaults != self.saved_role_defaults
    }

    /// A dropdown that writes the chosen value into one field of a role's profile.
    fn profile_selector(
        &self,
        id: String,
        choices: Vec<(String, String)>,
        current: String,
        apply: fn(&mut ModelProfile, String),
        field: ProfileField,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let weak = cx.weak_entity();
        let label = choices
            .iter()
            .find(|(value, _)| *value == current)
            .map_or(current.clone(), |(_, label)| label.clone());
        button(SharedString::from(id))
            .outline()
            .w_full()
            .label(label)
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in &choices {
                    let value = value.clone();
                    let weak = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(value == current)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |view, cx| {
                                    if let Some(profile) =
                                        view.role_defaults.get_mut(field.role_index).and_then(|d| {
                                            models::profile_mut(d, field.provider, field.small)
                                        })
                                    {
                                        apply(profile, value.clone());
                                    }
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu.scrollable(true).max_h(px(320.))
            })
    }

    fn profile_row(
        &self,
        defaults: &RoleDefault,
        field: ProfileField,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let ProfileField {
            role_index,
            provider,
            small,
        } = field;
        let Some(profile) = models::profile(defaults, provider, small) else {
            return div();
        };
        let approved: Vec<&str> = self
            .snapshot
            .iter()
            .flat_map(|s| &s.policies)
            .filter(|policy| policy.role == defaults.role)
            .flat_map(|policy| &policy.allowed)
            .filter(|allowed| allowed.provider == provider)
            .map(|allowed| allowed.model.as_str())
            .chain([profile.model.as_str()])
            .collect();
        let models = models::choices(&self.model_catalog, provider, &approved)
            .into_iter()
            .map(|option| (option.model, option.label))
            .collect();
        let efforts = models::effort_choices(
            &self.model_catalog,
            provider,
            &profile.model,
            &profile.effort,
        )
        .into_iter()
        .map(|effort| (effort.clone(), effort_label(&effort)))
        .collect();
        let key = format!("{role_index}-{provider:?}-{small}");
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .w(px(32.))
                    .text_size(px(12.))
                    .text_color(p.subtle)
                    .child(if small { "Small" } else { "Big" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(self.profile_selector(
                        format!("profile-model-{key}"),
                        models,
                        profile.model.clone(),
                        |profile, model| profile.model = model,
                        field,
                        cx,
                    )),
            )
            .child(div().flex_none().w(px(84.)).child(self.profile_selector(
                format!("profile-effort-{key}"),
                efforts,
                profile.effort.clone(),
                |profile, effort| profile.effort = effort,
                field,
                cx,
            )))
    }

    fn role_card(
        &self,
        role_index: usize,
        label: &'static str,
        defaults: &RoleDefault,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let tab = self.profile_tabs[role_index];
        let mut default_provider = div().flex().gap_1();
        for provider in PROVIDERS {
            default_provider = default_provider.child(
                segment(
                    SharedString::from(format!("default-provider-{role_index}-{provider:?}")),
                    format!("{provider:?}"),
                    defaults.default_provider == provider,
                )
                .on_click(cx.listener(move |v, _, _, c| {
                    if let Some(defaults) = v.role_defaults.get_mut(role_index) {
                        defaults.default_provider = provider;
                    }
                    v.profile_tabs[role_index] = provider;
                    c.notify();
                })),
            );
        }
        let tabs = TabBar::new(SharedString::from(format!("profile-provider-{role_index}")))
            .w_full()
            .small()
            .underline()
            .selected_index(PROVIDERS.iter().position(|p| *p == tab).unwrap_or(0))
            .children(PROVIDERS.map(|provider| Tab::new().label(format!("{provider:?} profiles"))))
            .on_click(cx.listener(move |v, selection: &usize, _, c| {
                v.profile_tabs[role_index] = PROVIDERS[*selection];
                c.notify();
            }));
        card(p)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(hint("Default", p))
                            .child(default_provider),
                    ),
            )
            .child(tabs)
            .children([false, true].map(|small| {
                let field = ProfileField {
                    role_index,
                    provider: tab,
                    small,
                };
                self.profile_row(defaults, field, p, cx)
            }))
    }

    pub(super) fn models_panel(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_3()
            .child(hint(
                "Pick each role’s default provider and approve a Big and a Small profile per provider. New agents start on the default provider’s Big profile; coordinators may only choose saved profiles. Coordinator covers main and repository coordinators.",
                p,
            ));
        if !self.catalog_notice.is_empty() {
            body = body.child(hint(self.catalog_notice.clone(), p).text_color(p.yellow));
        }
        let mut roles = div().flex().flex_wrap().gap_3();
        for (index, (_, label)) in ROLES.into_iter().enumerate() {
            if let Some(defaults) = self.role_defaults.get(index) {
                roles = roles.child(
                    div()
                        .flex_1()
                        .min_w(px(340.))
                        .child(self.role_card(index, label, defaults, p, cx)),
                );
            }
        }
        body.child(roles)
    }

    pub(super) fn models_footer(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let dirty = self.role_defaults_dirty();
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
                hint(if dirty { "Unsaved changes" } else { "Saved" }, p)
                    .flex_1()
                    .min_w_0(),
            )
            .when(dirty, |d| {
                d.child(
                    button("discard-policy")
                        .ghost()
                        .label("Discard")
                        .on_click(cx.listener(|v, _, _, c| {
                            v.role_defaults = v.saved_role_defaults.clone();
                            c.notify();
                        })),
                )
            })
            .child(
                button("save-policy")
                    .primary()
                    .label("Save profiles")
                    .disabled(!dirty)
                    .on_click(cx.listener(|v, _, w, c| v.save_role_defaults(w, c))),
            )
    }
}
