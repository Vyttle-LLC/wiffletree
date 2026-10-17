//! How full the selected session's context window is, and with what; and the Compact action
//! that asks the agent's provider to replace the conversation with a summary.
use super::*;
use gpui_component::{Disableable, Sizable, button::ButtonVariants};
use ui::{Spinner, age, card, hint, icon, progress_bar, section};
use workspace_host::compaction::REASON_LIMIT;
use workspace_host::context::{ContextUsage, Probe};

/// The Compact form on a session's Context card, while it is open.
#[derive(Default)]
pub(super) struct CompactForm {
    /// What was typed, kept while another session is selected or the host is asked.
    note: String,
    /// Why the host refused the request, shown in the form.
    refused: Option<String>,
    sending: bool,
}

/// What a measurement is as fresh as: the session's last finished turn and the outcome of
/// its last compaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Measured {
    turn: Option<i64>,
    compaction: Option<i64>,
}
impl Measured {
    fn of(runtime: &SessionRuntime) -> Self {
        Self {
            turn: runtime.last_finished_at,
            compaction: runtime.last_compaction.as_ref().map(|outcome| outcome.at),
        }
    }
}

fn compacting(runtime: Option<&SessionRuntime>) -> bool {
    let request = runtime.and_then(|r| r.compaction.as_ref());
    request.is_some_and(|c| c.state == CompactionState::Running)
}

/// Whether to measure the context now. A finished turn is measured once the agent has stopped
/// working. A compaction's outcome is measured at once, even while the next turn runs, so the
/// card never shows the old figure beside "Context compacted". Never while one is compacting.
fn measure_now(seen: Option<Measured>, runtime: &SessionRuntime, working: bool) -> bool {
    let now = Measured::of(runtime);
    let new_outcome =
        now.compaction.is_some() && seen.map(|s| s.compaction) != Some(now.compaction);
    let new_turn = seen.map(|s| s.turn) != Some(now.turn);
    !compacting(Some(runtime)) && (new_outcome || (new_turn && !working))
}

/// The Compact row: its hint, the button's label, and whether the button works. The same for
/// either provider.
fn compact_action(has_session: bool, working: bool) -> (&'static str, &'static str, bool) {
    const HINT: &str = "Replace the conversation with a summary.";
    match (has_session, working) {
        (false, _) => (
            "Available after this agent’s first turn.",
            "Compact…",
            false,
        ),
        (true, true) => (HINT, "Compact after this turn", true),
        (true, false) => (HINT, "Compact…", true),
    }
}

/// The form's consequence line and its confirm button.
fn compact_form_text(provider: Provider, working: bool) -> (&'static str, &'static str) {
    match (working, provider) {
        (true, _) => (
            "This agent is working. The compaction runs when this turn ends, before any waiting messages are delivered. It can’t be undone and uses one model call.",
            "Compact after this turn",
        ),
        (false, Provider::Claude) => (
            "Claude replaces this agent’s conversation with a summary. This can’t be undone and uses one model call.",
            "Compact now",
        ),
        (false, Provider::Codex) => (
            "Codex replaces this agent’s conversation with a summary and chooses what the summary keeps. This can’t be undone and uses one model call.",
            "Compact now",
        ),
    }
}

/// Whether the form has the "What to keep" field.
fn takes_note(provider: Provider) -> bool {
    provider == Provider::Claude
}

/// What the request carries of what was typed. Codex's compact request has no field for a
/// note, so its form has none and a draft kept for the session is never sent.
fn compact_instructions(provider: Provider, note: &str) -> Option<String> {
    (takes_note(provider) && !note.trim().is_empty()).then(|| note.to_owned())
}

/// What a queued compaction waits for: its turn, or an agent that is paused, stopped or
/// interrupted to run again.
fn queued_text(working: bool) -> &'static str {
    if working {
        "Compacts when this turn ends"
    } else {
        "Compacts when this agent runs again"
    }
}

/// What the header chip adds after its percentage while a compaction waits or runs.
fn chip_note(compaction: Option<&Compaction>) -> Option<&'static str> {
    Some(match compaction?.state {
        CompactionState::Queued => "compact queued",
        CompactionState::Running => "compacting",
    })
}

/// The notice a compaction's outcome leaves on the card: its title, its text, and whether it is
/// a failure. A compacted context needs none; the lower figure is the result. A reason is
/// cut where the host cuts the ones it builds, so those show whole.
fn compaction_notice(
    provider: Provider,
    outcome: &CompactionOutcome,
) -> Option<(&'static str, String, bool)> {
    let reason = outcome.reason.as_deref().map(|reason| {
        let reason = conversation::first_line(reason, REASON_LIMIT);
        if reason.ends_with(['.', '!', '?', '…']) {
            format!("{reason} ")
        } else {
            format!("{reason}. ")
        }
    });
    let reason = reason.unwrap_or_default();
    let title = outcome.result.title();
    match outcome.result {
        CompactionResult::Compacted => None,
        CompactionResult::NothingToCompact => {
            let said = if reason.is_empty() {
                ""
            } else {
                "Claude said: "
            };
            Some((
                title,
                format!("{said}{reason}The context is unchanged."),
                false,
            ))
        }
        CompactionResult::Failed => {
            Some((title, format!("{reason}The context is unchanged."), true))
        }
        CompactionResult::Interrupted => Some((
            title,
            format!(
                "Wiffletree stopped while {} was compacting. The figure above was measured again.",
                provider.label()
            ),
            true,
        )),
    }
}

/// A block under the measurement, marked along its left edge in `color`.
fn notice(color: Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .px(px(10.))
        .py(px(7.))
        .border_l_2()
        .border_color(color)
        .rounded_r(px(6.))
        .bg(color.opacity(0.07))
        .text_size(px(12.))
}

fn notice_title(title: &'static str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .font_weight(FontWeight::MEDIUM)
        .child(title)
}

/// The part of the card under the measurement, set off by a rule.
fn below_measurement(p: Palette) -> Div {
    div()
        .pt(px(9.))
        .border_t_1()
        .border_color(p.edge.opacity(0.4))
}

fn percent(tokens: u64, window: u64) -> u64 {
    (tokens as f64 / window.max(1) as f64 * 100.).round() as u64
}

/// A share of the window; small but present amounts are not rounded away to zero.
fn share(tokens: u64, window: u64) -> String {
    match percent(tokens, window) {
        0 if tokens > 0 => "<1%".into(),
        whole => format!("{whole}%"),
    }
}

fn fill_color(percent: u64, p: Palette) -> Hsla {
    if percent >= 90 {
        p.red
    } else if percent >= 75 {
        p.yellow
    } else {
        p.green
    }
}

fn swatch(color: Hsla) -> Div {
    div().flex_none().size(px(9.)).rounded(px(2.)).bg(color)
}

impl Workspace {
    /// Probes the selected session when its last turn or its last compaction is newer than
    /// what is shown; see `measure_now`.
    pub(super) fn sync_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let runtime = self.runtime(&session.id).cloned().unwrap_or_default();
        let seen = self.context_seen.get(&session.id).copied();
        if measure_now(seen, &runtime, session.status == Status::Working) {
            self.refresh_context(window, cx);
        }
    }

    /// Keeps what was typed in the Compact form with its session when another is selected.
    pub(super) fn swap_compact_note(
        &mut self,
        old: Option<&str>,
        new: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let typed = self.compact_note.read(cx).value().to_string();
        if let Some(form) = old.and_then(|old| self.compact_forms.get_mut(old)) {
            form.note = typed;
        }
        let note = self.compact_forms.get(new).map(|form| form.note.clone());
        self.compact_note.update(cx, |input, cx| {
            input.set_value(note.unwrap_or_default(), window, cx)
        });
    }

    fn open_compact_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let takes_note = takes_note(session.provider);
        self.compact_forms
            .insert(session.id.clone(), CompactForm::default());
        self.compact_note.update(cx, |input, cx| {
            input.set_value("", window, cx);
            if takes_note {
                input.focus(window, cx);
            }
        });
        cx.notify();
    }

    fn close_compact_form(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = &self.selected {
            self.compact_forms.remove(id);
            cx.notify();
        }
    }

    /// Sends the request; nothing reaches the host before this.
    fn confirm_compact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let (session_id, provider) = (session.id.clone(), session.provider);
        let note = self.compact_note.read(cx).value().to_string();
        let instructions = compact_instructions(provider, &note);
        let form = self.compact_forms.entry(session_id.clone()).or_default();
        *form = CompactForm {
            note,
            refused: None,
            sending: true,
        };
        self.request(
            Command::CompactContext {
                session_id,
                instructions,
            },
            window,
            cx,
        );
    }

    /// The host stored the request, so the card shows it in the form's place.
    pub(super) fn compact_accepted(&mut self, session_id: &str) {
        self.compact_forms.remove(session_id);
    }

    /// The host refused the request: its reason shows in the form, with what was typed.
    pub(super) fn compact_refused(&mut self, session_id: &str, reason: &str) {
        let form = self.compact_forms.entry(session_id.into()).or_default();
        form.refused = Some(reason.into());
        form.sending = false;
    }

    pub(super) fn compacting(&self, session_id: &str) -> bool {
        compacting(self.runtime(session_id))
    }

    pub(super) fn refresh_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        if self.context_pending.contains(&session.id) {
            return;
        }
        let runtime = self.runtime(&session.id).cloned().unwrap_or_default();
        self.context_seen
            .insert(session.id.clone(), Measured::of(&runtime));
        let skill = Skill::for_role(session.role);
        let instructions = (self.snapshot())
            .and_then(|snapshot| snapshot.skills.iter().find(|s| s.skill == skill))
            .map(|state| workspace_host::provider::role_instructions(session.role, state));
        let (Some(provider_session), Some(directory), Some(profile), Some(instructions)) = (
            runtime.provider_session_id,
            runtime.directory,
            self.selected_profile(),
            instructions,
        ) else {
            self.context.insert(
                session.id,
                Err("Appears after this session’s next turn.".into()),
            );
            return;
        };
        self.context_pending.insert(session.id.clone());
        let (send, receive) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let probe = Probe {
                profile: &profile,
                provider_session: &provider_session,
                directory: std::path::Path::new(&directory),
                instructions: &instructions,
            };
            let usage = workspace_host::context::usage(&probe).map_err(|e| format!("{e:#}"));
            let _ = send.send_blocking(usage);
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(usage) = receive.recv().await {
                let _ = this.update_in(cx, |view, window, cx| {
                    view.context_pending.remove(&session.id);
                    view.context.insert(session.id, usage);
                    // A turn or a compaction that ended while this probe ran is measured now;
                    // no later snapshot may come to ask for it.
                    view.sync_context(window, cx);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn context_colors(p: Palette) -> [Hsla; 8] {
        // Ordered so neighbouring segments differ in hue.
        [
            p.blue, p.yellow, p.magenta, p.green, p.accent, p.focus, p.red, p.subtle,
        ]
    }

    /// The fill level for the conversation header; opens the full breakdown.
    pub(super) fn context_chip(
        &self,
        session: &Session,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let usage = self.context.get(&session.id)?.as_ref().ok()?;
        let filled = percent(usage.used, usage.window);
        let color = fill_color(filled, p);
        let request = self
            .runtime(&session.id)
            .and_then(|r| r.compaction.as_ref());
        Some(
            div()
                .id("context-chip")
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .px_2()
                .py(px(3.))
                .rounded_full()
                .cursor_pointer()
                .bg(color.opacity(0.10))
                .text_color(color)
                .text_size(px(11.))
                .hover(move |d| d.bg(color.opacity(0.18)))
                .tooltip(move |w, c| {
                    gpui_component::tooltip::Tooltip::new("Context window in use").build(w, c)
                })
                .on_click(cx.listener(|v, _, w, c| v.show_panel(Panel::Overview, w, c)))
                .child(
                    div()
                        .w(px(28.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(color.opacity(0.25))
                        .child(
                            div()
                                .h_full()
                                .w(relative(filled.min(100) as f32 / 100.))
                                .rounded_full()
                                .bg(color),
                        ),
                )
                .child(format!("{filled}%"))
                .when_some(chip_note(request), |chip, note| {
                    chip.child(
                        div()
                            .pl(px(6.))
                            .border_l_1()
                            .border_color(color.opacity(0.35))
                            .child(note),
                    )
                }),
        )
    }

    /// The Compact row, or in its place the form, the waiting request or the running
    /// compaction; above it, the notice the last compaction left.
    fn compact_section(&self, session: &Session, p: Palette, cx: &mut Context<Self>) -> Div {
        let runtime = self.runtime(&session.id);
        let working = session.status == Status::Working;
        if let Some(request) = runtime.and_then(|r| r.compaction.as_ref()) {
            return match request.state {
                CompactionState::Running => notice(p.focus)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Spinner::new()
                                    .icon(icon("loader-circle"))
                                    .with_size(px(13.))
                                    .color(p.focus),
                            )
                            .child(notice_title("Compacting…")),
                    )
                    .child(progress_bar(None, BarState::Indeterminate, 4., p))
                    .child(hint(
                        "Messages you send now are delivered when it finishes.",
                        p,
                    )),
                CompactionState::Queued => notice(p.focus)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("clock").size(px(13.)).text_color(p.focus))
                            .child(notice_title(queued_text(working)))
                            .child(
                                button("compact-withdraw")
                                    .outline()
                                    .label("Cancel")
                                    .on_click(cx.listener(|v, _, w, c| {
                                        if let Some(session_id) = v.selected.clone() {
                                            v.request(
                                                Command::CancelCompaction { session_id },
                                                w,
                                                c,
                                            );
                                        }
                                    })),
                            ),
                    )
                    .when_some(request.instructions.as_ref(), |d, kept| {
                        d.child(hint(format!("Keeping: {kept}"), p))
                    }),
            };
        }
        let outcome = runtime.and_then(|r| r.last_compaction.as_ref());
        let left = outcome
            .and_then(|outcome| compaction_notice(session.provider, outcome))
            .map(|(title, text, failed)| {
                let color = if failed { p.red } else { p.subtle };
                notice(color)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(notice_title(title))
                            .child(button("compact-dismiss").ghost().label("Dismiss").on_click(
                                cx.listener(|v, _, w, c| {
                                    if let Some(session_id) = v.selected.clone() {
                                        v.request(Command::DismissCompaction { session_id }, w, c);
                                    }
                                }),
                            )),
                    )
                    .child(hint(text, p))
            });
        let action = match self.compact_forms.get(&session.id) {
            Some(form) => {
                let (consequence, confirm) = compact_form_text(session.provider, working);
                below_measurement(p)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(takes_note(session.provider), |form| {
                        form.child(
                            div()
                                .flex()
                                .gap_1()
                                .text_size(px(12.))
                                .child(div().font_weight(FontWeight::MEDIUM).child("What to keep"))
                                .child(div().text_color(p.subtle).child("· optional")),
                        )
                        .child(Textarea::new(&self.compact_note).w_full())
                    })
                    .child(hint(consequence, p))
                    .when_some(form.refused.clone(), |d, reason| {
                        d.child(hint(reason, p).text_color(p.red))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                button("compact-cancel")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|v, _, _, c| v.close_compact_form(c))),
                            )
                            .child(
                                button("compact-confirm")
                                    .primary()
                                    .label(confirm)
                                    .loading(form.sending)
                                    .on_click(cx.listener(|v, _, w, c| v.confirm_compact(w, c))),
                            ),
                    )
            }
            None => {
                let has_session = runtime.is_some_and(|r| r.provider_session_id.is_some());
                let (reason, label, enabled) = compact_action(has_session, working);
                below_measurement(p)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    // In a narrow panel the button wraps under the hint instead of squeezing it,
                    // and stays right-aligned with the card's other actions.
                    .child(hint(reason, p).flex_1().min_w(px(120.)))
                    .child(
                        button("compact")
                            .ml_auto()
                            .outline()
                            .label(label)
                            .disabled(!enabled)
                            .on_click(cx.listener(|v, _, w, c| v.open_compact_form(w, c))),
                    )
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(9.))
            .children(left)
            .child(action)
    }

    fn context_breakdown(usage: &ContextUsage, p: Palette) -> Div {
        let colors = Self::context_colors(p);
        let window = usage.window.max(1);
        let mut bar = div()
            .w_full()
            .h(px(14.))
            .flex()
            .rounded(px(3.))
            .overflow_hidden()
            .bg(p.edge.opacity(0.3));
        let mut legend = div().flex().flex_wrap().gap_x_3().gap_y_1();
        let entry = |color: Hsla, name: &str, tokens: u64| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(11.5))
                .child(swatch(color))
                .child(div().text_color(p.subtle).child(name.to_lowercase()))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(compact_tokens(tokens)),
                )
                .child(div().text_color(p.subtle).child(share(tokens, window)))
        };
        for (index, category) in usage.categories.iter().enumerate() {
            let color = colors[index % colors.len()];
            bar = bar.child(
                div()
                    .h_full()
                    .flex_none()
                    .w(relative(category.tokens as f32 / window as f32))
                    .bg(color),
            );
            legend = legend.child(entry(color, &category.name, category.tokens));
        }
        let free = usage.window.saturating_sub(usage.used);
        legend = legend.child(entry(p.edge.opacity(0.6), "Free", free));
        div().flex().flex_col().gap_2().child(bar).child(legend)
    }

    pub(super) fn context_card(
        &self,
        session: &Session,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let pending = self.context_pending.contains(&session.id);
        let header = section("CONTEXT", p).child(
            button("context-refresh")
                .ghost()
                .xsmall()
                .icon(icon("refresh").size(px(13.)))
                .loading(pending)
                .disabled(
                    pending || session.status == Status::Working || self.compacting(&session.id),
                )
                .tooltip("Measure again · no model call")
                .on_click(cx.listener(|v, _, w, c| v.refresh_context(w, c))),
        );
        let body = match self.context.get(&session.id) {
            None => hint(
                if pending {
                    "Measuring…"
                } else {
                    "Not measured yet."
                },
                p,
            ),
            Some(Err(reason)) => hint(reason.clone(), p).line_clamp(3),
            Some(Ok(usage)) => {
                let filled = percent(usage.used, usage.window);
                let color = fill_color(filled, p);
                let limits = match usage.compacts_at {
                    Some(point) => format!(
                        "of {} · compacts at {}",
                        compact_tokens(usage.window),
                        compact_tokens(point)
                    ),
                    None => format!("of {}", compact_tokens(usage.window)),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(compact_tokens(usage.used)),
                            )
                            .child(hint(limits, p).flex_1().min_w_0())
                            .child(
                                div()
                                    .flex_none()
                                    .px_2()
                                    .rounded_md()
                                    .bg(color.opacity(0.15))
                                    .text_color(color)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("{filled}%")),
                            ),
                    )
                    .child(Self::context_breakdown(usage, p))
                    .child(hint(
                        format!(
                            "{}{} · {}",
                            if usage.estimated {
                                "Codex reports the total; categories are estimated from its session record"
                            } else {
                                "Reported by Claude"
                            },
                            usage
                                .model
                                .as_ref()
                                .map(|model| format!(" · {model}"))
                                .unwrap_or_default(),
                            age(usage.observed_at).to_lowercase()
                        ),
                        p,
                    ))
            }
        };
        card(p)
            .child(header)
            .child(body)
            .child(self.compact_section(session, p, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Measured, chip_note, compact_action, compact_form_text, compact_instructions, compacting,
        compaction_notice, measure_now, percent, queued_text, takes_note,
    };
    use workspace_core::{
        Compaction, CompactionOutcome, CompactionResult, CompactionState, Provider, SessionRuntime,
    };

    fn runtime(
        turn: Option<i64>,
        outcome: Option<i64>,
        state: Option<CompactionState>,
    ) -> SessionRuntime {
        SessionRuntime {
            last_finished_at: turn,
            last_compaction: outcome.map(|at| CompactionOutcome {
                result: CompactionResult::Compacted,
                reason: None,
                at,
            }),
            compaction: state.map(|state| Compaction {
                instructions: None,
                requested_at: 0,
                state,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_finished_turn_is_measured_once_the_agent_stops_and_a_compaction_outcome_at_once() {
        let seen = |turn, compaction| Some(Measured { turn, compaction });
        // A session never measured, and a newer turn, wait for the agent to stop working.
        assert!(measure_now(None, &runtime(Some(1), None, None), false));
        assert!(!measure_now(None, &runtime(Some(1), None, None), true));
        assert!(measure_now(
            seen(Some(1), None),
            &runtime(Some(2), None, None),
            false
        ));
        assert!(!measure_now(
            seen(Some(1), None),
            &runtime(Some(2), None, None),
            true
        ));
        assert!(!measure_now(
            seen(Some(2), None),
            &runtime(Some(2), None, None),
            false
        ));
        // Working with a newer compaction outcome: measured during the turn that followed.
        assert!(measure_now(
            seen(Some(2), None),
            &runtime(Some(2), Some(3), None),
            true
        ));
        assert!(measure_now(None, &runtime(Some(2), Some(3), None), true));
        // Once, not on every refresh, and not again when the notice is dismissed.
        assert!(!measure_now(
            seen(Some(2), Some(3)),
            &runtime(Some(2), Some(3), None),
            true
        ));
        assert!(!measure_now(
            seen(Some(2), Some(3)),
            &runtime(Some(2), None, None),
            true
        ));
        // Never while a compaction runs, whatever is newer.
        let running = runtime(Some(4), Some(3), Some(CompactionState::Running));
        assert!(!measure_now(seen(Some(2), None), &running, false));
        // A queued one changes nothing about a finished turn's measurement.
        let queued = runtime(Some(4), None, Some(CompactionState::Queued));
        assert!(measure_now(seen(Some(2), None), &queued, false));
    }

    #[test]
    fn the_compact_row_says_why_it_is_off_and_what_it_does_now() {
        // The row never asks which provider the agent runs on: a Codex agent's is the same.
        const HINT: &str = "Replace the conversation with a summary.";
        assert_eq!(compact_action(true, false), (HINT, "Compact…", true));
        assert_eq!(
            compact_action(true, true),
            (HINT, "Compact after this turn", true)
        );
        for working in [false, true] {
            assert_eq!(
                compact_action(false, working),
                (
                    "Available after this agent’s first turn.",
                    "Compact…",
                    false
                )
            );
        }
    }

    #[test]
    fn a_codex_form_says_codex_chooses_and_sends_no_note() {
        let (line, confirm) = compact_form_text(Provider::Codex, false);
        assert_eq!(confirm, "Compact now");
        assert_eq!(
            line,
            "Codex replaces this agent’s conversation with a summary and chooses what the summary keeps. This can’t be undone and uses one model call."
        );
        // While the agent works the line names no provider, so both forms show the same one.
        assert_eq!(
            compact_form_text(Provider::Codex, true),
            compact_form_text(Provider::Claude, true)
        );
        assert!(takes_note(Provider::Claude));
        assert!(!takes_note(Provider::Codex));
        // A draft typed on a Claude agent and kept for the session never reaches Codex.
        let draft = "the failing test names";
        assert_eq!(compact_instructions(Provider::Codex, draft), None);
        assert_eq!(
            compact_instructions(Provider::Claude, draft),
            Some(draft.into())
        );
        assert_eq!(compact_instructions(Provider::Claude, " \n "), None);
    }

    #[test]
    fn the_form_names_the_cost_and_where_a_queued_compaction_falls() {
        let (line, confirm) = compact_form_text(Provider::Claude, false);
        assert_eq!(confirm, "Compact now");
        assert_eq!(
            line,
            "Claude replaces this agent’s conversation with a summary. This can’t be undone and uses one model call."
        );
        let (line, confirm) = compact_form_text(Provider::Claude, true);
        assert_eq!(confirm, "Compact after this turn");
        for said in [
            "runs when this turn ends, before any waiting messages are delivered",
            "can’t be undone",
            "one model call",
        ] {
            assert!(line.contains(said), "{said}");
        }
        assert_eq!(queued_text(true), "Compacts when this turn ends");
        assert_eq!(queued_text(false), "Compacts when this agent runs again");
    }

    #[test]
    fn the_chip_names_a_queued_or_running_compaction_and_nothing_otherwise() {
        let queued = runtime(None, None, Some(CompactionState::Queued));
        let running = runtime(None, None, Some(CompactionState::Running));
        assert_eq!(
            chip_note(queued.compaction.as_ref()),
            Some("compact queued")
        );
        assert_eq!(chip_note(running.compaction.as_ref()), Some("compacting"));
        assert_eq!(chip_note(None), None);
        assert!(compacting(Some(&running)));
        assert!(!compacting(Some(&queued)) && !compacting(None));
    }

    #[test]
    fn each_outcome_but_a_compacted_one_leaves_its_notice() {
        let outcome = |result, reason: Option<&str>| CompactionOutcome {
            result,
            reason: reason.map(str::to_owned),
            at: 1,
        };
        let compaction_notice = |outcome| compaction_notice(Provider::Claude, &outcome);
        assert_eq!(
            compaction_notice(outcome(CompactionResult::Compacted, None)),
            None
        );
        assert_eq!(
            compaction_notice(outcome(
                CompactionResult::NothingToCompact,
                Some("Not enough messages to compact.")
            )),
            Some((
                "Nothing to compact",
                "Claude said: Not enough messages to compact. The context is unchanged.".into(),
                false
            ))
        );
        assert_eq!(
            compaction_notice(outcome(
                CompactionResult::Failed,
                Some("Claude stopped before finishing: the connection was lost\nTraceback")
            )),
            Some((
                "Compaction failed",
                "Claude stopped before finishing: the connection was lost. The context is unchanged."
                    .into(),
                true
            ))
        );
        assert_eq!(
            compaction_notice(outcome(CompactionResult::Interrupted, None)),
            Some((
                "Compaction interrupted",
                "Wiffletree stopped while Claude was compacting. The figure above was measured again."
                    .into(),
                true
            ))
        );
    }

    #[test]
    fn a_codex_notice_names_codex_and_shows_the_hosts_reason_whole() {
        let outcome = |result, reason: Option<String>| CompactionOutcome {
            result,
            reason,
            at: 1,
        };
        let notice = |outcome| super::compaction_notice(Provider::Codex, &outcome);
        assert_eq!(
            notice(outcome(CompactionResult::Interrupted, None)),
            Some((
                "Compaction interrupted",
                "Wiffletree stopped while Codex was compacting. The figure above was measured again."
                    .into(),
                true
            ))
        );
        let refused = "Codex refused thread/compact/start: this Codex has no such method";
        assert_eq!(
            notice(outcome(CompactionResult::Failed, Some(refused.into()))),
            Some((
                "Compaction failed",
                format!("{refused}. The context is unchanged."),
                true
            ))
        );
        // A reason the host cut at its limit is not cut again, so it ends once, with its "…".
        let cut = format!("{}…", "x".repeat(super::REASON_LIMIT));
        assert_eq!(
            notice(outcome(CompactionResult::Failed, Some(cut.clone()))),
            Some((
                "Compaction failed",
                format!("{cut} The context is unchanged."),
                true
            ))
        );
        assert_eq!(notice(outcome(CompactionResult::Compacted, None)), None);
    }

    #[test]
    fn percentages_round_and_survive_an_unknown_window() {
        assert_eq!(percent(90_000, 1_000_000), 9);
        assert_eq!(percent(5, 0), 500);
        assert_eq!(super::share(4_200, 1_000_000), "<1%");
        assert_eq!(super::share(0, 1_000_000), "0%");
    }
}
