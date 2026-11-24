//! Settings › Screen control (screen 2 of `ui-evaluators`' study, as its spec amends it):
//! the Screen Recording and Accessibility grants of Wiffletree Evaluator, the helper app that
//! alone drives the screen for full-desktop evaluators. Each row is read live from the copy the
//! host installs in its data folder, with the helper's own `check`, which never prompts; macOS
//! charges a helper nested in Wiffletree.app to Wiffletree, so that copy is never asked.
use super::*;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The data folder the app and This Mac's host share, which holds the installed helper.
pub(crate) struct DataFolder(pub(crate) PathBuf);
impl Global for DataFolder {}

/// The helper's grants, as its `check` and `request` print them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(super) struct Grants {
    pub(super) screen_recording: bool,
    pub(super) accessibility: bool,
}

/// What the page last learned of the helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Reading {
    Checking,
    Read(Grants),
    /// No helper where the host installs it.
    Missing(PathBuf),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grant {
    ScreenRecording,
    Accessibility,
}
impl Grant {
    const ALL: [Self; 2] = [Self::ScreenRecording, Self::Accessibility];
    fn name(self) -> &'static str {
        match self {
            Self::ScreenRecording => "Screen Recording",
            Self::Accessibility => "Accessibility",
        }
    }
    fn purpose(self) -> &'static str {
        match self {
            Self::ScreenRecording => "Lets an evaluator see the apps its run names.",
            Self::Accessibility => "Lets an evaluator click, type and scroll in them.",
        }
    }
    /// The System Settings pane that holds this grant, opened at its list.
    pub(super) fn pane(self) -> &'static str {
        match self {
            Self::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        }
    }
    fn granted(self, grants: Grants) -> bool {
        match self {
            Self::ScreenRecording => grants.screen_recording,
            Self::Accessibility => grants.accessibility,
        }
    }
}

/// How long `check` may take to print; `request` waits up to two minutes for the human.
const CHECK_WAIT: Duration = Duration::from_secs(15);
const REQUEST_WAIT: Duration = Duration::from_secs(130);

/// The grants in what the helper printed: its last line of JSON.
pub(super) fn parse_grants(printed: &str) -> Option<Grants> {
    let line = printed.lines().rev().find(|line| !line.trim().is_empty())?;
    serde_json::from_str(line).ok()
}

/// Runs the helper at `helper` in `mode` through LaunchServices, so that it is its own
/// responsible process and macOS reads its own grants, and reads what it prints. `open -W`
/// often cannot wait for so short-lived an app, so its output goes to a file read until it
/// holds the grants.
fn run_helper(helper: &Path, mode: &str, wait: Duration) -> Result<Grants, String> {
    let printed = std::env::temp_dir().join(format!(
        "wiffletree-evaluator-{mode}-{}.json",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&printed, "").map_err(|e| e.to_string())?;
    let opened = std::process::Command::new("/usr/bin/open")
        .args(["-n", "-g", "--stdout"])
        .arg(&printed)
        .arg(helper)
        .args(["--args", mode])
        .output();
    let read = || {
        let started = Instant::now();
        loop {
            let text = std::fs::read_to_string(&printed).unwrap_or_default();
            if let Some(grants) = parse_grants(&text) {
                return Ok(grants);
            }
            if started.elapsed() > wait {
                return Err("Wiffletree Evaluator didn't answer. Check again.".to_owned());
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    };
    let result = match opened {
        Ok(output) if output.status.success() => read(),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    };
    let _ = std::fs::remove_file(&printed);
    result
}

/// The installed helper, which this Mac's host runs for every full-desktop run.
fn helper(cx: &App) -> Option<PathBuf> {
    let data = &cx.try_global::<DataFolder>()?.0;
    Some(helper_app(data))
}

impl Settings {
    /// Reads the helper's grants without asking for them.
    pub(super) fn check_grants(&mut self, cx: &mut Context<Self>) {
        self.read_grants("check", CHECK_WAIT, cx);
    }
    /// Asks macOS for both grants, which shows its prompts, and waits for the human.
    fn request_grants(&mut self, cx: &mut Context<Self>) {
        self.requesting = true;
        self.read_grants("request", REQUEST_WAIT, cx);
    }
    fn read_grants(&mut self, mode: &'static str, wait: Duration, cx: &mut Context<Self>) {
        let Some(helper) = helper(cx) else {
            return;
        };
        if !helper.exists() {
            self.screen = Reading::Missing(helper);
            self.requesting = false;
            return cx.notify();
        }
        if mode == "check" {
            self.screen = Reading::Checking;
        }
        cx.notify();
        let reading = cx
            .background_executor()
            .spawn(async move { run_helper(&helper, mode, wait) });
        cx.spawn(async move |this, cx| {
            let reading = reading.await;
            let _ = this.update(cx, |view, cx| {
                view.screen = match reading {
                    Ok(grants) => Reading::Read(grants),
                    Err(error) => Reading::Failed(error),
                };
                view.requesting = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn screen_control_section(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section("SCREEN CONTROL", p))
            .child(hint(
                "Full-desktop evaluators run inside Wiffletree Evaluator, a small helper app, so only evaluators get these permissions and your other agents never do.",
                p,
            ));
        if let Some(host) = self.driving.name() {
            return body.child(hint(
                format!("Full-desktop evaluators use the screen of {host}; set this up there."),
                p,
            ));
        }
        for grant in Grant::ALL {
            body = body.child(self.grant_row(grant, p));
        }
        let note = match &self.screen {
            Reading::Missing(path) => Some(format!(
                "Wiffletree Evaluator isn't installed at {}. The host installs it when it starts from Wiffletree.app.",
                path.display()
            )),
            Reading::Failed(error) => Some(error.clone()),
            Reading::Checking | Reading::Read(_) => None,
        };
        let installed = !matches!(self.screen, Reading::Missing(_));
        let shown = helper(cx);
        body.children(note.map(|note| hint(note, p).text_color(p.yellow)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button("check-grants")
                            .label("Check again")
                            .disabled(self.requesting)
                            .on_click(cx.listener(|view, _, _, cx| view.check_grants(cx))),
                    )
                    .child(
                        button("request-grants")
                            .label(if self.requesting {
                                "Waiting for you…"
                            } else {
                                "Request access"
                            })
                            .disabled(self.requesting || !installed)
                            .on_click(cx.listener(|view, _, _, cx| view.request_grants(cx))),
                    )
                    .child(
                        button("show-helper")
                            .label("Show in Finder")
                            .disabled(!installed)
                            .on_click(move |_, _, cx| {
                                if let Some(path) = &shown {
                                    cx.reveal_path(path);
                                }
                            }),
                    ),
            )
            .child(hint(
                "Request access asks macOS for both and waits up to 2 minutes for your answer. To grant by hand, press + in the pane and choose the Wiffletree Evaluator that Show in Finder reveals, in Wiffletree's data folder; the one inside Wiffletree.app would be checked as Wiffletree.",
                p,
            ))
            .child(check_row(
                p.green,
                "App only",
                "Works for apps with an automation socket. Today that means Wiffletree. It needs no permission here.",
                div().text_color(p.green).child("Ready").into_any_element(),
                p,
            ))
    }

    fn grant_row(&self, grant: Grant, p: Palette) -> Div {
        let granted = match &self.screen {
            Reading::Read(grants) => Some(grant.granted(*grants)),
            _ => None,
        };
        let (color, status) = match granted {
            Some(true) => (
                p.green,
                div()
                    .text_color(p.green)
                    .child("Granted")
                    .into_any_element(),
            ),
            Some(false) => (
                p.yellow,
                button(SharedString::from(format!("open-pane-{}", grant.name())))
                    .label("Open System Settings…")
                    .on_click(move |_, _, cx| cx.open_url(grant.pane()))
                    .into_any_element(),
            ),
            None => (
                p.subtle,
                hint(
                    match self.screen {
                        Reading::Checking => "Checking…",
                        _ => "Unknown",
                    },
                    p,
                )
                .into_any_element(),
            ),
        };
        check_row(color, grant.name(), grant.purpose(), status, p)
    }
}

/// One live check: its dot, its name with what it is for, and its state or the way to fix it.
fn check_row(
    color: Hsla,
    name: &'static str,
    purpose: &'static str,
    status: AnyElement,
    p: Palette,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .py_1p5()
        .border_b_1()
        .border_color(p.edge.opacity(edge::HAIRLINE))
        .child(div().flex_none().size(px(8.)).rounded_full().bg(color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(name)
                .child(hint(purpose, p)),
        )
        .child(div().flex_none().child(status))
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{Grant, Grants, parse_grants};

    #[test]
    fn the_helpers_printed_grants_are_read_from_its_last_line() {
        let printed = "starting\n{\"screen_recording\":true,\"accessibility\":false}\n\n";
        assert_eq!(
            parse_grants(printed),
            Some(Grants {
                screen_recording: true,
                accessibility: false
            })
        );
        // Nothing yet, or not its JSON: keep waiting.
        assert_eq!(parse_grants(""), None);
        assert_eq!(parse_grants("{\"screen_recording\":true"), None);
    }

    #[test]
    fn each_grant_opens_its_own_privacy_pane() {
        assert!(
            Grant::ScreenRecording
                .pane()
                .ends_with("?Privacy_ScreenCapture")
        );
        assert!(
            Grant::Accessibility
                .pane()
                .ends_with("?Privacy_Accessibility")
        );
        let grants = Grants {
            screen_recording: false,
            accessibility: true,
        };
        assert!(!Grant::ScreenRecording.granted(grants));
        assert!(Grant::Accessibility.granted(grants));
    }
}
