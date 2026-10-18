//! Account limits: the sidebar's strip of 5-hour and weekly pills, and how a reading is worded
//! and colored there and on the Usage page.
use super::*;
use gpui_component::tooltip::Tooltip;
use ui::now;
use wording::age;

pub(super) fn reading_age(at: i64) -> String {
    if at == 0 {
        "No reading".into()
    } else {
        age(at)
    }
}
pub(super) fn quota_color(used: f64, p: Palette) -> Hsla {
    if used >= 90. {
        p.red
    } else if used >= 75. {
        p.yellow
    } else {
        p.focus
    }
}
pub(super) fn remaining(window: &QuotaWindow) -> String {
    match window.used_percent {
        Some(used) => format!("{:.0}% left", (100. - used).clamp(0., 100.)),
        None => "Unavailable".into(),
    }
}
pub(super) fn stale(window: &QuotaWindow) -> bool {
    now() - window.observed_at > 15 * 60_000 || window.resets_at.is_some_and(|t| t <= now())
}
pub(super) fn reset(window: &QuotaWindow) -> Option<String> {
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
