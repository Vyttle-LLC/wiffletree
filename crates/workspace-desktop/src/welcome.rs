//! The first screen on a machine with no saved model setup: it leads to the Models page
//! instead of offering a project the host would refuse.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants};
use ui::{Spinner, hint, humanize, icon};

/// What fills the main area while no session can be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StartScreen {
    /// The first snapshot has not arrived.
    Opening,
    /// No setup is saved and detection has not answered for every provider.
    Checking,
    /// No setup is saved and at least one CLI is signed in.
    WelcomeWithCli,
    /// No setup is saved and no CLI is signed in.
    WelcomeWithoutCli,
    StartWithProject,
}

/// The host refuses a project until a setup is saved, so a store without one has no projects.
pub(super) fn start_screen(
    snapshot_loaded: bool,
    saved: &ModelSelection,
    detection: &Detection,
) -> StartScreen {
    if !snapshot_loaded {
        StartScreen::Opening
    } else if saved.is_saved() {
        StartScreen::StartWithProject
    } else if !PROVIDERS.iter().all(|p| detection.contains_key(p)) {
        StartScreen::Checking
    } else if detection.values().any(ProviderStatus::usable) {
        StartScreen::WelcomeWithCli
    } else {
        StartScreen::WelcomeWithoutCli
    }
}

/// New project is offered once a setup is saved. Before the first snapshot nothing is known,
/// so a saved store never opens with it withheld.
pub(super) fn offers_new_project(snapshot_loaded: bool, saved: &ModelSelection) -> bool {
    !snapshot_loaded || saved.is_saved()
}

/// "Claude · Max plan", or the provider alone when its plan is unknown.
fn signed_in_label(provider: Provider, status: &ProviderStatus) -> String {
    match &status.plan {
        Some(plan) => format!("{} · {} plan", provider.label(), humanize(plan)),
        None => provider.label().into(),
    }
}

fn step(number: usize, title: &'static str, next: bool, body: Option<Div>, p: Palette) -> Div {
    let color = if next { p.accent } else { p.subtle };
    div()
        .w_full()
        .px_3()
        .py(px(10.))
        .flex()
        .items_start()
        .gap(px(10.))
        .rounded_lg()
        .border_1()
        .border_color(if next { p.accent } else { p.edge.opacity(0.6) })
        .bg(p.surface)
        .when(!next, |d| d.opacity(0.55))
        .child(
            div()
                .flex_none()
                .size(px(20.))
                .mt(px(1.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(color)
                .text_color(color)
                .text_size(px(11.))
                .child(number.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .children(body),
        )
}

impl Workspace {
    pub(super) fn start_screen_kind(&self, cx: &App) -> StartScreen {
        let models = self.models.read(cx);
        start_screen(
            self.shared.index.is_some(),
            &models.saved_model_selection,
            &models.detection,
        )
    }

    /// Each provider as detection found it: signed in with its plan, or its one-line fix.
    fn detected_providers(detection: &Detection, p: Palette) -> Div {
        let mut lines = div().flex().flex_col().gap(px(2.));
        for provider in PROVIDERS {
            let Some(status) = detection.get(&provider) else {
                continue;
            };
            lines = lines.child(match models::welcome_fix_hint(provider, status) {
                None => {
                    hint(format!("● {}", signed_in_label(provider, status)), p).text_color(p.green)
                }
                Some(fix) => div()
                    .flex()
                    .flex_wrap()
                    .gap_x_1()
                    .child(hint(format!("○ {}", provider.label()), p).text_color(p.yellow))
                    .child(hint(format!("· {fix}"), p)),
            });
        }
        lines
    }

    /// The welcome's heading is drawn by the start screen; this is what follows it.
    pub(super) fn welcome_steps(&self, signed_in: bool, p: Palette, cx: &mut Context<Self>) -> Div {
        let project = "Start a project";
        let models = self.models.read(cx);
        let (detection, checking) = (models.detection.clone(), models.catalog_pending > 0);
        let content = div().w_full().flex().flex_col().items_start().gap_3();
        if signed_in {
            let models = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(Self::detected_providers(&detection, p))
                .child(hint(
                    "Review which models your agents may use, then Save.",
                    p,
                ));
            content
                .child(hint(
                    "Two steps and you are ready. We found your signed-in CLIs and prepared a suggested setup for you to review.",
                    p,
                ))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(step(1, "Set up your models", true, Some(models), p))
                        .child(step(
                            2,
                            project,
                            false,
                            Some(hint("One outcome: a feature, a bug, a migration.", p)),
                            p,
                        )),
                )
                .child(
                    button("review-setup")
                        .primary()
                        .label("Review suggested setup →")
                        .mt_1()
                        .on_click(cx.listener(|v, _, w, c| v.open_page(Page::Models, w, c))),
                )
        } else {
            content
                .child(hint(
                    "Wiffletree runs your agents through the Claude Code and Codex CLIs, using your own sign-in. You need at least one.",
                    p,
                ))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(step(
                            1,
                            "Sign in to a CLI",
                            true,
                            Some(Self::detected_providers(&detection, p)),
                            p,
                        ))
                        .child(step(2, "Set up your models", false, None, p))
                        .child(step(3, project, false, None, p)),
                )
                .child(
                    ui::icon_or_spinner(
                        button("check-again").primary(),
                        icon("refresh"),
                        Some("Check again"),
                        checking,
                        gpui_component::Size::Small,
                    )
                    .disabled(checking)
                        .mt_1()
                        .on_click(cx.listener(|v, _, w, c| {
                            v.models.update(c, |models, c| models.refresh_models(w, c))
                        })),
                )
        }
    }

    pub(super) fn checking_clis(p: Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_color(p.subtle)
            .child(Spinner::new().small())
            .child("Checking the CLIs on this machine…")
    }
}

#[cfg(test)]
mod tests {
    use super::{StartScreen, offers_new_project, signed_in_label, start_screen};
    use workspace_core::{Detection, ModelSelection, Provider, ProviderStatus};

    fn detected(statuses: &[(Provider, bool, bool)]) -> Detection {
        statuses
            .iter()
            .map(|&(provider, installed, signed_in)| {
                let status = ProviderStatus {
                    installed,
                    signed_in,
                    plan: None,
                };
                (provider, status)
            })
            .collect()
    }
    fn saved() -> ModelSelection {
        ModelSelection {
            revision: 1,
            ..Default::default()
        }
    }

    #[test]
    fn the_start_screen_follows_the_snapshot_the_saved_setup_and_detection() {
        let unsaved = ModelSelection::default();
        let both = detected(&[
            (Provider::Claude, true, true),
            (Provider::Codex, true, true),
        ]);
        let one = detected(&[
            (Provider::Claude, true, false),
            (Provider::Codex, true, true),
        ]);
        let none = detected(&[
            (Provider::Claude, true, false),
            (Provider::Codex, false, false),
        ]);
        let half = detected(&[(Provider::Claude, false, false)]);
        // Nothing is decided before the first snapshot, whatever detection says.
        assert_eq!(start_screen(false, &unsaved, &none), StartScreen::Opening);
        assert_eq!(start_screen(false, &saved(), &both), StartScreen::Opening);
        // The no-CLI welcome needs a result for every provider.
        assert_eq!(
            start_screen(true, &unsaved, &Detection::new()),
            StartScreen::Checking
        );
        assert_eq!(start_screen(true, &unsaved, &half), StartScreen::Checking);
        for found in [&both, &one] {
            assert_eq!(
                start_screen(true, &unsaved, found),
                StartScreen::WelcomeWithCli
            );
        }
        assert_eq!(
            start_screen(true, &unsaved, &none),
            StartScreen::WelcomeWithoutCli
        );
        // A saved setup never shows the welcome, detected or not.
        for detection in [&Detection::new(), &none, &both] {
            assert_eq!(
                start_screen(true, &saved(), detection),
                StartScreen::StartWithProject
            );
        }
    }

    #[test]
    fn new_project_is_offered_only_once_a_setup_is_saved() {
        let unsaved = ModelSelection::default();
        assert!(!offers_new_project(true, &unsaved));
        assert!(offers_new_project(true, &saved()));
        // A saved store's first frames, before its snapshot, do not withhold it.
        assert!(offers_new_project(false, &unsaved));
    }

    #[test]
    fn a_signed_in_provider_shows_its_plan_when_known() {
        let mut status = ProviderStatus {
            installed: true,
            signed_in: true,
            plan: Some("max".into()),
        };
        assert_eq!(
            signed_in_label(Provider::Claude, &status),
            "Claude · Max plan"
        );
        status.plan = None;
        assert_eq!(signed_in_label(Provider::Codex, &status), "Codex");
    }
}
