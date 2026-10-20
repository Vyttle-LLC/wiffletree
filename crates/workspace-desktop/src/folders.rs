//! Choosing folders on the host the window shows (design 8 of `remote-hosts`): this Mac's open
//! panel for This Mac, and for a remote host an in-app browser over `ListFolder`, starting at its
//! home folder, so a folder chosen is always the host's, never one of the same path here.
use super::*;
use gpui_component::{Disableable, button::ButtonVariants};
use std::collections::BTreeSet;
use ui::{hint, icon, mono, pill};

/// The folders chosen, or none when the human cancelled.
pub(super) type Chosen = async_channel::Receiver<Vec<PathBuf>>;

/// Asks for folders on `bridge`'s host, with `prompt` as the panel's title.
pub(super) fn choose(
    bridge: &Bridge,
    prompt: &str,
    multiple: bool,
    window: &mut Window,
    cx: &mut App,
) -> Chosen {
    let (sender, chosen) = async_channel::bounded(1);
    let Some(host) = bridge.remote().map(|r| r.name.clone()) else {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple,
            prompt: Some(prompt.to_owned().into()),
        });
        cx.spawn(async move |_| {
            let paths = match paths.await {
                Ok(Ok(Some(paths))) => paths,
                _ => vec![],
            };
            let _ = sender.send(paths).await;
        })
        .detach();
        return chosen;
    };
    let browser = cx.new(|cx| FolderBrowser::new(bridge.clone(), host, multiple, sender, cx));
    let title = prompt.to_owned();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(title.clone())
            .width(px(520.))
            .child(browser.clone())
    });
    chosen
}

struct FolderBrowser {
    bridge: Bridge,
    host: String,
    multiple: bool,
    /// The folder shown, once the host has listed it.
    listing: Option<FolderListing>,
    picked: BTreeSet<PathBuf>,
    error: Option<String>,
    sender: async_channel::Sender<Vec<PathBuf>>,
}
impl FolderBrowser {
    fn new(
        bridge: Bridge,
        host: String,
        multiple: bool,
        sender: async_channel::Sender<Vec<PathBuf>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut browser = Self {
            bridge,
            host,
            multiple,
            listing: None,
            picked: BTreeSet::new(),
            error: None,
            sender,
        };
        browser.open_home(cx);
        browser
    }
    /// Lists the host's home folder for a Control device. A View only device may list only the
    /// host's roots, so it starts at the workspace folder.
    fn open_home(&mut self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let control = bridge.reached().level == Some(AccessLevel::Control);
        cx.spawn(async move |this, cx| {
            let start = match control {
                true => Ok(PathBuf::from("~")),
                false => (ask(&bridge, ask::settings()).await)
                    .map(|settings| PathBuf::from(settings.workspaces_dir)),
            };
            let listed = match start {
                Ok(folder) => ask(&bridge, ask::list_folder(folder)).await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |browser, cx| browser.shown(listed, cx));
        })
        .detach();
    }
    fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let listed = ask(&self.bridge, ask::list_folder(path));
        cx.spawn(async move |this, cx| {
            let listed = listed.await;
            let _ = this.update(cx, |browser, cx| browser.shown(listed, cx));
        })
        .detach();
    }
    fn shown(&mut self, listed: Result<FolderListing, String>, cx: &mut Context<Self>) {
        match listed {
            Ok(listing) => {
                self.listing = Some(listing);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
    /// Hands back what was chosen: the picked folders, or the folder shown.
    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = match (self.picked.is_empty(), &self.listing) {
            (false, _) => self.picked.iter().cloned().collect(),
            (true, Some(listing)) => vec![listing.path.clone()],
            (true, None) => return,
        };
        let _ = self.sender.try_send(chosen);
        window.close_dialog(cx);
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.sender.try_send(vec![]);
        window.close_dialog(cx);
    }
}

/// `ask`'s answer from `bridge`'s host, or why there is none.
fn ask<R: serde::de::DeserializeOwned>(
    bridge: &Bridge,
    ask: Ask<R>,
) -> impl Future<Output = Result<R, String>> + use<R> {
    let reply = bridge.ask(ask);
    async move { reply?.await }
}

impl Render for FolderBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(is_dark(cx));
        let mut body = div().flex().flex_col().gap_2();
        let Some(listing) = self.listing.clone() else {
            return body.child(hint(
                self.error
                    .clone()
                    .unwrap_or("Listing the host's folders…".into()),
                p,
            ));
        };
        let shown = listing.path.to_string_lossy().into_owned();
        let parent = listing.path.parent().map(std::path::Path::to_path_buf);
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    button("folder-up")
                        .ghost()
                        .icon(icon("arrow-up"))
                        .tooltip("The folder above")
                        .disabled(parent.is_none())
                        .on_click(cx.listener(move |browser, _, _, cx| {
                            if let Some(parent) = parent.clone() {
                                browser.open(parent, cx);
                            }
                        })),
                )
                .child(mono(hosts::labelled_path(Some(&self.host), &shown), p)),
        );
        let mut rows = div()
            .id("folder-rows")
            .max_h(px(320.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if listing.folders.is_empty() {
            rows = rows.child(hint("No folders here.", p));
        }
        for (index, entry) in listing.folders.iter().enumerate() {
            let path = listing.path.join(&entry.name);
            let (into, pick) = (path.clone(), path.clone());
            let picked = self.picked.contains(&path);
            rows = rows.child(
                div()
                    .id(("folder", index))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(picked, |d| d.bg(p.overlay))
                    .hover(|d| d.bg(p.overlay))
                    .child(icon("folder").size(px(14.)).text_color(p.subtle))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(entry.name.clone()),
                    )
                    .when(entry.repository, |d| d.child(pill("Git", p.subtle)))
                    .on_click(cx.listener(move |browser, event: &ClickEvent, _, cx| {
                        if event.click_count() > 1 {
                            browser.picked.clear();
                            browser.open(into.clone(), cx);
                        } else if browser.multiple {
                            if !browser.picked.remove(&pick) {
                                browser.picked.insert(pick.clone());
                            }
                        } else {
                            browser.picked = BTreeSet::from([pick.clone()]);
                        }
                        cx.notify();
                    })),
            );
        }
        body.child(rows)
            .child(hint(
                "Double-click a folder to open it. Choose takes the folders picked, or the folder shown.",
                p,
            ))
            .children(self.error.clone().map(|e| hint(e, p).text_color(p.red)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        button("cancel-folders")
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|b, _, w, c| b.cancel(w, c))),
                    )
                    .child(
                        button("choose-folders")
                            .primary()
                            .label("Choose")
                            .on_click(cx.listener(|b, _, w, c| b.choose(w, c))),
                    ),
            )
    }
}
