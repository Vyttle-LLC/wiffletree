//! Connect a computer (design 4 and 6 of `remote-hosts`, study 3 of its mockup): the Wiffletree
//! hosts browsing finds on this network while finding is on, or one typed by name or address,
//! then the 4 digits the host shows. A paired host joins the switcher and stays there.
use super::*;
use crate::{browse::Found, hosts::PairedHost};
use base64::Engine as _;
use gpui_component::{
    Disableable,
    button::ButtonVariants,
    input::{Input, InputState},
};
use ui::{hint, section};
use workspace_core::{
    pairing,
    remote_client::{self, RemoteError},
};

/// Where the sheet is.
#[derive(Clone, PartialEq)]
enum Step {
    Choose,
    /// The code for the host at `address`, named `name` as far as the sheet knows.
    Code {
        address: String,
        name: String,
    },
}

pub(super) struct ConnectSheet {
    step: Step,
    address: Entity<InputState>,
    code: Entity<InputState>,
    found: Vec<Found>,
    pairing: bool,
    error: Option<String>,
}

/// Opens Connect a computer over `window`.
pub(super) fn open(window: &mut Window, cx: &mut App) {
    let sheet = cx.new(|cx| ConnectSheet::new(window, cx));
    window.open_dialog(cx, move |dialog, _, cx| {
        let title = sheet.read(cx).title();
        dialog.title(title).width(px(480.)).child(sheet.clone())
    });
}

impl ConnectSheet {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("studio.tail4c2e.ts.net"));
        let code = cx.new(|cx| InputState::new(window, cx).placeholder("4 digits"));
        // The list follows what browsing finds while the sheet is open.
        cx.spawn(async move |this, cx| {
            loop {
                let shown = this.update(cx, |sheet, cx| {
                    let found = browsed(cx);
                    if found != sheet.found {
                        sheet.found = found;
                        cx.notify();
                    }
                });
                if shown.is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
            }
        })
        .detach();
        Self {
            step: Step::Choose,
            address,
            code,
            found: browsed(cx),
            pairing: false,
            error: None,
        }
    }
    fn title(&self) -> String {
        match &self.step {
            Step::Choose => "Connect a computer".into(),
            Step::Code { name, .. } => format!("Pair with {name}"),
        }
    }
    fn ask_code(
        &mut self,
        address: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step = Step::Code { address, name };
        self.error = None;
        self.code.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        window.refresh();
        cx.notify();
    }
    fn typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let address = self.address.read(cx).value().trim().to_owned();
        if !address.is_empty() {
            let name = remote_client::split_address(&address).0;
            self.ask_code(address, name, window, cx);
        }
    }
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::Choose;
        self.error = None;
        window.refresh();
        cx.notify();
    }
    /// Runs the exchange off the main thread; a paired host joins the switcher and is shown.
    fn pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Code { address, name } = self.step.clone() else {
            return;
        };
        let code = self.code.read(cx).value().trim().to_owned();
        if !pairing::is_code(&code) {
            self.error = Some(format!("Type the 4 digits {name} is showing."));
            return cx.notify();
        }
        let device = match cx.global_mut::<Hosts>().device() {
            Ok(device) => device,
            Err(error) => {
                self.error = Some(error);
                return cx.notify();
            }
        };
        self.pairing = true;
        self.error = None;
        cx.notify();
        let (sender, receiver) = async_channel::bounded(1);
        let tried = address.clone();
        std::thread::spawn(move || {
            let paired = remote_client::pair(&tried, &code, &device, &hosts::this_mac_name());
            let _ = sender.send_blocking(paired);
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(paired) = receiver.recv().await else {
                return;
            };
            let _ = this.update_in(cx, |sheet, window, cx| {
                sheet.pairing = false;
                match paired {
                    Ok(host) => paired_with(host, address, window, cx),
                    Err(error) => {
                        sheet.error = Some(failure(&error, &name));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

/// The hosts browsing has found, by name.
fn browsed(cx: &App) -> Vec<Found> {
    let browsed = cx.global::<Hosts>().browsed.clone();
    browsed.keep_fresh();
    let mut found = browsed.found();
    found.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
    found
}

/// While finding is off, the sheet says so, and Turn it on flips the same setting as Settings.
fn finding_off(p: Palette) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(hint("Finding Wiffletree on this network is off.", p))
        .child(
            button("find-on-network")
                .link()
                .label("Turn it on")
                .on_click(|_, _, cx| set_find_on_network(true, cx)),
        )
        .child(hint(
            "to list the Macs nearby, or type a name or address.",
            p,
        ))
}

/// What a failed pairing says.
fn failure(error: &RemoteError, host: &str) -> String {
    match error {
        RemoteError::Confirmation => format!(
            "That code didn't match. Check the 4 digits {host} shows; pairing closes there after 3 wrong tries."
        ),
        RemoteError::Refused(reason) => reason.clone(),
        error => format!("Couldn't pair with {host}: {error}"),
    }
}

/// Keeps the host in this device's client settings and the switcher, and shows it.
fn paired_with(
    host: remote_client::PairedHost,
    address: String,
    window: &mut Window,
    cx: &mut App,
) {
    let paired = PairedHost {
        id: host.id.clone(),
        public_key: base64::engine::general_purpose::STANDARD.encode(&host.public_key),
        name: host.name.clone(),
        addresses: vec![address],
        last_seen: Some(ui::now()),
    };
    let preferences = cx.global_mut::<Preferences>();
    preferences.paired_hosts.retain(|h| h.id != paired.id);
    preferences.paired_hosts.push(paired.clone());
    if let Err(error) = preferences.save() {
        eprintln!("Saving preferences: {error}");
    }
    cx.global_mut::<Hosts>().add(paired);
    let bridge = (cx.global::<Hosts>().remotes.iter())
        .find(|r| r.paired.id == host.id)
        .map(|r| r.bridge.clone());
    if let Some(bridge) = bridge {
        hosts::follow(host.id.clone(), bridge, cx);
    }
    window.close_dialog(cx);
    toast(format!("Paired with {}", host.name), window, cx);
    // The host's connection is new, so a window that showed it is built again on it.
    show_host(HostChoice::Remote(host.id), true, window, cx);
}

impl Render for ConnectSheet {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let body = div().flex().flex_col().gap_3();
        let body = match self.step.clone() {
            Step::Choose => self.choose(body, p, cx),
            Step::Code { name, .. } => self.code_step(body, &name, p, cx),
        };
        body.children(
            self.error
                .clone()
                .map(|error| hint(error, p).text_color(p.red)),
        )
    }
}

impl ConnectSheet {
    fn choose(&self, body: Div, p: Palette, cx: &mut Context<Self>) -> Div {
        let nearby = match cx.global::<Preferences>().find_on_network {
            true => self.nearby(p, cx),
            false => finding_off(p),
        };
        body.child(nearby).child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section("BY NAME OR ADDRESS", p))
                .child(hint(
                    "Not listed, or on Tailscale or a VPN? Enter its name or address.",
                    p,
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(Input::new(&self.address).flex_1())
                        .child(
                            button("pair-typed")
                                .label("Pair")
                                .on_click(cx.listener(|sheet, _, w, c| sheet.typed(w, c))),
                        ),
                ),
        )
    }
    /// The hosts browsing finds, each with Pair while its pairing is open.
    fn nearby(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut nearby = div().flex().flex_col().gap_2().child(hint(
            "Wiffletree hosts on this network. Turn on pairing on the other Mac first.",
            p,
        ));
        if self.found.is_empty() {
            nearby = nearby.child(hint("Looking for hosts…", p));
        }
        for (index, found) in self.found.iter().enumerate() {
            let detail = match found.pairing {
                true => format!("Pairing now · Wiffletree {}", found.version),
                false => "Pairing is off there".into(),
            };
            let (address, name) = (found.address.clone(), found.name.clone());
            nearby = nearby.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(found.name.clone())
                            .child(hint(detail, p)),
                    )
                    .when(found.pairing, |row| {
                        row.child(button(("pair-found", index)).label("Pair").on_click(
                            cx.listener(move |sheet, _, window, cx| {
                                sheet.ask_code(address.clone(), name.clone(), window, cx)
                            }),
                        ))
                    }),
            );
        }
        nearby
    }
    fn code_step(&self, body: Div, name: &str, p: Palette, cx: &mut Context<Self>) -> Div {
        body.child(hint(format!("Type the 4 digits {name} is showing."), p))
            .child(Input::new(&self.code))
            .child(hint(
                format!(
                    "{name}'s key will be remembered. If it ever changes, Wiffletree will refuse to connect and ask you to pair again."
                ),
                p,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("cancel-pairing")
                            .ghost()
                            .label("Cancel")
                            .disabled(self.pairing)
                            .on_click(cx.listener(|sheet, _, w, c| sheet.back(w, c))),
                    )
                    .child(
                        ui::icon_or_spinner(
                            button("pair-code").primary(),
                            ui::icon("check"),
                            Some("Pair"),
                            self.pairing,
                            gpui_component::Size::Small,
                        )
                        .disabled(self.pairing)
                        .on_click(cx.listener(|sheet, _, w, c| sheet.pair(w, c))),
                    ),
            )
    }
}
