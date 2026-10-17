//! Before a repository is chosen: what is shared, the field and Check, and who can read the
//! repository checked, with Set up.
use super::*;
use gpui_component::input::Input;

const INTRO: &str = "Keep your settings the same on every Mac you use Wiffletree on. Name a private GitHub repository: an empty one to set up, or the one your other Mac already uses.";
const NO_REPOSITORY: &str = "No repository yet? Create an empty private repository on GitHub, then name it here. Wiffletree doesn’t create repositories for you.";
const SHARED: [&str; 5] = [
    "Skills: your rules, rewrites and their history",
    "Review: verifiers, levels, rounds and reviews per ticket",
    "The model guide",
    "Check-in thresholds",
    "PR keeper triggers",
];
const STAYS: [&str; 6] = [
    "Providers and allowed models",
    "The PR keeper’s model",
    "Workspace folder",
    "Owners allowed to get pull requests",
    "Repositories and their checks",
    "Appearance",
];
const NEVER: [&str; 3] = [
    "Passwords, tokens and keys",
    "The database",
    "Conversations, projects and tickets",
];

impl Settings {
    fn check(&mut self, cx: &mut Context<Self>) {
        let repository = self.repository_input.read(cx).value().trim().to_owned();
        self.checked = None;
        self.ask_repository(Command::CheckSettingsRepository { repository }, cx);
    }

    /// The field, then what a check found or what would be shared.
    pub(super) fn choose_view(&self, section: Div, p: Palette, cx: &mut Context<Self>) -> Div {
        let field = div()
            .flex()
            .items_center()
            .gap_2()
            .child(Input::new(&self.repository_input).small().flex_1())
            .child(
                button("check-repository")
                    .primary()
                    .label("Check")
                    .disabled(self.asking)
                    .on_click(cx.listener(|v, _, _, c| v.check(c))),
            );
        let found = (self.checked.as_ref()).map(|(_, check)| self.check_card(check, p, cx));
        let section = section
            .child(heading("Not syncing", p.subtle, p))
            .child(hint(INTRO, p))
            .child(field)
            .child(hint(NO_REPOSITORY, p).text_size(px(font::CAPTION)));
        match found {
            Some(card) => section.child(card),
            None => section.child(what_is_shared(p)),
        }
    }

    /// Who can read a repository not set up yet, and Set up.
    fn check_card(
        &self,
        check: &SettingsRepositoryCheck,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let (kind, not_set_up, set_up) = match check.organization {
            false => (
                "Private · your account",
                "Not set up yet. Set up adds wiffletree.json, settings/ and skills/ beside what it holds, and changes no other file.",
                button("set-up-repository").primary().label("Set up…"),
            ),
            true => (
                "Private · organization",
                "Not set up yet. A repository under your own account keeps your settings to yourself.",
                button("set-up-repository").ghost().label("Set up anyway…"),
            ),
        };
        let set_up = set_up
            .disabled(self.asking)
            .on_click(cx.listener(|v, _, _, c| v.confirm(Some(Confirm::SetUp), c)));
        let kind = pill(kind, p.subtle);
        repository_card(&check.repository, check.organization, kind, None, p)
            .child(hint(not_set_up, p))
            .child(div().flex().justify_end().child(set_up))
    }
}

/// What would be shared, what stays on this Mac and what is never shared.
fn what_is_shared(p: Palette) -> Div {
    let list = |icon_name: &'static str, title: &'static str, items: &[&'static str]| {
        div()
            .flex_1()
            .min_w_0()
            .p_2()
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(edge::BOX))
            .bg(p.surface)
            .text_size(px(font::DETAIL))
            .line_height(relative(1.4))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_1p5()
                    .text_size(px(font::SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .child(
                        icon(icon_name)
                            .size(px(14.))
                            .mt(px(1.))
                            .text_color(p.subtle),
                    )
                    .child(div().flex_1().min_w_0().child(title)),
            )
            .children(
                (items.iter()).map(|item| div().text_color(p.subtle).child(format!("• {item}"))),
            )
    };
    div()
        .flex()
        .gap_2()
        .child(list("share", "Shared with your other Macs", &SHARED))
        .child(list("laptop", "Stays on this Mac", &STAYS))
        .child(list("ban", "Never shared", &NEVER))
}
