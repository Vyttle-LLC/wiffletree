//! The Usage page: account limits and the tokens recorded by period, provider and project.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants, tooltip::Tooltip};
use quotas::{quota_color, reading_age, remaining, reset, stale};
use ui::{card, eyebrow, hint, property, section, segment};

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
fn dollars(amount: f64) -> String {
    let cents = (amount * 100.).round() as u64;
    format!("${}.{:02}", number(cents / 100), cents % 100)
}
/// A cost, or a dash for a model with no known price.
fn estimated(cost: Option<Cost>) -> String {
    cost.map_or_else(|| "—".into(), |cost| dollars(cost.total()))
}
fn cost_parts(cost: &Cost) -> String {
    format!(
        "Input {} · cache read {} · cache write {} · output {}",
        dollars(cost.input),
        dollars(cost.cache_read),
        dollars(cost.cache_write),
        dollars(cost.output)
    )
}
/// Each model keeps its color wherever it appears on the page.
fn model_color(model: &str, p: Palette) -> Hsla {
    match model {
        "claude-opus-5-5" => p.accent,
        "gpt-6.1-sol" => p.blue,
        "claude-sonnet-5-5" => p.green,
        "claude-sonnet-5" => p.magenta,
        "claude-fable-5-1" => p.focus,
        "claude-haiku-5-5" => p.yellow,
        _ => p.subtle,
    }
}
fn swatch(color: Hsla) -> Div {
    div().size(px(9.)).flex_none().rounded(px(3.)).bg(color)
}
fn cell(text: impl Into<SharedString>) -> Div {
    div().w(px(78.)).flex_none().text_right().child(text.into())
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
    fn cost_tile(report: &UsageReport, p: Palette) -> Div {
        let total = report.cost.total();
        let priced: Vec<_> = report.models.iter().filter(|m| m.cost.is_some()).collect();
        let detail = match priced.first() {
            Some(top) if total > 0. => format!(
                "{} model{} · {} is {:.0}%",
                priced.len(),
                if priced.len() == 1 { "" } else { "s" },
                pricing::model_name(&top.model),
                top.cost.map_or(0., |cost| cost.total()) / total * 100.
            ),
            _ => "No priced requests".into(),
        };
        let figure = if total >= 100. {
            format!("${}", number(total.round() as u64))
        } else {
            dollars(total)
        };
        Self::tile("Estimated API cost", figure, detail, p)
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
        let models: Vec<_> = report
            .daily_models
            .iter()
            .filter(|day| day.date == date)
            .map(|day| &day.usage)
            .collect();
        let cost: f64 = models
            .iter()
            .filter_map(|usage| usage.cost)
            .map(|cost| cost.total())
            .sum();
        let split = models
            .iter()
            .map(|usage| {
                format!(
                    "{} {}",
                    pricing::model_name(&usage.model),
                    estimated(usage.cost)
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        detail = detail
            .child(
                div()
                    .mt_1()
                    .pt_2()
                    .border_t_1()
                    .border_color(p.edge.opacity(edge::SEPARATOR))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(property("Est. cost", dollars(cost), p)),
            )
            .when(!split.is_empty(), |d| d.child(hint(split, p)));
        detail.child(hint(
            "Cache is part of input; reasoning is part of output.",
            p,
        ))
    }
    fn coordinator_usage(&self, report: &UsageReport, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut work = card(p).child(
            div()
                .flex()
                .justify_between()
                .child(eyebrow("BY COORDINATOR", p))
                .child(eyebrow("EST. COST", p)),
        );
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
                            .child(div().flex_none().child(compact(total)))
                            .child(
                                cell(estimated(coordinator.cost)).font_weight(FontWeight::SEMIBOLD),
                            ),
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
    /// Every model used in the period: its share of the cost, its tokens by kind and its cost.
    fn model_usage(report: &UsageReport, p: Palette) -> Div {
        let total = report.cost.total();
        let mut share = div()
            .w_full()
            .h(px(8.))
            .rounded(px(4.))
            .flex()
            .overflow_hidden()
            .bg(p.overlay);
        let mut table = div().flex().flex_col().text_size(px(font::SMALL)).child(
            div()
                .flex()
                .gap_2()
                .pb_1()
                .border_b_1()
                .border_color(p.edge.opacity(edge::SEPARATOR))
                .text_color(p.subtle)
                .child(div().flex_1().min_w_0().child("Model"))
                .children(
                    [
                        "Requests",
                        "Input",
                        "Cache read",
                        "Cache write",
                        "Output",
                        "Est. cost",
                        "Share",
                    ]
                    .map(cell),
                ),
        );
        let row = || {
            div()
                .flex()
                .items_center()
                .gap_2()
                .py_1p5()
                .border_b_1()
                .border_color(p.edge.opacity(edge::SEPARATOR))
        };
        let mut all = TokenTotals::default();
        for usage in &report.models {
            let color = model_color(&usage.model, p);
            let cost = usage.cost.map(|cost| cost.total());
            if let Some(cost) = cost.filter(|cost| *cost > 0.) {
                share = share.child(div().h_full().w(relative((cost / total) as f32)).bg(color));
            }
            all.add(&usage.counts);
            let parts = usage.cost.as_ref().map(cost_parts);
            table = table.child(
                row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(swatch(color))
                            .child(
                                div()
                                    .min_w_0()
                                    .child(pricing::model_name(&usage.model).to_owned())
                                    .child(hint(
                                        format!("{} · {}", usage.provider.label(), usage.model),
                                        p,
                                    )),
                            ),
                    )
                    .child(cell(number(usage.counts.requests)))
                    .child(cell(compact(usage.counts.uncached_input())))
                    .child(cell(compact(usage.counts.cache_read)))
                    // Codex reports no cache writes.
                    .child(cell(if usage.provider == Provider::Codex {
                        "—".into()
                    } else {
                        compact(usage.counts.cache_write)
                    }))
                    .child(cell(compact(usage.counts.output)))
                    .child(
                        div()
                            .id(SharedString::from(format!("model-cost-{}", usage.model)))
                            .child(cell(estimated(usage.cost)).font_weight(FontWeight::SEMIBOLD))
                            .when_some(parts, |d, parts| {
                                d.tooltip(move |w, c| Tooltip::new(parts.clone()).build(w, c))
                            }),
                    )
                    .child(cell(cost.map_or_else(
                        || "—".into(),
                        |cost| format!("{:.1}%", cost / total.max(f64::MIN_POSITIVE) * 100.),
                    ))),
            );
        }
        table = table.child(
            row()
                .border_b_0()
                .font_weight(FontWeight::SEMIBOLD)
                .child(div().flex_1().child("All models"))
                .child(cell(number(all.requests)))
                .child(cell(compact(all.uncached_input())))
                .child(cell(compact(all.cache_read)))
                .child(cell(compact(all.cache_write)))
                .child(cell(compact(all.output)))
                .child(cell(dollars(total)))
                .child(cell("")),
        );
        let mut card = card(p)
            .child(eyebrow("BY MODEL", p))
            .child(share)
            .child(table)
            .child(hint(
                "Input here is input that was neither read from nor written to the cache. Hover a cost to see its parts.",
                p,
            ));
        if report.unpriced_requests > 0 {
            card = card.child(
                hint(
                    format!(
                        "Left out of the total: {} request{} on models with no known price.",
                        number(report.unpriced_requests),
                        if report.unpriced_requests == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ),
                    p,
                )
                .text_color(p.yellow),
            );
        }
        card
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
                    ))
                    .child(Self::cost_tile(report, p)),
            )
            .child(Self::model_usage(report, p))
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
        .child(hint(
            format!(
                "Estimated cost prices each request at the public API list price of its model, as of {}. Your subscription is billed differently; this shows what the same work would cost on the API. Claude cache writes are priced by their lifetime (1 hour or 5 minutes) and fast-mode requests at fast-mode rates. A model with no known price shows “—” and is left out of the total.",
                report.prices_as_of
            ),
            p,
        ))
    }
    fn daily_chart(&self, report: &UsageReport, p: Palette, cx: &mut Context<Self>) -> Div {
        // Each day's models in the By model order, so the costliest sits at the bottom.
        let mut days: BTreeMap<&str, (Vec<&ModelUsage>, bool)> = BTreeMap::new();
        for day in &report.daily {
            days.entry(&day.date).or_default().1 |= day.covered;
        }
        for model in &report.models {
            for day in report.daily_models.iter().filter(|day| {
                day.usage.model == model.model && day.usage.provider == model.provider
            }) {
                days.entry(&day.date).or_default().0.push(&day.usage);
            }
        }
        let day_total =
            |models: &[&ModelUsage]| models.iter().map(|m| m.counts.total()).sum::<u64>();
        let max = days
            .values()
            .map(|(models, _)| day_total(models))
            .max()
            .unwrap_or(1)
            .max(1);
        let mut chart = div().w_full().h(px(150.)).flex().items_end().gap_1();
        let count = days.len();
        for (index, (date, (models, covered))) in days.into_iter().enumerate() {
            let mut tooltip = date.to_owned();
            for usage in &models {
                tooltip.push_str(&format!(
                    "\n{}: input {} · output {} · {}",
                    pricing::model_name(&usage.model),
                    number(usage.counts.input),
                    number(usage.counts.output),
                    estimated(usage.cost)
                ));
            }
            if !covered {
                tooltip.push_str("\nNo collection coverage");
            }
            let click_date = date.to_owned();
            let mut stack = div().w_full().flex().flex_col().justify_end().min_h(px(2.));
            for usage in models.iter().rev() {
                stack = stack.child(
                    div()
                        .w_full()
                        .h(px(usage.counts.total() as f32 / max as f32 * 118.))
                        .bg(model_color(&usage.model, p)),
                );
            }
            if day_total(&models) == 0 {
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
        let mut legend = div()
            .flex()
            .gap_x_4()
            .gap_y_1()
            .flex_wrap()
            .text_size(px(font::CAPTION))
            .text_color(p.subtle);
        for usage in &report.models {
            legend = legend.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(swatch(model_color(&usage.model, p)))
                    .child(pricing::model_name(&usage.model).to_owned()),
            );
        }
        card(p)
            .child(eyebrow("DAILY TOKENS BY MODEL", p))
            .child(hint(
                format!("Peak {} tokens · tap a day for exact totals", number(max)),
                p,
            ))
            .child(chart)
            .child(legend)
            .child(hint(
                "Each bar stacks the day's tokens by model · Baseline: zero or no coverage",
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
            &self.shared.host,
            p,
            cx,
        )
    }
}
