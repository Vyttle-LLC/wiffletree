//! How full the selected session's context window is, and with what.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants};
use ui::{age, card, hint, icon, section};
use workspace_host::context::{ContextUsage, Probe};

/// Whole numbers stay whole: `90k`, `4.2k`, `1M`, `601`.
fn compact(tokens: u64) -> String {
    let scaled = |value: f64, unit: &str| {
        if value >= 10. || value.fract() < 0.05 {
            format!("{value:.0}{unit}")
        } else {
            format!("{value:.1}{unit}")
        }
    };
    if tokens >= 1_000_000 {
        scaled(tokens as f64 / 1e6, "M")
    } else if tokens >= 1000 {
        scaled(tokens as f64 / 1e3, "k")
    } else {
        tokens.to_string()
    }
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
    /// Probes the selected session when its last turn is newer than what is shown.
    pub(super) fn sync_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let finished = self.runtime(&session.id).and_then(|r| r.last_finished_at);
        let current = self.context_seen.get(&session.id) == Some(&finished);
        if !current && session.status != Status::Working {
            self.refresh_context(window, cx);
        }
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
            .insert(session.id.clone(), runtime.last_finished_at);
        let (Some(provider_session), Some(directory), Some(profile)) = (
            runtime.provider_session_id,
            runtime.directory,
            self.selected_profile(),
        ) else {
            self.context.insert(
                session.id,
                Err("Appears after this session’s next turn.".into()),
            );
            return;
        };
        self.context_pending.insert(session.id.clone());
        let (send, receive) = async_channel::bounded(1);
        let role = session.role;
        std::thread::spawn(move || {
            let probe = Probe {
                role,
                profile: &profile,
                provider_session: &provider_session,
                directory: std::path::Path::new(&directory),
            };
            let usage = workspace_host::context::usage(&probe).map_err(|e| format!("{e:#}"));
            let _ = send.send_blocking(usage);
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(usage) = receive.recv().await {
                let _ = this.update(cx, |view, cx| {
                    view.context_pending.remove(&session.id);
                    view.context.insert(session.id, usage);
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
                .child(format!("{filled}%")),
        )
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
                        .child(compact(tokens)),
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
                .disabled(pending || session.status == Status::Working)
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
                        compact(usage.window),
                        compact(point)
                    ),
                    None => format!("of {}", compact(usage.window)),
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
                                    .child(compact(usage.used)),
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
        card(p).child(header).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{compact, percent};

    #[test]
    fn token_counts_read_like_the_provider_prints_them() {
        assert_eq!(compact(90_000), "90k");
        assert_eq!(compact(4_200), "4.2k");
        assert_eq!(compact(1_000_000), "1M");
        assert_eq!(compact(601), "601");
        assert_eq!(compact(987_000), "987k");
    }

    #[test]
    fn percentages_round_and_survive_an_unknown_window() {
        assert_eq!(percent(90_000, 1_000_000), 9);
        assert_eq!(percent(5, 0), 500);
        assert_eq!(super::share(4_200, 1_000_000), "<1%");
        assert_eq!(super::share(0, 1_000_000), "0%");
    }
}
