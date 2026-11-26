//! Settings › Remote access, at the host itself: the switch, this Mac's name and addresses,
//! pairing a device with its code or a phone with a QR code and the time left, and the paired
//! devices with what each can do and Remove. Only the host's own app shows it; remote devices are
//! refused these commands.
use super::*;
use chrono::{Local, TimeZone};
use gpui_component::{button::ButtonVariants, switch::Switch};
use std::time::Duration;
use workspace_core::PairingQr;

/// The QR code of the pairing mode it was read for, as dark and light modules.
pub(super) struct PhoneQr {
    /// When that pairing mode closes; another one has another secret.
    expires_at: i64,
    width: usize,
    dark: Rc<Vec<bool>>,
}
impl PhoneQr {
    fn new(qr: &PairingQr) -> Result<Self, String> {
        let text = serde_json::to_string(&qr.payload).map_err(|e| e.to_string())?;
        let code = qrcode::QrCode::new(text).map_err(|e| e.to_string())?;
        let dark = code.to_colors().into_iter();
        Ok(Self {
            expires_at: qr.expires_at,
            width: code.width(),
            dark: Rc::new(dark.map(|color| color == qrcode::Color::Dark).collect()),
        })
    }
}

/// A module's side, and the light border scanners need, in modules.
const QR_MODULE: f32 = 4.;
const QR_QUIET: usize = 4;

/// Dark modules on white whatever the theme, as cameras read best.
fn qr_image(qr: &PhoneQr) -> Div {
    let side = (qr.width + 2 * QR_QUIET) as f32 * QR_MODULE;
    let (width, dark) = (qr.width, qr.dark.clone());
    let modules = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let module = size(px(QR_MODULE), px(QR_MODULE));
            for at in (0..dark.len()).filter(|at| dark[*at]) {
                let (x, y) = (at % width + QR_QUIET, at / width + QR_QUIET);
                let offset = point(px(x as f32 * QR_MODULE), px(y as f32 * QR_MODULE));
                window.paint_quad(fill(Bounds::new(bounds.origin + offset, module), black()));
            }
        },
    )
    .size_full();
    div()
        .size(px(side))
        .flex_none()
        .rounded_md()
        .bg(white())
        .child(modules)
}

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

/// A run of a step's text, bold when `true`.
type Run<'a> = (&'a str, bool);

fn steps_before_code() -> [Vec<Run<'static>>; 3] {
    [
        vec![
            ("Click ", false),
            ("Pair a device", true),
            (" here. This Mac shows a 4-digit code.", false),
        ],
        vec![
            ("On the other Mac, open Wiffletree and choose ", false),
            ("Connect a computer…", true),
            (" from the sidebar's host menu.", false),
        ],
        vec![
            ("Pick ", false),
            ("this Mac", true),
            (
                " from the list, or type its name or address, and enter the code.",
                false,
            ),
        ],
    ]
}

fn steps_with_code<'a>(
    name: &'a str,
    address: Option<&'a str>,
    code: &'a str,
) -> [Vec<Run<'a>>; 3] {
    let mut pick = vec![("Pick ", false), (name, true)];
    if let Some(address) = address {
        pick.extend([(", or type ", false), (address, true)]);
    }
    pick.push((".", false));
    [
        vec![
            ("On the other Mac, open Wiffletree › ", false),
            ("Connect a computer…", true),
            (" in the sidebar's host menu.", false),
        ],
        pick,
        vec![
            ("Enter ", false),
            (code, true),
            (". It works once, for 2 minutes.", false),
        ],
    ]
}

fn steps_with_qr() -> [Vec<Run<'static>>; 3] {
    [
        vec![
            ("On the iPhone, open Wiffletree and tap ", false),
            ("Scan QR code", true),
            (".", false),
        ],
        vec![(
            "Point it at this code. It works once, for 2 minutes.",
            false,
        )],
        vec![(
            "Away from home, the iPhone reaches this Mac over Tailscale or a VPN you already use.",
            false,
        )],
    ]
}

/// Numbered steps in hint text, their bold runs in the text color.
fn steps(steps: &[Vec<Run>], p: Palette) -> Div {
    let bold = HighlightStyle {
        font_weight: Some(FontWeight::SEMIBOLD),
        color: Some(p.text),
        ..Default::default()
    };
    let rows = steps.iter().enumerate().map(|(at, runs)| {
        let (mut text, mut highlights) = (String::new(), vec![]);
        for (run, is_bold) in runs {
            if *is_bold {
                highlights.push((text.len()..text.len() + run.len(), bold));
            }
            text.push_str(run);
        }
        div()
            .flex()
            .gap_2()
            .child(div().flex_none().child(format!("{}.", at + 1)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(StyledText::new(text).with_highlights(highlights)),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(font::SMALL))
        .line_height(relative(1.5))
        .text_color(p.subtle)
        .children(rows)
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
                self.opening_pairing = false;
                cx.notify();
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let answer = answer.await;
            let _ = this.update(cx, |view, cx| {
                view.opening_pairing = false;
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

    /// Opens pairing mode, a new one when one is open, and shows its QR code.
    fn connect_phone(&mut self, cx: &mut Context<Self>) {
        let opened = match self.bridge.ask(ask::open_pairing()) {
            Ok(opened) => opened,
            Err(error) => {
                self.remote_error = Some(error);
                cx.notify();
                return;
            }
        };
        // The host takes commands in the order they were sent, so this reads the new mode's code.
        let qr = self.bridge.ask(ask::pairing_qr());
        self.opening_qr = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let opened = opened.await;
            let qr = match qr {
                Ok(qr) => qr.await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |view, cx| {
                view.opening_qr = false;
                let shown = opened.and_then(|remote| Ok((remote, PhoneQr::new(&qr?)?)));
                match shown {
                    Ok((remote, qr)) => {
                        view.remote = Some(remote);
                        view.phone_qr = Some(qr);
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
        let card = card(p).child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Pair a device"),
        );
        let opening = |id: &'static str| {
            button(id)
                .disabled(true)
                .child(ui::Spinner::new().small())
                .child("Opening pairing…")
        };
        let Some(pairing) = &remote.pairing else {
            let open = match self.opening_pairing {
                true => opening("open-pairing"),
                false => button("open-pairing")
                    .label("Pair a device")
                    .disabled(self.opening_qr)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.opening_pairing = true;
                        view.ask_remote(ask::open_pairing(), cx);
                        cx.notify();
                    })),
            };
            let phone = match self.opening_qr {
                true => opening("connect-phone"),
                false => button("connect-phone")
                    .label("Connect a phone")
                    .disabled(self.opening_pairing)
                    .on_click(cx.listener(|view, _, _, cx| view.connect_phone(cx))),
            };
            return card
                .child(steps(&steps_before_code(), p))
                .child(hint(
                    "For an iPhone, click Connect a phone and scan its QR code with Wiffletree on the iPhone.",
                    p,
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(open.primary())
                        .child(phone.outline()),
                );
        };
        let waiting = div().flex().items_center().gap_1().child(hint(
            format!(
                "Waiting for a device… · {} ·",
                time_left(pairing.expires_at, ui::now())
            ),
            p,
        ));
        let cancel = button("cancel-pairing")
            .ghost()
            .xsmall()
            .label("Cancel")
            .on_click(cx.listener(|view, _, _, cx| {
                view.ask_remote(ask::cancel_pairing(), cx);
            }));
        let shown_qr = (self.phone_qr.as_ref()).filter(|qr| qr.expires_at == pairing.expires_at);
        if let Some(qr) = shown_qr {
            let new_code = match self.opening_qr {
                true => opening("new-qr").ghost().xsmall(),
                false => button("new-qr")
                    .ghost()
                    .xsmall()
                    .label("New code")
                    .on_click(cx.listener(|view, _, _, cx| view.connect_phone(cx))),
            };
            return card
                .child(div().flex().justify_center().child(qr_image(qr)))
                .child(steps(&steps_with_qr(), p))
                .child(waiting.child(new_code).child(cancel));
        }
        // The new pairing mode can show before its QR code arrives.
        if self.opening_qr {
            return card.child(hint("Opening pairing…", p));
        }
        let address = remote.addresses.first().map(String::as_str);
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
                .font_family(mono_font())
                .text_size(px(24.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(digit.to_string())
        });
        card.child(div().flex().justify_center().gap_2().children(digits))
            .child(steps(
                &steps_with_code(&remote.host_name, address, &pairing.code),
                p,
            ))
            .child(waiting.child(cancel))
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
    use super::{PhoneQr, steps_with_code, time_left};
    use workspace_core::{PairingQr, QrPayload};

    #[test]
    fn a_qr_code_with_every_kind_of_address_stays_small_enough_to_scan() {
        let qr = PairingQr {
            payload: QrPayload {
                host_key: "B".repeat(88),
                host_name: "Michael's Studio Mac Pro".into(),
                addresses: ["192.168.1.20", "10.0.0.5", "100.101.7.9"]
                    .map(String::from)
                    .into_iter()
                    .chain(["michaels-studio-mac-pro.tail4c2e.ts.net".to_string()])
                    .collect(),
                port: 47821,
                secret: "A".repeat(24),
            },
            expires_at: 120_000,
        };
        let shown = PhoneQr::new(&qr).unwrap();
        assert_eq!(shown.dark.len(), shown.width * shown.width);
        // Version 13 or lower: 69 modules, drawn 4 points each.
        assert!(shown.width <= 69, "{}", shown.width);
    }

    #[test]
    fn time_left_counts_whole_seconds_up() {
        assert_eq!(time_left(102_000, 0), "1:42 left");
        assert_eq!(time_left(1_500, 1_000), "0:01 left");
        assert_eq!(time_left(0, 1_000), "0:00 left");
    }

    #[test]
    fn the_code_steps_name_an_address_only_when_there_is_one() {
        let text = |address| -> Vec<String> {
            let steps = steps_with_code("Studio Mac", address, "4719");
            let joined = steps
                .iter()
                .map(|runs| runs.iter().map(|(run, _)| *run).collect());
            joined.collect()
        };
        assert_eq!(
            text(Some("studio.local"))[1],
            "Pick Studio Mac, or type studio.local."
        );
        assert_eq!(text(None)[1], "Pick Studio Mac.");
        assert_eq!(text(None)[2], "Enter 4719. It works once, for 2 minutes.");
    }
}
