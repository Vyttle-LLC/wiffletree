//! Account quota, recorded token usage and the project event log.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants, tooltip::Tooltip};
use ui::{card, empty_state, eyebrow, hint, now, property, section, segment};
use wording::{age, humanize};

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
    wording::local_time(at, "America/New_York").unwrap_or_else(|| "Unknown".into())
}
fn meter(used: f64, color: Hsla, p: Palette) -> Div {
    div()
        .w_full()
        .h(px(5.))
        .rounded_full()
        .bg(p.edge.opacity(edge::SEPARATOR))
        .child(
            div()
                .h_full()
                .w(relative((used.clamp(0., 100.) / 100.) as f32))
                .rounded_full()
                .bg(color),
        )
}
fn reset(window: &QuotaWindow) -> Option<String> {
    window.resets_at.map(wording::resets_in)
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
fn quota_pill<V: EventEmitter<Intent>>(
    provider: Provider,
    short: &'static str,
    window: Option<&QuotaWindow>,
    reading: Option<&QuotaReading>,
    p: Palette,
    cx: &mut Context<V>,
) -> Stateful<Div> {
    let used = window.and_then(|w| w.used_percent);
    let faded = window.is_none_or(stale);
    let tooltip = match (window, reading) {
        (Some(w), Some(r)) => [
            Some(format!(
                "{} {} · {}",
                provider.label(),
                w.label,
                remaining(w)
            )),
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
            .unwrap_or_else(|| format!("{} reported no {short} window", provider.label())),
        _ => "Waiting for provider telemetry".into(),
    };
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
                .text_size(px(font::SMALL))
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
        .on_click(cx.listener(|_, _, _, cx| cx.emit(Intent::OpenPage(Page::Usage))))
}
/// One row per provider of compact 5-hour and weekly pills, filled to the share used.
pub(super) fn quota_strip<V: EventEmitter<Intent>>(
    quotas: &[QuotaReading],
    p: Palette,
    cx: &mut Context<V>,
) -> Div {
    let mut strip = div().flex_none().px_4().pb_2().flex().flex_col().gap_1();
    for provider in [Provider::Claude, Provider::Codex] {
        let reading = quotas.iter().find(|r| r.provider == provider);
        let mut row = div().flex().items_center().gap_1().child(
            div()
                .w(px(46.))
                .flex_none()
                .text_size(px(font::CAPTION))
                .text_color(p.subtle)
                .child(provider.label()),
        );
        for (duration, short) in [(300, "5h"), (10080, "wk")] {
            let window = reading.and_then(|r| busiest(&r.windows, duration));
            row = row.child(quota_pill(provider, short, window, reading, p, cx));
        }
        strip = strip.child(row);
    }
    strip
}

impl Inspector {
    /// Events for the selected session: everything for a project, a team's own and its workers'.
    pub(super) fn events_panel(&self, p: Palette) -> Div {
        let Some(selected) = self.shared.selected_session() else {
            return div();
        };
        let PanelData::Events(events) = &self.panel_data else {
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
                .and_then(|id| self.shared.session(id))
                .map(|s| s.name.clone());
            body = body.child(
                div()
                    .py_2()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(p.edge.opacity(edge::HAIRLINE))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(font::MESSAGE))
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
}
/// The Usage page: account limits, and the tokens recorded in this app by period, provider and
/// project.
pub(super) struct UsagePage {
    bridge: Bridge,
    shared: Shared,
    report: Option<UsageReport>,
    days: u16,
    /// Only the selected session's project, rather than all of them.
    scoped: bool,
    provider: Option<Provider>,
    /// The day the chart's detail shows; the period's last when none is picked.
    day: Option<String>,
}
impl EventEmitter<Intent> for UsagePage {}
impl UsagePage {
    pub(super) fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            shared: Shared::default(),
            report: None,
            days: 7,
            scoped: false,
            provider: None,
            day: None,
        }
    }
    /// Loads the report when the page opens and again with each new quota reading, which a
    /// provider's usage report also brings.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.shared, shared);
        if self.shared.page != Page::Usage {
            return;
        }
        if old.page != Page::Usage || !Rc::ptr_eq(&old.quotas, &self.shared.quotas) {
            self.load(window, cx);
        }
        cx.notify();
    }
    pub(super) fn scoped_project_id(&self) -> Option<String> {
        if self.scoped {
            self.shared.project_id()
        } else {
            None
        }
    }
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (days, project_id, provider) = (self.days, self.scoped_project_id(), self.provider);
        let ask = ask::usage(
            days,
            project_id.clone(),
            provider,
            "America/New_York".into(),
        );
        self.ask(ask, window, cx, move |view, report, window, cx| {
            // A reply to filters since changed is dropped.
            let filters = (view.days, view.scoped_project_id(), view.provider);
            if filters != (days, project_id, provider) {
                return;
            }
            match report {
                Ok(report) => view.report = Some(report),
                Err(error) => toast_error(error, window, cx),
            }
        });
    }
    fn usage_filters(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut periods = div().flex().gap_1();
        for (days, label) in [(1, "Today"), (7, "7 days"), (30, "30 days")] {
            periods = periods.child(
                segment(
                    SharedString::from(format!("usage-days-{days}")),
                    label,
                    self.days == days,
                )
                .on_click(cx.listener(move |v, _, w, c| {
                    v.days = days;
                    v.day = None;
                    v.report = None;
                    v.load(w, c);
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
                    self.provider == provider,
                )
                .on_click(cx.listener(move |v, _, w, c| {
                    v.provider = provider;
                    v.report = None;
                    v.load(w, c);
                })),
            );
        }
        let project = self.shared.project_name();
        let mut scopes = div().flex().gap_1();
        for (scoped, label) in [(false, "All projects".to_owned()), (true, project.clone())] {
            scopes = scopes.child(
                segment(
                    SharedString::from(format!("usage-scope-{scoped}")),
                    label,
                    self.scoped == scoped,
                )
                .disabled(scoped && project.is_empty())
                .on_click(cx.listener(move |v, _, w, c| {
                    v.scoped = scoped;
                    v.report = None;
                    v.load(w, c);
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
                    .text_size(px(font::FIGURE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
            .child(hint(detail, p))
    }
    fn day_detail(&self, report: &UsageReport, p: Palette) -> Div {
        let date = self.day.as_deref().unwrap_or(&report.end_date);
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
                                .child(day.provider.label()),
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
                    .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::Select(id.clone()))))
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
                format!(
                    "{} · {} QUOTA USED PER DAY",
                    trend.provider.label(),
                    trend.label
                ),
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
    fn body(&self, p: Palette, cx: &mut Context<Self>) -> Div {
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
        let Some(report) = &self.report else {
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
                            .text_size(px(font::NANO))
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
                        v.day = Some(click_date.clone());
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
                    .text_size(px(font::CAPTION))
                    .child(div().text_color(p.focus).child("Claude"))
                    .child(div().text_color(p.accent).child("Codex")),
            )
            .child(hint(
                "Light: input · Solid: output · Baseline: zero or no coverage",
                p,
            ))
    }
    fn provider_limits(&self, provider: Provider, p: Palette) -> Div {
        let reading = self.shared.quotas.iter().find(|r| r.provider == provider);
        let mut body = card(p).flex_1().min_w(px(280.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_size(px(font::BODY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(provider.label()),
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
                    ui::icon_or_spinner(
                        button("quota-refresh").ghost().xsmall(),
                        ui::icon("refresh").size(px(13.)),
                        None,
                        self.shared.quota_pending,
                        gpui_component::Size::XSmall,
                    )
                    .disabled(self.shared.quota_pending)
                        .tooltip("Refresh account limits")
                        .on_click(cx.listener(|_, _, _, c| c.emit(Intent::RefreshQuotas))),
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
impl Requests for UsagePage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        _: Command,
        _: Result<Value, String>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }
}
impl Render for UsagePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Usage");
        let p = palette(is_dark(cx));
        ui::page(
            "Usage",
            "Account limits and tokens across every project",
            self.body(p, cx),
            None,
            p,
            cx,
        )
    }
}
