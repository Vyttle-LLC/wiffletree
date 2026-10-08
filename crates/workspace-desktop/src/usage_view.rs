//! Account quota, recorded token usage and the project event log.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants, tooltip::Tooltip};
use ui::{age, card, empty_state, eyebrow, hint, humanize, now, property, section, segment};

fn number(n: u64) -> String {
    let raw = n.to_string();
    raw.chars()
        .enumerate()
        .fold(String::new(), |mut s, (i, c)| {
            if i > 0 && (raw.len() - i).is_multiple_of(3) {
                s.push(',');
            }
            s.push(c);
            s
        })
}
fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.)
    } else {
        n.to_string()
    }
}
fn reading_age(at: i64) -> String {
    if at == 0 {
        "No reading".into()
    } else {
        age(at)
    }
}
fn quota_color(used: f64, p: Palette) -> Hsla {
    if used >= 90. {
        p.red
    } else if used >= 75. {
        p.yellow
    } else {
        p.focus
    }
}
fn remaining(window: &QuotaWindow) -> String {
    match window.used_percent {
        Some(used) => format!("{:.0}% left", (100. - used).clamp(0., 100.)),
        None => "Unavailable".into(),
    }
}
fn stale(window: &QuotaWindow) -> bool {
    now() - window.observed_at > 15 * 60_000 || window.resets_at.is_some_and(|t| t <= now())
}
fn timestamp(at: i64) -> String {
    workspace_host::usage::local_time(at, "America/New_York").unwrap_or_else(|| "Unknown".into())
}
fn meter(used: f64, color: Hsla, p: Palette) -> Div {
    div()
        .w_full()
        .h(px(5.))
        .rounded_full()
        .bg(p.edge.opacity(0.3))
        .child(
            div()
                .h_full()
                .w(relative((used.clamp(0., 100.) / 100.) as f32))
                .rounded_full()
                .bg(color),
        )
}
fn reset(window: &QuotaWindow) -> Option<String> {
    window.resets_at.map(|reset| {
        let minutes = (reset - now()) / 60_000;
        if minutes <= 0 {
            "reset due".to_owned()
        } else {
            format!("resets in {}h {}m", minutes / 60, minutes % 60)
        }
    })
}
/// The most-used window of a duration; Codex can report several buckets per duration.
fn busiest(windows: &[QuotaWindow], duration_mins: u64) -> Option<&QuotaWindow> {
    windows
        .iter()
        .filter(|w| w.duration_mins == Some(duration_mins))
        .max_by(|a, b| {
            a.used_percent
                .unwrap_or(-1.)
                .total_cmp(&b.used_percent.unwrap_or(-1.))
        })
}
fn quota_pill(
    provider: Provider,
    short: &'static str,
    window: Option<&QuotaWindow>,
    reading: Option<&QuotaReading>,
    p: Palette,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let used = window.and_then(|w| w.used_percent);
    let faded = window.is_none_or(stale);
    let tooltip = match (window, reading) {
        (Some(w), Some(r)) => [
            Some(format!("{provider:?} {} · {}", w.label, remaining(w))),
            reset(w),
            Some(format!(
                "{} · {}{}",
                r.source,
                reading_age(w.observed_at).to_lowercase(),
                if stale(w) { " · stale" } else { "" }
            )),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n"),
        (None, Some(r)) => r
            .error
            .clone()
            .unwrap_or_else(|| format!("{provider:?} reported no {short} window")),
        _ => "Waiting for provider telemetry".into(),
    };
    let weak = cx.weak_entity();
    div()
        .id(SharedString::from(format!("quota-{provider:?}-{short}")))
        .relative()
        .flex_1()
        .min_w_0()
        .h(px(24.))
        .rounded_md()
        .overflow_hidden()
        .cursor_pointer()
        .bg(p.overlay.opacity(0.6))
        .hover(move |d| d.bg(p.overlay))
        .when_some(used, |d, used| {
            d.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative((used.clamp(0., 100.) / 100.) as f32))
                    .bg(quota_color(used, p).opacity(0.3)),
            )
        })
        .child(
            div()
                .relative()
                .size_full()
                .px_2()
                .flex()
                .items_center()
                .gap_1()
                .text_size(px(12.))
                .when(faded, |d| d.opacity(0.55))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(used.map_or("–".into(), |u| format!("{u:.0}%"))),
                )
                .child(div().text_color(p.subtle).child(short)),
        )
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |v, cx| v.open_page(Page::Usage, window, cx));
        })
}
impl Workspace {
    pub(super) fn usage_project_id(&self) -> Option<String> {
        if self.usage_project {
            self.project_id()
        } else {
            None
        }
    }
    pub(super) fn load_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(
            Command::Usage {
                days: self.usage_days,
                project_id: self.usage_project_id(),
                provider: self.usage_provider,
                timezone: "America/New_York".into(),
            },
            window,
            cx,
        );
    }
    pub(super) fn refresh_quota(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(Command::Quotas, window, cx);
        if self.quota_pending {
            return;
        }
        self.quota_pending = true;
        let (send, receive) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let reading =
                workspace_host::runtime::codex_quota().unwrap_or_else(|error| QuotaReading {
                    provider: Provider::Codex,
                    account: None,
                    observed_at: now(),
                    source: "Codex app-server".into(),
                    windows: vec![],
                    error: Some(error.to_string()),
                });
            let _ = send.send_blocking(reading);
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(reading) = receive.recv().await {
                let _ = cx.update(|window, cx| {
                    this.update(cx, |view, cx| {
                        view.request(Command::RecordQuota { reading }, window, cx)
                    })
                });
            }
        })
        .detach();
    }
    /// One row per provider of compact 5-hour and weekly pills, filled to the share used.
    pub(super) fn quota_strip(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut strip = div().flex_none().px_4().pb_2().flex().flex_col().gap_1();
        for provider in [Provider::Claude, Provider::Codex] {
            let reading = self.quotas.iter().find(|r| r.provider == provider);
            let mut row = div().flex().items_center().gap_1().child(
                div()
                    .w(px(46.))
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(p.subtle)
                    .child(format!("{provider:?}")),
            );
            for (duration, short) in [(300, "5h"), (10080, "wk")] {
                let window = reading.and_then(|r| busiest(&r.windows, duration));
                row = row.child(quota_pill(provider, short, window, reading, p, cx));
            }
            strip = strip.child(row);
        }
        strip
    }
    /// Events for the selected session: everything for a project, a team's own and its workers'.
    pub(super) fn events_panel(&self, p: Palette) -> Div {
        let Some(selected) = self.selected_session() else {
            return div();
        };
        let Ok(events) = serde_json::from_value::<Vec<Activity>>(self.panel_data.clone()) else {
            return hint("Loading events…", p);
        };
        let in_scope = |event: &Activity| {
            selected.role == Role::ProjectOrchestrator
                || event
                    .session_id
                    .as_deref()
                    .is_some_and(|id| id == selected.id)
        };
        let mut body = div().flex().flex_col().child(section(
            format!("RECENT · {}", selected.name.to_uppercase()),
            p,
        ));
        let mut shown = 0;
        for event in events.iter().filter(|event| in_scope(event)) {
            shown += 1;
            let source = event
                .session_id
                .as_deref()
                .and_then(|id| self.session(id))
                .map(|s| s.name.clone());
            body = body.child(
                div()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(p.edge.opacity(0.25))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(humanize(&event.kind)),
                            )
                            .child(hint(age(event.created_at), p).flex_none()),
                    )
                    .when_some(source, |d, source| d.child(hint(source, p)))
                    .child(
                        // A rejection's allowed list is the part a reader needs.
                        hint(event.detail.clone(), p)
                            .when(event.kind != "model_rejected", |d| d.line_clamp(3)),
                    ),
            );
        }
        if shown == 0 {
            body = body.child(empty_state(
                "activity",
                "Nothing yet",
                "Messages, status changes and reports appear here as they happen.",
                p,
            ));
        }
        body
    }
    fn usage_filters(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut periods = div().flex().gap_1();
        for (days, label) in [(1, "Today"), (7, "7 days"), (30, "30 days")] {
            periods = periods.child(
                segment(
                    SharedString::from(format!("usage-days-{days}")),
                    label,
                    self.usage_days == days,
                )
                .on_click(cx.listener(move |v, _, w, c| {
                    v.usage_days = days;
                    v.usage_day = None;
                    v.usage_report = None;
                    v.load_usage(w, c);
                })),
            );
        }
        let mut providers = div().flex().gap_1();
        for (provider, label) in [
            (None, "Both"),
            (Some(Provider::Claude), "Claude"),
            (Some(Provider::Codex), "Codex"),
        ] {
            providers = providers.child(
                segment(
                    SharedString::from(format!("usage-provider-{label}")),
                    label,
                    self.usage_provider == provider,
                )
                .on_click(cx.listener(move |v, _, w, c| {
                    v.usage_provider = provider;
                    v.usage_report = None;
                    v.load_usage(w, c);
                })),
            );
        }
        let project = self.project_name();
        let mut scopes = div().flex().gap_1();
        for (scoped, label) in [(false, "All projects".to_owned()), (true, project.clone())] {
            scopes = scopes.child(
                segment(
                    SharedString::from(format!("usage-scope-{scoped}")),
                    label,
                    self.usage_project == scoped,
                )
                .disabled(scoped && project.is_empty())
                .on_click(cx.listener(move |v, _, w, c| {
                    v.usage_project = scoped;
                    v.usage_report = None;
                    v.load_usage(w, c);
                })),
            );
        }
        let filter = |label: &'static str, choices: Div| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(hint(label, p))
                .child(choices)
        };
        div()
            .flex()
            .flex_wrap()
            .gap_x_5()
            .gap_y_2()
            .child(filter("Scope", scopes))
            .child(filter("Period", periods))
            .child(filter("Provider", providers))
    }

    fn tile(label: &'static str, value: String, detail: String, p: Palette) -> Div {
        card(p)
            .flex_1()
            .min_w(px(150.))
            .gap_1()
            .child(hint(label, p))
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
            .child(hint(detail, p))
    }

    fn day_detail(&self, report: &UsageReport, p: Palette) -> Div {
        let date = self.usage_day.as_deref().unwrap_or(&report.end_date);
        let mut detail = card(p).child(eyebrow(format!("DAY · {date}"), p));
        for day in report.daily.iter().filter(|day| day.date == date) {
            detail = detail
                .child(
                    div()
                        .mt_1()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(format!("{:?}", day.provider)),
                        )
                        .child(hint(
                            if day.covered {
                                "Recorded"
                            } else {
                                "No coverage"
                            },
                            p,
                        )),
                )
                .child(property("Input", number(day.counts.input), p))
                .child(property("Output", number(day.counts.output), p))
                .child(property("Cache read", number(day.counts.cache_read), p))
                .child(property("Cache write", number(day.counts.cache_write), p))
                .child(property("Reasoning", number(day.counts.reasoning), p));
        }
        detail.child(hint(
            "Cache is part of input; reasoning is part of output.",
            p,
        ))
    }

    fn coordinator_usage(&self, report: &UsageReport, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut work = card(p).child(eyebrow("BY COORDINATOR", p));
        let largest = report
            .coordinators
            .iter()
            .map(|c| c.own.total() + c.workers.total())
            .max()
            .unwrap_or(1)
            .max(1);
        for coordinator in &report.coordinators {
            let id = coordinator.id.clone();
            let total = coordinator.own.total() + coordinator.workers.total();
            work = work.child(
                div()
                    .id(SharedString::from(format!("usage-owner-{id}")))
                    .py_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, _, w, c| v.select(id.clone(), w, c)))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!(
                                        "{} · {}",
                                        coordinator.name, coordinator.project
                                    )),
                            )
                            .child(div().flex_none().child(compact(total))),
                    )
                    .child(meter(total as f64 / largest as f64 * 100., p.focus, p))
                    .child(hint(
                        format!(
                            "Own {} · workers {}",
                            number(coordinator.own.total()),
                            number(coordinator.workers.total())
                        ),
                        p,
                    )),
            );
        }
        if report.coordinators.is_empty() {
            work = work.child(hint("No recorded requests in this period.", p));
        }
        work
    }

    fn quota_trend(report: &UsageReport, trend: &QuotaTrend, p: Palette) -> Div {
        let largest = trend
            .daily
            .iter()
            .filter_map(|d| d.increase)
            .fold(1., f64::max);
        let mut chart = div().w_full().h(px(70.)).flex().items_end().gap_1();
        for (index, day) in trend.daily.iter().enumerate() {
            let tooltip = format!(
                "{} · {}",
                day.date,
                day.increase
                    .map(|n| format!("{n:.1} percentage points used"))
                    .unwrap_or_else(|| "No comparable readings".into())
            );
            chart = chart.child(
                div()
                    .id(SharedString::from(format!(
                        "quota-bar-{}-{}-{index}",
                        trend.provider as u8, trend.key
                    )))
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(
                        div()
                            .w_full()
                            .h(px(day
                                .increase
                                .map(|v| (v / largest * 62.) as f32)
                                .unwrap_or(2.)
                                .max(2.)))
                            .bg(if day.increase.is_some() {
                                p.accent
                            } else {
                                p.edge
                            }),
                    )
                    .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c)),
            );
        }
        card(p)
            .flex_1()
            .min_w(px(280.))
            .child(eyebrow(
                format!("{:?} · {} QUOTA USED PER DAY", trend.provider, trend.label),
                p,
            ))
            .child(chart)
            .child(hint(
                format!(
                    "{} → {}{}",
                    report.start_date,
                    report.end_date,
                    trend
                        .points_per_hour
                        .map(|rate| format!(" · {rate:.1} points per hour now"))
                        .unwrap_or_default()
                ),
                p,
            ))
    }

    /// Account limits and recorded tokens for the whole workspace.
    pub(super) fn usage_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_6()
            .child(self.limits(p, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(section("RECORDED IN THIS APP", p))
                    .child(self.usage_filters(p, cx)),
            );
        let Some(report) = &self.usage_report else {
            return body.child(hint("Loading recorded usage…", p));
        };
        let totals = &report.totals;
        body = body
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(Self::tile(
                        "Tokens",
                        compact(totals.total()),
                        format!("{} → {}", report.start_date, report.end_date),
                        p,
                    ))
                    .child(Self::tile(
                        "Input",
                        compact(totals.input),
                        format!("{} from cache", compact(totals.cache_read)),
                        p,
                    ))
                    .child(Self::tile(
                        "Output",
                        compact(totals.output),
                        format!("{} reasoning", compact(totals.reasoning)),
                        p,
                    ))
                    .child(Self::tile(
                        "Requests",
                        number(totals.requests),
                        format!("{} partial", totals.partial_requests),
                        p,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(360.))
                            .child(self.daily_chart(report, p, cx)),
                    )
                    .child(
                        div()
                            .w(px(280.))
                            .flex_none()
                            .child(self.day_detail(report, p)),
                    ),
            )
            .child(self.coordinator_usage(report, p, cx));
        if !report.quota_trends.is_empty() {
            let mut trends = div().flex().flex_wrap().gap_3();
            for trend in &report.quota_trends {
                trends = trends.child(Self::quota_trend(report, trend, p));
            }
            body = body.child(trends);
        }
        body.child(hint(
            format!(
                "Tracking began {} ({}). {} runs have no request-level report. Quota charts show observed account-wide changes, including other apps. Subagent usage counts only when the provider reports it.",
                timestamp(report.tracking_since),
                report.timezone,
                report.unreported_runs
            ),
            p,
        ))
    }
    fn daily_chart(&self, report: &UsageReport, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut days: BTreeMap<String, (TokenTotals, TokenTotals, bool)> = BTreeMap::new();
        for day in &report.daily {
            let (claude, codex, covered) = days.entry(day.date.clone()).or_default();
            if day.provider == Provider::Claude {
                claude.add(&day.counts);
            } else {
                codex.add(&day.counts);
            }
            *covered |= day.covered;
        }
        let max = days
            .values()
            .map(|(a, b, _)| a.total() + b.total())
            .max()
            .unwrap_or(1)
            .max(1);
        let mut chart = div().w_full().h(px(150.)).flex().items_end().gap_1();
        let count = days.len();
        for (index, (date, (claude, codex, covered))) in days.into_iter().enumerate() {
            let tooltip = format!(
                "{date}\nClaude input {} · output {}\nCodex input {} · output {}{}",
                number(claude.input),
                number(claude.output),
                number(codex.input),
                number(codex.output),
                if covered {
                    ""
                } else {
                    "\nNo collection coverage"
                }
            );
            let click_date = date.clone();
            let mut stack = div().w_full().flex().flex_col().justify_end().min_h(px(2.));
            for (tokens, color) in [
                (codex.output, p.accent),
                (codex.input, p.accent.opacity(0.45)),
                (claude.output, p.focus),
                (claude.input, p.focus.opacity(0.45)),
            ] {
                if tokens > 0 {
                    stack = stack.child(
                        div()
                            .w_full()
                            .h(px(tokens as f32 / max as f32 * 118.))
                            .bg(color),
                    );
                }
            }
            if claude.total() + codex.total() == 0 {
                stack = stack.child(div().w_full().h(px(2.)).bg(if covered {
                    p.subtle
                } else {
                    p.edge
                }));
            }
            chart = chart.child(
                div()
                    .id(SharedString::from(format!("token-bar-{date}")))
                    .cursor_pointer()
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap_1()
                    .child(stack)
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_center()
                            .text_color(p.subtle)
                            .child(if count <= 7 || index == 0 || index == count - 1 {
                                date[8..].to_owned()
                            } else {
                                "".into()
                            }),
                    )
                    .tooltip(move |w, c| Tooltip::new(tooltip.clone()).build(w, c))
                    .on_click(cx.listener(move |v, _, _, c| {
                        v.usage_day = Some(click_date.clone());
                        c.notify();
                    })),
            );
        }
        card(p)
            .child(eyebrow("DAILY TOKENS", p))
            .child(hint(
                format!("Peak {} tokens · tap a day for exact totals", number(max)),
                p,
            ))
            .child(chart)
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_wrap()
                    .text_size(px(11.))
                    .child(div().text_color(p.focus).child("Claude"))
                    .child(div().text_color(p.accent).child("Codex")),
            )
            .child(hint(
                "Light: input · Solid: output · Baseline: zero or no coverage",
                p,
            ))
    }
    fn provider_limits(&self, provider: Provider, p: Palette) -> Div {
        let reading = self.quotas.iter().find(|r| r.provider == provider);
        let mut body = card(p).flex_1().min_w(px(280.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{provider:?}")),
                )
                .child(
                    hint(
                        reading
                            .and_then(|r| r.account.clone())
                            .unwrap_or_else(|| "Account unknown".into()),
                        p,
                    )
                    .min_w_0()
                    .text_ellipsis(),
                ),
        );
        let Some(reading) = reading else {
            return body.child(hint("Unavailable · waiting for provider telemetry", p));
        };
        if reading.windows.is_empty() {
            body = body.child(hint(
                if provider == Provider::Claude {
                    "No reading yet. Claude reports its limits during live responses."
                } else {
                    "Unavailable"
                },
                p,
            ));
        }
        for window in &reading.windows {
            let detail = [
                reset(window),
                Some(format!(
                    "read {}{}",
                    reading_age(window.observed_at).to_lowercase(),
                    if stale(window) { " · stale" } else { "" }
                )),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            body = body
                .child(
                    div()
                        .mt_1()
                        .flex()
                        .justify_between()
                        .child(window.label.clone())
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(remaining(window)),
                        ),
                )
                .when_some(window.used_percent, |d, used| {
                    d.child(meter(used, quota_color(used, p), p))
                })
                .child(hint(detail, p));
        }
        if let Some(error) = &reading.error {
            body = body.child(hint(error.clone(), p).text_color(p.yellow).line_clamp(3));
        }
        body
    }

    /// Both providers, always: limits belong to the account, not to the selected agent.
    fn limits(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                section("ACCOUNT LIMITS", p).child(
                    button("quota-refresh")
                        .ghost()
                        .xsmall()
                        .icon(ui::icon("refresh").size(px(13.)))
                        .loading(self.quota_pending)
                        .disabled(self.quota_pending)
                        .tooltip("Refresh account limits")
                        .on_click(cx.listener(|v, _, w, c| v.refresh_quota(w, c))),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(PROVIDERS.map(|provider| self.provider_limits(provider, p))),
            )
            .child(hint(
                "Shared across every app and machine on these accounts. Codex refreshes every 5 minutes; Claude reports during live responses.",
                p,
            ))
    }
}
