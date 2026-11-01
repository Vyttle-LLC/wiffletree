//! A brain conflict's card (OpenSpec change `brain-compile`), as a project's Inbox and the
//! Brains page show it: what each side says, the evidence, the recommended ruling and the four
//! rulings, or who ruled once someone did. Brain text is shown as plain text.
use super::*;
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputState};
use ui::{card, hint, pill};

/// What the human did on a card.
pub(super) enum ConflictAction {
    Rule(Ruling),
    /// Opens Keep both's line, or with `false` closes it.
    KeepBoth(bool),
    /// Opens the brain browser's Conflicts view.
    Evidence,
}

pub(super) type OnConflict = Rc<dyn Fn(ConflictAction, &mut Window, &mut App)>;

/// The card's key in a view's state: its brain and conflict id.
pub(super) fn key(card: &ConflictCard) -> String {
    format!("{}#{}", card.repository, card.conflict.id)
}

fn side(side: &ConflictSide, p: Palette) -> Div {
    div()
        .flex_1()
        .min_w(px(180.))
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(p.edge)
        .bg(p.base)
        .flex()
        .flex_col()
        .gap_1()
        .child(hint(side.label.clone(), p).text_size(px(font::CAPTION)))
        .child(
            div()
                .text_size(px(font::TEXT))
                .line_height(relative(1.5))
                .child(side.text.clone()),
        )
        .child(hint(side.detail.clone(), p).text_size(px(font::CAPTION)))
}

/// The card. `scope` is Keep both's one-line field, shown when `keeping_both`.
pub(super) fn conflict_card(
    card_data: &ConflictCard,
    keeping_both: bool,
    scope: &Entity<InputState>,
    on: OnConflict,
    p: Palette,
) -> Div {
    let conflict = &card_data.conflict;
    let rule = match conflict.rule.as_deref() {
        Some("decision-contradiction") => "Contradicts a decision",
        Some("agent-disagreement") => "Two authors disagree",
        Some("destructive") => "Destructive change",
        _ => "Conflict",
    };
    let id = key(card_data);
    let header = div()
        .flex()
        .items_center()
        .gap_2()
        .child(match &card_data.state {
            ConflictState::Open => pill("Brain conflict", p.magenta),
            ConflictState::Ruled { .. } => pill("Ruled", p.green),
        })
        .child(div().font_weight(FontWeight::SEMIBOLD).child(rule))
        .child(div().flex_1())
        .child(hint(
            format!("{} · {}", card_data.repository, conflict.id),
            p,
        ));
    let mut body = card(p)
        .bg(p.surface)
        .border_color(match &card_data.state {
            ConflictState::Open => p.magenta.opacity(0.4),
            ConflictState::Ruled { .. } => p.edge,
        })
        .child(header)
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(conflict.title.clone()),
        );
    if let ConflictState::Ruled {
        ruling,
        by,
        pull_request,
        merged,
    } = &card_data.state
    {
        let pr = pull_request.map_or(String::new(), |n| format!(" in PR #{n}"));
        let status = match merged {
            true => "The ruling is on main; the compiler compiles it next.".to_owned(),
            false => format!(
                "Its decision entry is on its way{pr}. Compile starts when the ruling merges."
            ),
        };
        return body
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .child(format!("{ruling} by {by}.")),
            )
            .child(hint(status, p));
    }
    let sides = [&card_data.existing, &card_data.claim]
        .into_iter()
        .flatten()
        .map(|s| side(s, p));
    body = body.child(div().flex().flex_wrap().gap_2().children(sides));
    if !conflict.evidence.is_empty() {
        body = body.child(hint(format!("Evidence: {}", conflict.evidence), p));
    }
    if !conflict.recommend.is_empty() {
        body = body.child(
            div()
                .pl_2()
                .border_l_2()
                .border_color(p.focus)
                .child(hint(format!("Recommended: {}", conflict.recommend), p)),
        );
    }
    let recommended = card_data.recommended.as_deref();
    let ruling_button = |name: &'static str, ruling: Ruling, on: &OnConflict| {
        let on = on.clone();
        let mut button = button(SharedString::from(format!("rule-{name}-{id}"))).label(name);
        button = match recommended == Some(name) {
            true => button.primary(),
            false => button.outline(),
        };
        button.on_click(move |_, window, cx| on(ConflictAction::Rule(ruling.clone()), window, cx))
    };
    let evidence = {
        let on = on.clone();
        button(SharedString::from(format!("evidence-{id}")))
            .link()
            .label("Evidence")
            .on_click(move |_, window, cx| on(ConflictAction::Evidence, window, cx))
    };
    if keeping_both {
        let (keep, cancel) = (on.clone(), on.clone());
        let scope_line = scope.clone();
        return body.child(Input::new(scope).small()).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    button(SharedString::from(format!("keep-both-{id}")))
                        .primary()
                        .label("Keep both")
                        .on_click(move |_, window, cx| {
                            let scope = scope_line.read(cx).value().to_string();
                            keep(ConflictAction::Rule(Ruling::KeepBoth { scope }), window, cx)
                        }),
                )
                .child(
                    button(SharedString::from(format!("keep-both-cancel-{id}")))
                        .ghost()
                        .label("Cancel")
                        .on_click(move |_, window, cx| {
                            cancel(ConflictAction::KeepBoth(false), window, cx)
                        }),
                ),
        );
    }
    let keep_both = {
        let on = on.clone();
        let mut button =
            button(SharedString::from(format!("rule-keep-both-{id}"))).label("Keep both…");
        button = match recommended == Some("Keep both") {
            true => button.primary(),
            false => button.outline(),
        };
        button.on_click(move |_, window, cx| on(ConflictAction::KeepBoth(true), window, cx))
    };
    body.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(ruling_button(
                "Uphold existing",
                Ruling::UpholdExisting,
                &on,
            ))
            .child(ruling_button("Keep new", Ruling::KeepNew, &on))
            .child(keep_both)
            .child(ruling_button("Defer", Ruling::Defer, &on).ghost().tooltip(
                "Hides the card until the next compile, which raises it again if it still holds",
            ))
            .child(div().flex_1())
            .child(evidence),
    )
}
