//! Settings › Remote access, at the host itself: the switch, this Mac's name and addresses,
//! pairing a device with its code and the time left, and the paired devices with what each can
//! do and Remove. Only the host's own app shows it; remote devices are refused these commands.
use super::*;
use chrono::{Local, TimeZone};
use gpui_component::{button::ButtonVariants, switch::Switch};
use std::time::Duration;

/// How often the page looks again while pairing mode counts down, and otherwise while remote
/// access is on, so a device that pairs or connects shows up.
const PAIRING_TICK: Duration = Duration::from_secs(1);
const WATCH_TICK: Duration = Duration::from_secs(5);

const EXPLANATION: &str = "Control means the device can start and message agents here. Agents run with permission prompts off, so that is the same as running commands on this Mac. Remove takes effect at once, even if the device is offline.";

/// "1:42 left" for the milliseconds until `expires_at`.
fn time_left(expires_at: i64, now: i64) -> String {
    let seconds = ((expires_at - now).max(0) + 999) / 1000;
    format!("{}:{:02} left", seconds / 60, seconds % 60)
}

/// "Connected now", when it last connected, or "Never".
fn last_connected(device: &PairedDevice) -> String {
    if device.connected {
        return "Connected now".into();
    }
    (device.last_connected_at)
        .and_then(|at| Local.timestamp_millis_opt(at).single())
        .map_or_else(
            || "Never".into(),
            |at| wording::when(at, Local::now().date_naive()),
        )
}

impl Settings {
    /// Shows the host's answer, or its error, and keeps the page current while it matters.
    pub(super) fn ask_remote(&mut self, ask: Ask<RemoteAccess>, cx: &mut Context<Self>) {
        let answer = match self.bridge.ask(ask) {
            Ok(answer) => answer,
            Err(error) => {
                self.remote_error = Some(error);
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |view, cx| {
                match answer {
                    Ok(remote) => {
                        view.remote = Some(remote);
                        view.remote_error = None;
                    }
                    Err(error) => view.remote_error = Some(error),
                }
                view.watch_remote(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Looks again every second while pairing mode is open and every few seconds while remote
    /// access is on, until neither is so or the window closes.
    fn watch_remote(&mut self, cx: &mut Context<Self>) {
        if self.watching_remote {
            return;
        }
        self.watching_remote = true;
        cx.spawn(async move |this, cx| {
            loop {
                let tick = this.update(cx, |view, _| {
                    let remote = view.remote.as_ref()?;
                    match (&remote.pairing, remote.enabled) {
                        (Some(_), _) => Some(PAIRING_TICK),
                        (None, true) => Some(WATCH_TICK),
                        (None, false) => None,
                    }
                });
                let Ok(Some(tick)) = tick else { break };
                cx.background_executor().timer(tick).await;
                let asked = this.update(cx, |view, _| view.bridge.ask(ask::remote_access()));
                let Ok(Ok(answer)) = asked else { break };
                let answer = answer.await;
                let updated = this.update(cx, |view, cx| {
                    if let Ok(remote) = answer {
                        view.remote = Some(remote);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |view, _| view.watching_remote = false);
        })
        .detach();
    }

    pub(super) fn remote_access_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let section_body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section("REMOTE ACCESS", p));
        let Some(remote) = &self.remote else {
            return section_body.children(
                (self.remote_error.clone()).map(|error| hint(error, p).text_color(p.red)),
            );
        };
        let switch = Switch::new("remote-access")
            .checked(remote.enabled)
            .on_click(cx.listener(|view, enabled: &bool, _, cx| {
                view.ask_remote(ask::set_remote_access(*enabled), cx);
            }));
        let mut section_body = section_body.child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child("Let other devices use this Mac's Wiffletree")
                        .child(hint(
                            "Off by default. While on, paired devices can see this Mac's projects and, if you allow it, run its agents.",
                            p,
                        )),
                )
                .child(switch),
        );
        if remote.enabled {
            let reachable = match remote.addresses.as_slice() {
                [] => String::new(),
                addresses => format!(" · reachable at {}", addresses.join(" and ")),
            };
            section_body = section_body.child(hint(
                format!("This Mac is called {}{reachable}", remote.host_name),
                p,
            ));
            section_body = section_body.child(self.pairing_card(remote, p, cx));
        }
        let error = (remote.error.clone()).or_else(|| self.remote_error.clone());
        section_body = section_body.children(error.map(|error| hint(error, p).text_color(p.red)));
        if !remote.devices.is_empty() {
            section_body = section_body.child(self.devices_card(&remote.devices, p, cx));
        }
        if remote.enabled || !remote.devices.is_empty() {
            section_body = section_body.child(hint(EXPLANATION, p));
        }
        section_body
    }

    fn pairing_card(&self, remote: &RemoteAccess, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(pairing) = &remote.pairing else {
            return card(p).items_center().child(
                button("open-pairing")
                    .label("Pair a device")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.ask_remote(ask::open_pairing(), cx);
                    })),
            );
        };
        let card = card(p).items_center().child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Pair a device"),
        );
        let digits = pairing.code.chars().map(|digit| {
            div()
                .w(px(36.))
                .py_1()
                .flex()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(p.edge.opacity(edge::CARD))
                .bg(p.surface)
                .font_family(".SystemMonoFont")
                .text_size(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(digit.to_string())
        });
        card.child(hint(
            "Type this code on the other Mac or iPhone. It works once, for 2 minutes.",
            p,
        ))
        .child(div().flex().gap_2().children(digits))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(hint(
                    format!(
                        "Waiting for a device… · {} ·",
                        time_left(pairing.expires_at, ui::now())
                    ),
                    p,
                ))
                .child(
                    button("cancel-pairing")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.ask_remote(ask::cancel_pairing(), cx);
                        })),
                ),
        )
    }

    fn devices_card(&self, devices: &[PairedDevice], p: Palette, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .gap_3()
            .text_size(px(font::SMALL))
            .text_color(p.subtle)
            .child(div().flex_1().child("Device"))
            .child(div().w(px(150.)).child("Can"))
            .child(div().w(px(120.)).child("Last connected"))
            .child(div().w(px(64.)));
        let rows = devices.iter().map(|device| {
            let levels = [AccessLevel::Control, AccessLevel::ViewOnly].map(|level| {
                let id = device.id.clone();
                segment(
                    SharedString::from(format!("level-{}-{:?}", device.id, level)),
                    level.label(),
                    device.level == level,
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.ask_remote(ask::set_device_level(id.clone(), level), cx);
                }))
            });
            let id = device.id.clone();
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().flex_1().min_w_0().child(device.name.clone()))
                .child(div().w(px(150.)).flex().gap_1().children(levels))
                .child(
                    div()
                        .w(px(120.))
                        .text_size(px(font::SMALL))
                        .child(last_connected(device)),
                )
                .child(
                    div().w(px(64.)).flex().justify_end().child(
                        button(SharedString::from(format!("remove-{}", device.id)))
                            .danger()
                            .xsmall()
                            .label("Remove")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.ask_remote(ask::remove_device(id.clone()), cx);
                            })),
                    ),
                )
        });
        card(p)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Paired devices"),
            )
            .child(header)
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::time_left;

    #[test]
    fn time_left_counts_whole_seconds_up() {
        assert_eq!(time_left(102_000, 0), "1:42 left");
        assert_eq!(time_left(1_500, 1_000), "0:01 left");
        assert_eq!(time_left(0, 1_000), "0:00 left");
    }
}
