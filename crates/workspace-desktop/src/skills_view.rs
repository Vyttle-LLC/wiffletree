//! The Skills page: the four skills and the fixed core, and each skill's page, where the human
//! adds their own rules after the stock text, resets it to stock and restores an earlier
//! version from its history. No agent tool reaches what this page changes.
use super::*;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{card, eyebrow, hint, icon, pill};
use workspace_host::{
    provider::CORE,
    skills::{Change, Entry, Recorded, version},
};

/// What the Skills page shows: the list, the fixed core at one of its parts, one skill or its
/// history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SkillView {
    List,
    Core(usize),
    Skill(Skill),
    History(Skill),
}

const CORE_NAMES: [&str; 5] = [
    "Contract",
    "Coordinator",
    "Implementer",
    "Verifier",
    "PR keeper",
];
const CORE_PURPOSE: &str = "How every agent talks to Wiffletree: its tools, its reports and findings, and the rules the app enforces whatever a skill says.";
const NO_PR_SKILL: &str = "Opening a pull request isn't a skill. Agents write pull requests by your repository's rules and your own, so put yours in Implementing's rules.";
const CORE_HINT: &str = "Wiffletree depends on these instructions: the names of its tools, how reports and findings are written, and how messages and pull requests work. They can't be edited, so no change to a skill can break them. Each agent gets the contract and its role's part, then its skill.";
const PR_RULES_HINT: &str = "Agents write pull requests by your repository's rules and your own, so put your PR rules here.";
const APPLIES: &str =
    "Applies from each agent's next turn, and a Codex agent's from its next new session";
const NOT_ENFORCED: &str = "Not enforced: an agent's shell isn't sandboxed, so it can still run git push or gh with your credentials.";

/// What a skill's page says about it: who uses it, what the app's own operations enforce
/// whatever its text says, and what its text decides.
struct About {
    roles: &'static str,
    /// Who is left with the stock text once the human's rules stop applying.
    gets: &'static str,
    enforced: &'static [&'static str],
    decides: &'static [&'static str],
}

fn about(skill: Skill) -> About {
    match skill {
        Skill::Coordinating => About {
            roles: "Project coordinator",
            gets: "The project coordinator gets",
            enforced: &[
                "Only the models allowed on this machine",
                "Every verifier of the chosen level runs, and all must pass",
                "A re-check only after a passed review",
                "The caps on rounds and full reviews",
                "Findings are sorted before a ticket is accepted",
                "Wiffletree has no tool that merges, and its publishing asks you unless the owner is allowed and the review passed",
            ],
            decides: &[
                "When to plan, create tickets and start agents",
                "Which model and effort, within the allowed ones",
                "When to stop chasing findings",
                "When to ask you",
            ],
        },
        Skill::Implementing => About {
            roles: "Implementers and maintenance agents",
            gets: "Implementers and maintenance agents get",
            enforced: &[
                "One ticket, in its own worktree and branch",
                "\u{201c}Ready for testing\u{201d} is refused with uncommitted files",
                "Wiffletree pushes and opens pull requests only through its publishing, which checks the review and the owners you allowed",
                "Wiffletree has no tool that merges",
            ],
            decides: &[
                "How to work and which checks to run",
                "How big a fix is",
                "When to object to a finding",
                "How to write commits and pull requests",
            ],
        },
        Skill::Reviewing => About {
            roles: "Testers and reviewers",
            gets: "Testers and reviewers get",
            enforced: &[
                "Every verifier of the chosen level runs, and all must pass",
                "Findings carry a severity, a location, a trigger and evidence",
                "A check fails only on a blocking finding with a location, a trigger and evidence",
                "The worktree must be left exactly as it was found",
            ],
            decides: &[
                "What counts as blocking",
                "How much to run",
                "What to check again",
            ],
        },
        Skill::KeepingAPr => About {
            roles: "PR keeper",
            gets: "The PR keeper gets",
            enforced: &[
                "Only the conflicted files, or the PR's files, change",
                "The repository's checks and a review run before a push",
                "One try per problem per head",
                "The triggers set in Repositories",
            ],
            decides: &["How to resolve a conflict", "How to fix failed checks"],
        },
    }
}

/// Each line with text is a rule, so rules of blank lines alone are none.
fn rule_count(rules: &str) -> usize {
    rules.lines().filter(|line| !line.trim().is_empty()).count()
}

/// "Stock", or "Stock and N of your rules".
fn state_label(state: &SkillState) -> String {
    match rule_count(&state.rules) {
        0 => "Stock".into(),
        rules => format!("Stock and {rules} of your rules"),
    }
}

/// Grey at stock, in the focus color once the human has rules.
fn state_pill(state: &SkillState, p: Palette) -> Div {
    let color = match rule_count(&state.rules) {
        0 => p.subtle,
        _ => p.focus,
    };
    pill(state_label(state), color)
}

/// Reset to stock's question, what stops applying, and that it stays in the history.
fn reset_question(state: &SkillState) -> [String; 3] {
    let skill = state.skill;
    let rules = match rule_count(&state.rules) {
        0 => "Your rules stop".into(),
        1 => "Your 1 rule stops".into(),
        count => format!("Your {count} rules stop"),
    };
    [
        format!("Reset {} to stock?", skill.name()),
        format!(
            "{rules} applying from each agent's next turn. {} the stock text only.",
            about(skill).gets
        ),
        "Your rules stay in the history, where you can restore them.".into(),
    ]
}

/// When a history entry was written, from its name.
fn written_at(name: &str) -> Option<DateTime<Local>> {
    let at = NaiveDateTime::parse_from_str(name, "%Y-%m-%dT%H-%M-%S%.3fZ").ok()?;
    Some(at.and_utc().with_timezone(&Local))
}

/// "14 Oct 09:12", or the entry's name when it is not a time.
fn entry_time(name: &str) -> String {
    written_at(name).map_or(name.into(), |at| at.format("%-d %b %H:%M").to_string())
}

/// "Today 08:00", "Yesterday 17:30" or "13 Oct 10:15".
fn when(at: DateTime<Local>, today: NaiveDate) -> String {
    let time = at.format("%H:%M");
    match at.date_naive() {
        day if day == today => format!("Today {time}"),
        day if Some(day) == today.pred_opt() => format!("Yesterday {time}"),
        _ => at.format("%-d %b %H:%M").to_string(),
    }
}

/// What happened, as "New stock version v1 → v2".
fn entry_title(entry: &Entry) -> String {
    match &entry.change {
        Change::SavedRules => "Saved your rules".into(),
        Change::ResetToStock => "Reset to stock".into(),
        Change::Restored { entry } => format!("Restored the version of {}", entry_time(entry)),
        Change::NewStockVersion { previous } => {
            format!("New stock version {previous} → {}", entry.stock_version)
        }
    }
}

/// The state the entry holds, as "2 rules · stock v1".
fn entry_detail(entry: &Entry) -> String {
    let rules = match rule_count(&entry.rules) {
        0 => "No rules of your own".into(),
        1 => "1 rule".into(),
        count => format!("{count} rules"),
    };
    format!("{rules} · stock {}", entry.stock_version)
}

/// A count with its thousands grouped, as "40,000".
fn grouped(count: usize) -> String {
    let digits = count.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// A part's tab, as "Contract v9".
fn core_tab(part: usize) -> String {
    format!("{} {}", CORE_NAMES[part], version(CORE[part].1))
}

impl Workspace {
    fn skill_state(&self, skill: Skill) -> Option<&SkillState> {
        (self.snapshot.as_ref()?.skills.iter()).find(|state| state.skill == skill)
    }

    fn skill_draft(&self, cx: &App) -> String {
        self.skill_rules.read(cx).value().to_string()
    }

    /// A skill opens with its saved rules in the editor and its stock text folded.
    pub(super) fn open_skill(
        &mut self,
        view: SkillView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SkillView::Skill(skill) = view {
            let rules = self.skill_state(skill).map(|state| state.rules.clone());
            self.skill_rules.update(cx, |input, cx| {
                input.set_value(rules.unwrap_or_default(), window, cx)
            });
            self.stock_unfolded = false;
            self.confirming_reset = false;
        }
        if let SkillView::History(skill) = view {
            self.skill_history.clear();
            self.viewed_entry = None;
            self.request(Command::SkillHistory { skill }, window, cx);
        }
        self.skill_view = view;
        cx.notify();
    }

    fn shows_skill(&self, skill: Skill) -> bool {
        matches!(self.skill_view, SkillView::Skill(s) | SkillView::History(s) if s == skill)
    }

    /// The host's answer to a reset or a restore: the editor takes the skill's new rules, and an
    /// open history gains the entry.
    pub(super) fn skill_replaced(
        &mut self,
        state: SkillState,
        command: &Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let skill = state.skill;
        if self.shows_skill(skill) {
            let rules = state.rules.clone();
            (self.skill_rules).update(cx, |input, cx| input.set_value(rules, window, cx));
        }
        self.skill_saved(state);
        let done = match command {
            Command::RestoreSkill { entry, .. } => {
                format!("Restored the version of {}", entry_time(entry))
            }
            _ => format!("{} is back to stock", skill.name()),
        };
        self.toast(done, window, cx);
        if self.skill_view == SkillView::History(skill) {
            self.request(Command::SkillHistory { skill }, window, cx);
        }
    }

    pub(super) fn history_arrived(&mut self, skill: Skill, history: Vec<Recorded>) {
        if self.skill_view == SkillView::History(skill) {
            self.skill_history = history;
        }
    }

    /// Before `snapshot` replaces the current one: an editor without unsaved edits follows the
    /// saved rules.
    pub(super) fn follow_saved_rules(
        &mut self,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SkillView::Skill(skill) = self.skill_view else {
            return;
        };
        let saved = |skills: &[SkillState]| {
            skills
                .iter()
                .find(|s| s.skill == skill)
                .map(|s| s.rules.clone())
        };
        let shown = self.snapshot.as_ref().and_then(|s| saved(&s.skills));
        if shown == Some(self.skill_draft(cx))
            && let Some(rules) = saved(&snapshot.skills)
        {
            self.skill_rules
                .update(cx, |input, cx| input.set_value(rules, window, cx));
        }
    }

    /// The host's answer to a save, shown until the next snapshot.
    pub(super) fn skill_saved(&mut self, saved: SkillState) {
        if let Some(state) = (self.snapshot.as_mut())
            .and_then(|s| s.skills.iter_mut().find(|state| state.skill == saved.skill))
        {
            *state = saved;
        }
    }

    pub(super) fn skills_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        match self.skill_view {
            SkillView::Core(part) => self.core_page(part, p, cx),
            SkillView::Skill(skill) => match self.skill_state(skill) {
                Some(state) => self.skill_page(state, p, cx),
                None => self.skills_list(p, cx),
            },
            SkillView::History(skill) => self.history_page(skill, p, cx),
            SkillView::List => self.skills_list(p, cx),
        }
    }

    fn skills_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let skills = self.snapshot.iter().flat_map(|s| &s.skills);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(hint(
                "How your agents work · changes apply from each agent's next turn",
                p,
            ))
            .child(eyebrow("SKILLS YOU CAN CHANGE", p).pt_2())
            .children(skills.map(|state| {
                let skill = state.skill;
                let head = (skill.name(), "book", state_pill(state, p));
                self.skill_row(
                    SkillView::Skill(skill),
                    head,
                    skill.purpose(),
                    about(skill).roles,
                    p,
                    cx,
                )
            }))
            .child(eyebrow("ALWAYS INCLUDED", p).pt_2())
            .child(self.skill_row(
                SkillView::Core(0),
                ("Fixed core", "lock", pill("Read only", p.subtle)),
                CORE_PURPOSE,
                "Every agent · sent before its skill",
                p,
                cx,
            ))
            .child(hint(NO_PR_SKILL, p).pt_2())
    }

    fn skill_row(
        &self,
        view: SkillView,
        (name, glyph, state): (&'static str, &'static str, Div),
        what: &'static str,
        who: &'static str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(SharedString::from(format!("skill-row-{name}")))
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .rounded_lg()
            .border_1()
            .border_color(p.edge.opacity(0.4))
            .cursor_pointer()
            .hover(move |d| d.bg(p.overlay.opacity(0.6)))
            .on_click(cx.listener(move |v, _, w, c| v.open_skill(view, w, c)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(glyph).size(px(14.)).text_color(p.subtle))
                    .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child(name))
                    .child(state),
            )
            .child(hint(what, p))
            .child(hint(who, p).text_size(px(11.)))
    }

    /// "Skills · <name>", where Skills goes back to the list, or "Skills · <skill> · History",
    /// where the skill goes back to its page.
    fn skills_crumb(&self, name: &'static str, p: Palette, cx: &mut Context<Self>) -> Div {
        let link = |id: &'static str, view: SkillView, label: &'static str| {
            div()
                .id(id)
                .text_color(p.focus)
                .cursor_pointer()
                .on_click(cx.listener(move |v, _, w, c| v.open_skill(view, w, c)))
                .child(label)
        };
        let crumb = div()
            .flex()
            .gap_1()
            .text_size(px(11.))
            .text_color(p.subtle)
            .child(link("skills-crumb", SkillView::List, "Skills"));
        match self.skill_view {
            SkillView::History(skill) => crumb
                .child("·")
                .child(link("skill-crumb", SkillView::Skill(skill), name))
                .child("· History"),
            _ => crumb.child(format!("· {name}")),
        }
    }

    fn skill_heading(glyph: &'static str, name: &'static str, state: Div) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(icon(glyph))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name),
            )
            .child(state)
    }

    fn read_only_text(text: impl Into<SharedString>, p: Palette) -> Div {
        div()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(p.edge.opacity(0.4))
            .bg(p.surface)
            .text_size(px(12.))
            .line_height(relative(1.5))
            .child(text.into())
    }

    fn core_page(&self, part: usize, p: Palette, cx: &mut Context<Self>) -> Div {
        let crumb = self.skills_crumb("Fixed core", p, cx);
        let tabs = (0..CORE.len()).map(|tab| {
            button(SharedString::from(format!("core-part-{tab}")))
                .ghost()
                .selected(tab == part)
                .label(core_tab(tab))
                .tooltip(format!("Sent to {}", CORE[tab].0))
                .on_click(cx.listener(move |v, _, w, c| v.open_skill(SkillView::Core(tab), w, c)))
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(crumb)
            .child(Self::skill_heading(
                "lock",
                "Fixed core",
                pill("Read only", p.subtle),
            ))
            .child(hint(CORE_HINT, p))
            .child(div().flex().flex_wrap().gap_1().children(tabs))
            .child(Self::read_only_text(CORE[part].1, p))
            .child(hint(
                "Bundled with Wiffletree · changes only with a new release",
                p,
            ))
    }

    fn skill_page(&self, state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let skill = state.skill;
        let (name, version) = (skill.name(), &state.stock_version);
        let fold = if self.stock_unfolded { "▾" } else { "▸" };
        let stock = format!("{fold} Stock text · {name} {version} · unchanged by your rules");
        let order = format!(
            "Each agent gets the contract, then its role's core, then {name}'s stock text, then your own rules."
        );
        // Nothing to reset: no rules, and no file that could not be read.
        let at_stock = state.rules.is_empty() && state.unreadable.is_none();
        let actions = [
            button("skill-history").ghost().label("History").on_click(
                cx.listener(move |v, _, w, c| v.open_skill(SkillView::History(skill), w, c)),
            ),
            button("reset-skill")
                .ghost()
                .label("Reset to stock")
                .disabled(at_stock)
                .on_click(cx.listener(|v, _, _, c| {
                    v.confirming_reset = true;
                    c.notify();
                })),
        ];
        let page = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.skills_crumb(name, p, cx))
            .child(
                Self::skill_heading("book", name, state_pill(state, p))
                    .child(div().flex_1())
                    .children(actions),
            )
            .child(hint(format!("{} · stock {version}", about(skill).roles), p))
            .child(
                div()
                    .id("stock-fold")
                    .cursor_pointer()
                    .on_click(cx.listener(|v, _, _, c| {
                        v.stock_unfolded = !v.stock_unfolded;
                        c.notify();
                    }))
                    .child(hint(stock, p)),
            )
            .when(self.stock_unfolded, |d| {
                d.child(Self::read_only_text(state.stock.clone(), p))
            })
            .children(self.skill_links(skill, p, cx))
            .child(self.rules_editor(state, p, cx))
            .when(skill == Skill::Implementing, |d| {
                d.child(hint(PR_RULES_HINT, p))
            })
            .child(hint(order, p))
            .child(Self::enforced_boxes(skill, p));
        if !self.confirming_reset {
            return page;
        }
        div().relative().child(page.opacity(0.35)).child(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .justify_center()
                .pt(px(120.))
                .child(Self::reset_dialog(state, p, cx)),
        )
    }

    /// Reset asks first; Cancel sends nothing.
    fn reset_dialog(state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let skill = state.skill;
        let [ask, stops, kept] = reset_question(state);
        let close = |v: &mut Self, c: &mut Context<Self>| {
            v.confirming_reset = false;
            c.notify();
        };
        div().w(px(380.)).h_full().flex().flex_col().child(
            div()
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_xl()
                .border_1()
                .border_color(p.edge)
                .bg(p.surface)
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(ask),
                )
                .child(hint(stops, p))
                .child(hint(kept, p))
                .child(
                    div()
                        .pt_2()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("cancel-reset")
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(move |v, _, _, c| close(v, c))),
                        )
                        .child(
                            button("confirm-reset")
                                .danger()
                                .label("Reset to stock")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    close(v, c);
                                    v.request(Command::ResetSkill { skill }, w, c);
                                })),
                        ),
                ),
        )
    }

    /// Newest first, each entry with View and, before the current one, Restore.
    fn history_page(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Div {
        let today = Local::now().date_naive();
        let crumb = self.skills_crumb(skill.name(), p, cx);
        let entries: Vec<_> = (self.skill_history.iter().enumerate())
            .map(|(index, recorded)| {
                let at =
                    written_at(&recorded.name).map_or(recorded.name.clone(), |at| when(at, today));
                self.history_entry(skill, recorded, at, index == 0, p, cx)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(crumb)
            .child(Self::skill_heading(
                "history",
                "History",
                hint("newest first", p),
            ))
            .when(self.skill_history.is_empty(), |d| {
                d.child(hint(
                    "No changes yet. Each save, reset and restore adds an entry.",
                    p,
                ))
            })
            .child(div().flex().flex_col().children(entries))
    }

    fn history_entry(
        &self,
        skill: Skill,
        recorded: &Recorded,
        at: String,
        current: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = recorded.name.clone();
        let viewed = self.viewed_entry.as_ref() == Some(&name);
        let action = |id: &str, label: &'static str| {
            div()
                .id(SharedString::from(format!("{id}-{name}")))
                .text_color(p.focus)
                .cursor_pointer()
                .child(label)
        };
        let (title, detail, actions, text) = match &recorded.entry {
            Ok(entry) => {
                let shown = name.clone();
                let view = action("view-entry", if viewed { "Hide" } else { "View" }).on_click(
                    cx.listener(move |v, _, _, c| {
                        v.viewed_entry = (!viewed).then(|| shown.clone());
                        c.notify();
                    }),
                );
                let entry_name = name.clone();
                let restore =
                    action("restore-entry", "Restore").on_click(cx.listener(move |v, _, w, c| {
                        let entry = entry_name.clone();
                        v.request(Command::RestoreSkill { skill, entry }, w, c);
                    }));
                let text = match entry.rules.as_str() {
                    "" => "No rules of your own: agents got the stock text alone.".into(),
                    rules => rules.to_owned(),
                };
                (
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(entry_title(entry)),
                        )
                        .when(current, |d| d.child(pill("current", p.focus))),
                    hint(entry_detail(entry), p),
                    div()
                        .flex()
                        .gap_3()
                        .child(view)
                        .when(!current, |d| d.child(restore)),
                    viewed.then_some(text),
                )
            }
            Err(error) => (
                div().text_color(p.red).child("Could not be read"),
                hint(format!("{name}.json: {error}"), p),
                div(),
                None,
            ),
        };
        div()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_t_1()
            .border_color(p.edge.opacity(0.3))
            .text_size(px(12.))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .w(px(104.))
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(p.subtle)
                            .child(at),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(title)
                            .child(detail),
                    )
                    .child(actions),
            )
            .children(text.map(|text| Self::read_only_text(text, p)))
    }

    /// The human's rules with Save and Discard, as on the Models page. The host refuses a text
    /// over the limit, and its error is what the page shows.
    fn rules_editor(&self, state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let skill = state.skill;
        let draft = self.skill_draft(cx);
        let dirty = draft != state.rules;
        let chars = draft.chars().count();
        let saved = state.rules.clone();
        let discard = cx.listener(move |v, _, w, c| {
            (v.skill_rules).update(c, |input, c| input.set_value(saved.clone(), w, c))
        });
        let save = cx.listener(move |v, _, w, c| {
            let rules = v.skill_draft(c);
            v.request(Command::SaveSkillRules { skill, rules }, w, c);
        });
        let unreadable = state.unreadable.as_ref().map(|error| {
            let why = format!(
                "Your rules could not be read ({error}), so agents get the stock text alone."
            );
            hint(why, p).text_color(p.red)
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Self::card_title(
                "Your rules",
                "sent after the stock text; where the two disagree, yours win",
                p,
            ))
            .children(unreadable)
            .child(Textarea::new(&self.skill_rules).w_full())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        hint(
                            format!("{APPLIES} · {} of 32,000 characters", grouped(chars)),
                            p,
                        )
                        .flex_1()
                        .when(chars > MAX_SKILL_TEXT, |d| d.text_color(p.red)),
                    )
                    .when(dirty, |d| {
                        d.child(
                            button("discard-skill-rules")
                                .ghost()
                                .label("Discard")
                                .on_click(discard),
                        )
                    })
                    .child(
                        button("save-skill-rules")
                            .primary()
                            .label("Save")
                            .disabled(!dirty)
                            .on_click(save),
                    ),
            )
    }

    fn enforced_boxes(skill: Skill, p: Palette) -> Div {
        let About {
            enforced, decides, ..
        } = about(skill);
        let column = |glyph: &'static str, title: &'static str, items: &[&'static str]| {
            card(p)
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(icon(glyph).size(px(14.)))
                        .child(title),
                )
                .children(items.iter().map(|item| hint(format!("• {item}"), p)))
        };
        div()
            .flex()
            .gap_3()
            .child(
                column("shield", "The app enforces, whatever this says", enforced)
                    .child(hint(NOT_ENFORCED, p).text_color(p.yellow)),
            )
            .child(column("pencil", "This text decides", decides))
    }

    /// Reviewing shows the review levels' "Use it" lines and Coordinating the model guide, both
    /// read-only and edited on the Models page.
    fn skill_links(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Option<Div> {
        let (title, from, lines) = match skill {
            Skill::Reviewing => {
                let levels =
                    (self.verifiers.read(cx).saved_verification()).map_or(&[][..], |v| &v.levels);
                let lines = levels.iter().map(|level| {
                    format!(
                        "{} · {} · {}",
                        level.name,
                        level.verifiers.join(", "),
                        level.use_when
                    )
                });
                (
                    "When the coordinator uses each level",
                    "from Models → Review ·",
                    lines.collect(),
                )
            }
            Skill::Coordinating => {
                let guide = self.saved_model_selection.guide.trim();
                let guide = if guide.is_empty() {
                    "No guide yet."
                } else {
                    guide
                };
                (
                    "How the coordinator picks a model and effort",
                    "from Models → Guide ·",
                    vec![guide.to_owned()],
                )
            }
            _ => return None,
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    Self::card_title(title, from, p).child(
                        div()
                            .id("edit-in-models")
                            .text_size(px(12.))
                            .text_color(p.focus)
                            .cursor_pointer()
                            .on_click(cx.listener(|v, _, w, c| v.open_page(Page::Models, w, c)))
                            .child("Edit in Models"),
                    ),
                )
                .child(Self::read_only_text(lines.join("\n"), p)),
        )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        CORE, core_tab, entry_title, grouped, reset_question, state_label, when, written_at,
    };
    use chrono::{Local, NaiveDate, TimeZone};
    use workspace_core::{Command, Role, Skill, SkillState};
    use workspace_host::{
        Host,
        skills::{Change, Entry, Recorded},
    };

    fn listing(host: &mut Host, skill: Skill) -> Vec<Recorded> {
        serde_json::from_value(host.execute(Command::SkillHistory { skill }).unwrap()).unwrap()
    }

    fn titles(history: &[Recorded]) -> Vec<String> {
        let title = |r: &Recorded| entry_title(r.entry.as_ref().unwrap());
        history.iter().map(title).collect()
    }

    #[test]
    fn rules_of_blank_lines_alone_are_stock() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Implementing, "\n  \n\t\n")
            .unwrap();
        let state = host.skills().state(Skill::Implementing);
        assert_eq!(state_label(&state), "Stock");
    }

    #[test]
    fn resetting_from_the_page_names_the_rules_and_restoring_brings_them_back() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let rules = "Write PR descriptions with a Summary and a Testing section\nRun clippy\nKeep commits small";
        host.skills()
            .save_rules(Skill::Implementing, rules)
            .unwrap();
        let state = host.skills().state(Skill::Implementing);
        assert_eq!(
            reset_question(&state),
            [
                "Reset Implementing to stock?",
                "Your 3 rules stop applying from each agent's next turn. Implementers and maintenance agents get the stock text only.",
                "Your rules stay in the history, where you can restore them.",
            ]
        );

        let reset = Command::ResetSkill {
            skill: Skill::Implementing,
        };
        let state: SkillState = serde_json::from_value(host.execute(reset).unwrap()).unwrap();
        assert_eq!(state_label(&state), "Stock");
        assert!(
            !host
                .instructions(Role::Implementer)
                .contains("Keep commits small")
        );
        let history = listing(&mut host, Skill::Implementing);
        assert_eq!(titles(&history), ["Reset to stock", "Saved your rules"]);
        assert_eq!(history[1].entry.as_ref().unwrap().rules, rules);

        // Restoring the entry from before the reset, as its Restore does.
        let entry = history[1].name.clone();
        let restore = Command::RestoreSkill {
            skill: Skill::Implementing,
            entry: entry.clone(),
        };
        host.execute(restore).unwrap();
        assert!(host.instructions(Role::Implementer).ends_with(rules));
        let history = listing(&mut host, Skill::Implementing);
        let restored = format!("Restored the version of {}", super::entry_time(&entry));
        assert_eq!(
            titles(&history),
            [restored.as_str(), "Reset to stock", "Saved your rules"]
        );
    }

    #[test]
    fn an_update_reads_as_a_new_stock_version_naming_both() {
        let update = Entry {
            change: Change::NewStockVersion {
                previous: "v1".into(),
            },
            stock_version: "v2".into(),
            rules: "Read the tests first".into(),
        };
        assert_eq!(entry_title(&update), "New stock version v1 → v2");
    }

    #[test]
    fn entries_are_dated_today_yesterday_or_by_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let at = |day, hour, minute| {
            Local
                .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
                .unwrap()
        };
        assert_eq!(when(at(16, 8, 0), today), "Today 08:00");
        assert_eq!(when(at(15, 17, 30), today), "Yesterday 17:30");
        assert_eq!(when(at(13, 10, 15), today), "13 Oct 10:15");
        let written = written_at("2026-10-15T14-03-22.481Z").unwrap();
        assert_eq!(
            written.to_utc().to_rfc3339(),
            "2026-10-15T14:03:22.481+00:00"
        );
    }

    #[test]
    fn opening_the_page_lists_the_four_skills_with_their_states() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Implementing, "One\n\nTwo\nThree\n")
            .unwrap();
        let listed: Vec<_> = (host.snapshot().unwrap().skills.iter())
            .map(|state| (state.skill.name(), state_label(state)))
            .collect();
        assert_eq!(
            listed,
            [
                ("Coordinating", "Stock".into()),
                ("Implementing", "Stock and 3 of your rules".into()),
                ("Reviewing", "Stock".into()),
                ("Keeping a PR", "Stock".into()),
            ]
        );
    }

    #[test]
    fn counts_group_their_thousands() {
        assert_eq!(
            [0, 999, 1_000, 40_000, 1_234_567].map(grouped),
            ["0", "999", "1,000", "40,000", "1,234,567"]
        );
    }

    #[test]
    fn the_fixed_core_has_a_tab_for_each_part_with_its_version() {
        let tabs: Vec<_> = (0..CORE.len()).map(core_tab).collect();
        assert_eq!(
            tabs,
            [
                "Contract v9",
                "Coordinator v1",
                "Implementer v1",
                "Verifier v1",
                "PR keeper v1"
            ]
        );
    }

    #[test]
    fn saving_rules_from_the_page_reaches_the_next_turn_and_a_text_too_long_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let rule = "Write PR descriptions with a Summary and a Testing section";
        let save = |rules: String| Command::SaveSkillRules {
            skill: Skill::Implementing,
            rules,
        };
        let saved: SkillState =
            serde_json::from_value(host.execute(save(rule.into())).unwrap()).unwrap();
        assert_eq!(state_label(&saved), "Stock and 1 of your rules");
        assert!(host.instructions(Role::Implementer).ends_with(rule));

        // The page sends any length, so the host's refusal, naming the limit, is what it shows.
        let refused = host.execute(save("x".repeat(40_000))).unwrap_err();
        assert!(refused.to_string().contains("32,000"), "{refused}");
        assert!(host.instructions(Role::Implementer).ends_with(rule));
    }
}
