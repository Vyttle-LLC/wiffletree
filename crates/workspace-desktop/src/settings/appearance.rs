//! Settings › Appearance as the theme-files study draws it: Mode, the Light theme and Dark theme
//! menus, the themes folder's buttons, and the only place a theme problem is shown.
use super::*;
use crate::themes::{self, Choice, Side, Themes, Tone};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};

const LABEL_WIDTH: f32 = 96.;

pub(super) fn section(p: Palette, cx: &App) -> Div {
    let preferences = cx.global::<Preferences>();
    let themes = cx.global::<Themes>();
    let current = preferences.appearance;
    let mut modes = div().flex().gap_1();
    for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
        modes = modes.child(
            segment(
                SharedString::from(format!("appearance-{}", appearance.label())),
                appearance.label(),
                appearance == current,
            )
            .icon(icon(appearance.icon()).size(px(13.)))
            .on_click(move |_, _, cx| set_appearance(appearance, cx)),
        );
    }
    let has_error = themes.notices.iter().any(|n| n.tone == Tone::Error);
    let use_tidal: Vec<Side> = (themes.notices.iter())
        .flat_map(|notice| notice.use_tidal.iter().copied())
        .collect();
    let mut actions = div().flex().gap_2().child(
        button("open-themes-folder")
            .xsmall()
            .label("Open themes folder")
            .on_click(|_, _, cx| themes::open_folder(cx)),
    );
    if has_error {
        actions = actions.child(
            button("use-tidal")
                .xsmall()
                .ghost()
                .label("Use Tidal")
                .on_click(move |_, _, cx| {
                    for side in &use_tidal {
                        themes::set(*side, Choice::Tidal, cx);
                    }
                }),
        );
    } else if themes.notices.is_empty() {
        actions = actions.child(
            button("new-theme-from-tidal")
                .xsmall()
                .label("New theme from Tidal")
                .on_click(|_, _, cx| themes::new_from_tidal(cx)),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(super::section("APPEARANCE", p))
        .child(row("Mode", modes, p))
        .child(row(
            "Light theme",
            theme_menu(Side::Light, &preferences.light_theme, themes, p),
            p,
        ))
        .child(row(
            "Dark theme",
            theme_menu(Side::Dark, &preferences.dark_theme, themes, p),
            p,
        ))
        .children(themes.notices.iter().map(|notice| {
            let color = match notice.tone {
                Tone::Error => p.red,
                Tone::Warning => p.yellow,
            };
            banner(color, &notice.head, &notice.body, p)
        }))
        .children((themes.failure.as_ref()).map(|failure| ui::banner(p.red).child(failure.clone())))
        .child(actions)
        .child(hint(
            "System shows the light theme when macOS is light and the dark theme when it is \
             dark. Themes are JSON files in the themes folder; Wiffletree reads one again when \
             you come back to it.",
            p,
        ))
}

fn row(label: &'static str, control: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(p.subtle)
                .child(label),
        )
        .child(control)
}

/// Tidal first, then each file with this side's variant by name, its file name on the right.
fn theme_menu(side: Side, chosen: &Choice, themes: &Themes, p: Palette) -> impl IntoElement {
    let options: Vec<(Choice, String, SharedString)> = std::iter::once((
        Choice::Tidal,
        "Tidal".to_string(),
        SharedString::from("built in"),
    ))
    .chain(
        (themes.listed.iter())
            .filter(|listed| listed.has(side))
            .map(|listed| {
                let file = listed.file.clone();
                (Choice::File(file.clone()), listed.name.clone(), file.into())
            }),
    )
    .collect();
    let chosen = chosen.clone();
    let id = match side {
        Side::Light => "light-theme",
        Side::Dark => "dark-theme",
    };
    button(id)
        .xsmall()
        .label(themes.label(&chosen))
        .dropdown_caret(true)
        .dropdown_menu(move |mut menu, _, _| {
            for (index, (choice, name, file)) in options.iter().enumerate() {
                if index == 1 {
                    menu = menu.separator();
                }
                let (name, file, picked) = (name.clone(), file.clone(), choice.clone());
                menu = menu.item(
                    PopupMenuItem::element(move |_, _| {
                        div()
                            .flex()
                            .w_full()
                            .justify_between()
                            .gap_6()
                            .child(name.clone())
                            .child(div().text_color(p.subtle).child(file.clone()))
                    })
                    .checked(*choice == chosen)
                    .on_click(move |_, _, cx| themes::set(side, picked.clone(), cx)),
                );
            }
            menu.min_w(px(260.))
        })
}

/// A tinted banner whose bold head runs into its text.
fn banner(color: Hsla, head: &str, body: &str, p: Palette) -> Div {
    let bold = 0..head.len();
    ui::banner(color).child(div().flex_1().min_w_0().text_color(p.text).child(
        StyledText::new(format!("{head} {body}")).with_highlights([(
            bold,
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        )]),
    ))
}
