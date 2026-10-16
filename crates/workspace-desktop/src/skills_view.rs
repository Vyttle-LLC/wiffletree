//! The Skills page: the four skills and the fixed core, and each skill's page, where the human
//! adds their own rules after the stock text. No agent tool reaches what this page changes.
use super::*;
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{card, eyebrow, hint, icon, pill};
use workspace_host::{provider::CORE, skills::version};

/// What the Skills page shows: the list, the fixed core at one of its parts, or one skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SkillView {
    List,
    Core(usize),
    Skill(Skill),
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
    enforced: &'static [&'static str],
    decides: &'static [&'static str],
}

fn about(skill: Skill) -> About {
    match skill {
        Skill::Coordinating => About {
            roles: "Project coordinator",
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

/// "Stock", or "Stock and N of your rules", counting each line with text as a rule.
fn state_label(state: &SkillState) -> String {
    match state
        .rules
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
    {
        0 => "Stock".into(),
        rules => format!("Stock and {rules} of your rules"),
    }
}

/// Grey at stock, in the focus color once the human has rules.
fn state_pill(state: &SkillState, p: Palette) -> Div {
    let color = if state.rules.is_empty() {
        p.subtle
    } else {
        p.focus
    };
    pill(state_label(state), color)
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
        }
        self.skill_view = view;
        cx.notify();
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

    /// "Skills · <name>", where Skills goes back to the list.
    fn skills_crumb(&self, name: &'static str, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .gap_1()
            .text_size(px(11.))
            .text_color(p.subtle)
            .child(
                div()
                    .id("skills-crumb")
                    .text_color(p.focus)
                    .cursor_pointer()
                    .on_click(cx.listener(|v, _, w, c| v.open_skill(SkillView::List, w, c)))
                    .child("Skills"),
            )
            .child(format!("· {name}"))
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
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.skills_crumb(name, p, cx))
            .child(Self::skill_heading("book", name, state_pill(state, p)))
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
            .child(Self::enforced_boxes(skill, p))
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
                    (self.verifiers.read(cx).verification()).map_or(&[][..], |v| &v.levels);
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
    use super::{CORE, core_tab, grouped, state_label};
    use workspace_core::{Command, Role, Skill, SkillState};
    use workspace_host::Host;

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
