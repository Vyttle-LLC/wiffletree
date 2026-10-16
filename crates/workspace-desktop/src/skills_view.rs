//! The Skills page: the four skills and the fixed core, and each skill's page, where the human
//! adds their own rules after the stock text or rewrites it, resets it to stock and restores an
//! earlier version from its history. No agent tool reaches what this page changes.
use super::*;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{card, eyebrow, hint, icon, pill};
use workspace_host::{
    provider::CORE,
    skills::{Change, Entry, Line, Recorded, Rewritten, version},
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

/// What a skill's page asks before it acts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    Reset,
    Rewrite,
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
const REWRITE_COST: &str = "When an update changes the stock text, your rewrite keeps running as you wrote it and is marked out of date until you bring the change in yourself. Adding your own rules never needs that.";
const OUT_OF_DATE: &str = "Your rewrite keeps running exactly as you wrote it. Wiffletree doesn't bring stock changes into a rewrite yet: look at what changed, copy in what you want, then mark it up to date.";
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

fn out_of_date(state: &SkillState) -> bool {
    state
        .rewrite
        .as_ref()
        .is_some_and(|rewrite| rewrite.out_of_date)
}

/// What the editor holds and saves: the rewrite in Rewrite mode, the human's rules otherwise.
fn saved_text(state: &SkillState) -> &str {
    state
        .rewrite
        .as_ref()
        .map_or(&state.rules, |rewrite| &rewrite.text)
}

/// "Stock", "Stock and N of your rules", "Rewritten" or "Rewritten · out of date".
fn state_label(state: &SkillState) -> String {
    match (&state.rewrite, rule_count(&state.rules)) {
        (Some(_), _) if out_of_date(state) => "Rewritten · out of date".into(),
        (Some(_), _) => "Rewritten".into(),
        (None, 0) => "Stock".into(),
        (None, rules) => format!("Stock and {rules} of your rules"),
    }
}

/// Grey at stock, yellow out of date, and in the focus color otherwise.
fn state_pill(state: &SkillState, p: Palette) -> Div {
    let color = match (&state.rewrite, rule_count(&state.rules)) {
        _ if out_of_date(state) => p.yellow,
        (None, 0) => p.subtle,
        _ => p.focus,
    };
    pill(state_label(state), color)
}

/// Reset to stock's question, what stops applying, and that it stays in the history.
fn reset_question(state: &SkillState) -> [String; 3] {
    let skill = state.skill;
    let (stops, kept) = match (&state.rewrite, rule_count(&state.rules)) {
        (Some(_), _) => ("Your rewrite stops".into(), "Your rewrite stays"),
        (None, 0) => ("Your rules stop".into(), "Your rules stay"),
        (None, 1) => ("Your 1 rule stops".into(), "Your rules stay"),
        (None, count) => (format!("Your {count} rules stop"), "Your rules stay"),
    };
    let restore = if state.rewrite.is_some() {
        "it"
    } else {
        "them"
    };
    [
        format!("Reset {} to stock?", skill.name()),
        format!(
            "{stops} applying from each agent's next turn. {} the stock text only.",
            about(skill).gets
        ),
        format!("{kept} in the history, where you can restore {restore}."),
    ]
}

/// Rewrite's question: what happens to the human's rules, and what a rewrite costs, or to save
/// or discard unsaved rules first.
fn rewrite_question(state: &SkillState, unsaved: bool) -> [String; 3] {
    if unsaved {
        return [
            "Save or discard your rules first".into(),
            "Your rules have changes you haven't saved, and a rewrite copies only your saved rules.".into(),
            "Save or discard them, then start the rewrite.".into(),
        ];
    }
    let copied = match rule_count(&state.rules) {
        0 => String::new(),
        1 => ", and your 1 rule is copied to its end so nothing is lost".into(),
        count => format!(", and your {count} rules are copied to its end so nothing is lost"),
    };
    [
        format!("Rewrite {}'s stock text?", state.skill.name()),
        format!("You'll edit the stock text itself{copied}."),
        REWRITE_COST.into(),
    ]
}

/// A base has no version when base.md is missing.
fn based_on(base_version: &str) -> String {
    if base_version.is_empty() {
        "an unknown stock version".into()
    } else {
        format!("stock {base_version}")
    }
}

/// "Written against Coordinating v1. This release has v2.", or that this release changed the
/// text without changing its version.
fn out_of_date_title(state: &SkillState, base_version: &str) -> String {
    let (name, stock) = (state.skill.name(), &state.stock_version);
    if base_version.is_empty() {
        format!("Written against an unknown version of {name}. This release has {stock}.")
    } else if base_version == stock {
        format!("Written against an earlier {name} {stock}. This release changed it.")
    } else {
        format!("Written against {name} {base_version}. This release has {stock}.")
    }
}

/// A line of what changed, or a run of unchanged lines left out.
#[derive(Debug, PartialEq, Eq)]
enum Shown<'a> {
    Line(&'a Line),
    Gap(usize),
}

/// What changed, with one unchanged line either side of each change and the rest left out.
fn shown_changes(lines: &[Line]) -> Vec<Shown<'_>> {
    let changed =
        |index: usize| (lines.get(index)).is_some_and(|l| !matches!(l, Line::Unchanged(_)));
    let mut shown = vec![];
    let mut left_out = 0;
    for (index, line) in lines.iter().enumerate() {
        if changed(index) || changed(index + 1) || index.checked_sub(1).is_some_and(changed) {
            if left_out > 0 {
                shown.push(Shown::Gap(left_out));
                left_out = 0;
            }
            shown.push(Shown::Line(line));
        } else {
            left_out += 1;
        }
    }
    if left_out > 0 {
        shown.push(Shown::Gap(left_out));
    }
    shown
}

/// The entry holds what is in effect now, with every file read.
fn is_live(entry: &Entry, state: &SkillState) -> bool {
    let rewrite = entry
        .edits
        .rewrite
        .as_ref()
        .map(|Rewritten { text, base }| Rewrite {
            text: text.clone(),
            base: base.clone(),
            base_version: version(base).into(),
            out_of_date: *base != state.stock,
        });
    state.unreadable.is_none() && entry.edits.rules == state.rules && rewrite == state.rewrite
}

pub(super) fn saved_toast(command: &Command) -> &'static str {
    match command {
        Command::SaveSkillRewrite { .. } => "Your rewrite is saved",
        Command::MarkSkillUpToDate { .. } => "Marked as up to date",
        _ => "Your rules are saved",
    }
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
        Change::StartedRewrite => "Started a rewrite".into(),
        Change::SavedRewrite => "Saved your rewrite".into(),
        Change::MarkedUpToDate => "Marked up to date".into(),
        Change::ResetToStock => "Reset to stock".into(),
        Change::Restored { entry } => format!("Restored the version of {}", entry_time(entry)),
        Change::NewStockVersion { previous } => {
            format!("New stock version {previous} → {}", entry.stock_version)
        }
    }
}

/// The state the entry holds, as "2 rules · stock v1" or "Based on stock v1 · 4,880
/// characters".
fn entry_detail(entry: &Entry) -> String {
    if let Some(Rewritten { text, base }) = &entry.edits.rewrite {
        let length = grouped(text.chars().count());
        return format!("Based on {} · {length} characters", based_on(version(base)));
    }
    let rules = match rule_count(&entry.edits.rules) {
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
        self.skill_text.read(cx).value().to_string()
    }

    /// Any rewrite is out of date, which the sidebar's Skills row marks.
    pub(super) fn a_rewrite_is_out_of_date(&self) -> bool {
        (self.snapshot.iter()).any(|snapshot| snapshot.skills.iter().any(out_of_date))
    }

    /// A skill opens with its saved rules or rewrite in the editor, its stock text folded, and
    /// what changed in the stock text when its rewrite is out of date.
    pub(super) fn open_skill(
        &mut self,
        view: SkillView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SkillView::Skill(skill) = view {
            let state = self.skill_state(skill).cloned();
            let text = state.as_ref().map_or("", saved_text).to_owned();
            self.skill_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
            self.stock_unfolded = false;
            self.confirming = None;
            self.stock_changes.clear();
            if state.as_ref().is_some_and(out_of_date) {
                self.request(Command::SkillStockChanges { skill }, window, cx);
            }
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

    /// The host's answer to a reset, a restore or a rewrite's start: the editor takes the
    /// skill's new text, and an open history gains the entry.
    pub(super) fn skill_replaced(
        &mut self,
        state: SkillState,
        command: &Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let skill = state.skill;
        if self.shows_skill(skill) {
            let text = saved_text(&state).to_owned();
            (self.skill_text).update(cx, |input, cx| input.set_value(text, window, cx));
            if out_of_date(&state) {
                self.request(Command::SkillStockChanges { skill }, window, cx);
            }
        }
        let restored_out_of_date = out_of_date(&state);
        self.skill_saved(state);
        let done = match command {
            Command::RestoreSkill { entry, .. } if restored_out_of_date => format!(
                "Restored the version of {}. It is out of date.",
                entry_time(entry)
            ),
            Command::RestoreSkill { entry, .. } => {
                format!("Restored the version of {}", entry_time(entry))
            }
            Command::StartSkillRewrite { .. } => {
                format!("You're rewriting {}'s stock text", skill.name())
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

    pub(super) fn stock_changes_arrived(&mut self, skill: Skill, lines: Vec<Line>) {
        if self.skill_view == SkillView::Skill(skill) {
            self.stock_changes = lines;
        }
    }

    /// Before `snapshot` replaces the current one: an editor without unsaved edits follows the
    /// saved rules or rewrite.
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
            (skills.iter())
                .find(|s| s.skill == skill)
                .map(|s| saved_text(s).to_owned())
        };
        let shown = self.snapshot.as_ref().and_then(|s| saved(&s.skills));
        if shown == Some(self.skill_draft(cx))
            && let Some(text) = saved(&snapshot.skills)
        {
            self.skill_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
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
        let name = skill.name();
        let at_stock =
            state.rules.is_empty() && state.rewrite.is_none() && state.unreadable.is_none();
        let actions = [
            button("skill-history").ghost().label("History").on_click(
                cx.listener(move |v, _, w, c| v.open_skill(SkillView::History(skill), w, c)),
            ),
            button("reset-skill")
                .ghost()
                .label("Reset to stock")
                .disabled(at_stock)
                .on_click(cx.listener(|v, _, _, c| v.confirm(Confirm::Reset, c))),
        ];
        let roles = about(skill).roles;
        let basis = match &state.rewrite {
            Some(rewrite) => format!(
                "{roles} · your rewrite, based on {}",
                based_on(&rewrite.base_version)
            ),
            None => format!("{roles} · stock {}", state.stock_version),
        };
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
            .child(hint(basis, p))
            .child(Self::modes(state, p, cx));
        let page = match &state.rewrite {
            Some(rewrite) if rewrite.out_of_date => page
                .child(self.out_of_date_banner(state, &rewrite.base_version, p, cx))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.stock_changes_view(state, &rewrite.base_version, p))
                        .child(self.skill_editor(state, p, cx).flex_1().min_w_0()),
                ),
            Some(_) => page.child(self.skill_editor(state, p, cx)),
            None => self.additions(page, state, p, cx),
        };
        let page = page
            .when(state.rewrite.is_some(), |d| {
                d.children(self.skill_links(skill, p, cx))
            })
            .child(Self::enforced_boxes(skill, p));
        let Some(confirm) = self.confirming else {
            return page;
        };
        let unsaved = self.skill_draft(cx) != saved_text(state);
        div().relative().child(page.opacity(0.35)).child(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .justify_center()
                .pt(px(120.))
                .child(Self::confirm_dialog(state, confirm, unsaved, p, cx)),
        )
    }

    fn confirm(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.confirming = Some(confirm);
        cx.notify();
    }

    /// Additions mode: the stock text folded above the human's rules.
    fn additions(&self, page: Div, state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let (name, version) = (state.skill.name(), &state.stock_version);
        let fold = if self.stock_unfolded { "▾" } else { "▸" };
        let stock = format!("{fold} Stock text · {name} {version} · unchanged by your rules");
        let order = format!(
            "Each agent gets the contract, then its role's core, then {name}'s stock text, then your own rules."
        );
        page.child(
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
        .children(self.skill_links(state.skill, p, cx))
        .child(self.skill_editor(state, p, cx))
        .when(state.skill == Skill::Implementing, |d| {
            d.child(hint(PR_RULES_HINT, p))
        })
        .child(hint(order, p))
    }

    /// "Add your own rules" and "Rewrite the stock text…", the mode in use marked. A rewrite is
    /// left by Reset to stock or a restore, so in Rewrite mode neither half acts.
    fn modes(state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let rewritten = state.rewrite.is_some();
        let mode = |id: &'static str, label: &'static str, on: bool| {
            let mode = div().id(id).px_3().py_1().child(label);
            if on {
                mode.bg(p.focus.opacity(0.14))
                    .font_weight(FontWeight::MEDIUM)
            } else {
                mode.text_color(p.subtle)
            }
        };
        let rewrite = if rewritten {
            mode("rewrite-mode", "Your rewrite", true)
        } else {
            mode("rewrite-mode", "Rewrite the stock text…", false)
                .cursor_pointer()
                .on_click(cx.listener(|v, _, _, c| v.confirm(Confirm::Rewrite, c)))
        };
        div().flex().child(
            div()
                .flex()
                .rounded_lg()
                .border_1()
                .border_color(p.edge)
                .overflow_hidden()
                .text_size(px(12.))
                .child(mode("rules-mode", "Add your own rules", !rewritten))
                .child(rewrite.border_l_1().border_color(p.edge)),
        )
    }

    /// Reset and Rewrite ask first; Cancel sends nothing.
    fn confirm_dialog(
        state: &SkillState,
        confirm: Confirm,
        unsaved: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let skill = state.skill;
        let ([ask, what, kept], command) = match confirm {
            Confirm::Reset => (reset_question(state), Command::ResetSkill { skill }),
            Confirm::Rewrite => (
                rewrite_question(state, unsaved),
                Command::StartSkillRewrite { skill },
            ),
        };
        let close = |v: &mut Self, c: &mut Context<Self>| {
            v.confirming = None;
            c.notify();
        };
        let act = button("confirm-skill").on_click(cx.listener(move |v, _, w, c| {
            close(v, c);
            v.request(command.clone(), w, c);
        }));
        let act = match confirm {
            Confirm::Reset => act.danger().label("Reset to stock"),
            Confirm::Rewrite => act.primary().label("Start rewrite").disabled(unsaved),
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
                .child(hint(what, p))
                .child(hint(kept, p))
                .child(
                    div()
                        .pt_2()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            button("cancel-skill")
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(move |v, _, _, c| close(v, c))),
                        )
                        .child(act),
                ),
        )
    }

    fn out_of_date_banner(
        &self,
        state: &SkillState,
        base_version: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let skill = state.skill;
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(p.yellow.opacity(0.45))
            .bg(p.yellow.opacity(0.09))
            .text_size(px(12.))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(out_of_date_title(state, base_version)),
            )
            .child(OUT_OF_DATE)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("mark-up-to-date")
                            .primary()
                            .label("Mark as up to date")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.request(Command::MarkSkillUpToDate { skill }, w, c)
                            })),
                    )
                    .child(
                        button("banner-reset")
                            .ghost()
                            .label("Reset to stock")
                            .on_click(cx.listener(|v, _, _, c| v.confirm(Confirm::Reset, c))),
                    ),
            )
    }

    /// What changed from the rewrite's base to the stock text, as removed and added lines.
    fn stock_changes_view(&self, state: &SkillState, base_version: &str, p: Palette) -> Div {
        let rows = shown_changes(&self.stock_changes).into_iter().map(|shown| {
            let row = div().px_2();
            let text = |text: &str| {
                SharedString::from(if text.is_empty() { " " } else { text }.to_owned())
            };
            match shown {
                Shown::Line(Line::Unchanged(line)) => row.text_color(p.subtle).child(text(line)),
                Shown::Line(Line::Removed(line)) => (row.bg(p.red.opacity(0.11)))
                    .text_color(p.subtle)
                    .line_through()
                    .child(text(line)),
                Shown::Line(Line::Added(line)) => row.bg(p.green.opacity(0.13)).child(text(line)),
                Shown::Gap(1) => row
                    .bg(p.overlay)
                    .text_color(p.subtle)
                    .child("⋯ 1 line unchanged ⋯"),
                Shown::Gap(count) => (row.bg(p.overlay))
                    .text_color(p.subtle)
                    .child(format!("⋯ {count} lines unchanged ⋯")),
            }
        });
        let base_version = if base_version.is_empty() {
            "unknown"
        } else {
            base_version
        };
        let versions = format!("{base_version} → {}", state.stock_version);
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("What changed in the stock text"),
                    )
                    .child(hint(versions, p)),
            )
            .child(
                div()
                    .py_1()
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .border_1()
                    .border_color(p.edge.opacity(0.55))
                    .bg(p.base)
                    .overflow_hidden()
                    .font_family(".SystemMonoFont")
                    .text_size(px(11.5))
                    .line_height(relative(1.55))
                    .children(rows),
            )
    }

    /// Newest first, each entry with View and Restore, except the newest when it holds what is
    /// in effect now.
    fn history_page(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Div {
        let today = Local::now().date_naive();
        let crumb = self.skills_crumb(skill.name(), p, cx);
        let state = self.skill_state(skill);
        let entries: Vec<_> = (self.skill_history.iter().enumerate())
            .map(|(index, recorded)| {
                let at =
                    written_at(&recorded.name).map_or(recorded.name.clone(), |at| when(at, today));
                let current = index == 0
                    && (recorded.entry.as_ref().ok())
                        .zip(state)
                        .is_some_and(|(entry, state)| is_live(entry, state));
                self.history_entry(skill, recorded, at, current, p, cx)
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
                let text = match (&entry.edits.rewrite, entry.edits.rules.as_str()) {
                    (Some(rewrite), _) => rewrite.text.clone(),
                    (None, "") => "No rules of your own: agents got the stock text alone.".into(),
                    (None, rules) => rules.to_owned(),
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

    /// The human's rules or rewrite with Save and Discard, as on the Models page. The host
    /// refuses a text over the limit, and its error is what the page shows.
    fn skill_editor(&self, state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let skill = state.skill;
        let rewriting = state.rewrite.is_some();
        let draft = self.skill_draft(cx);
        let saved = saved_text(state).to_owned();
        let dirty = draft != saved;
        let chars = draft.chars().count();
        let discard = cx.listener(move |v, _, w, c| {
            (v.skill_text).update(c, |input, c| input.set_value(saved.clone(), w, c))
        });
        let save = cx.listener(move |v, _, w, c| {
            let text = v.skill_draft(c);
            let command = if rewriting {
                Command::SaveSkillRewrite { skill, text }
            } else {
                Command::SaveSkillRules { skill, rules: text }
            };
            v.request(command, w, c);
        });
        let unreadable = (state.unreadable.as_ref()).map(|error| {
            hint(
                format!("{error}. Until it can be read, it counts as missing."),
                p,
            )
            .text_color(p.red)
        });
        let (title, note) = if rewriting {
            ("Your rewrite", "sent in place of the stock text")
        } else {
            (
                "Your rules",
                "sent after the stock text; where the two disagree, yours win",
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Self::card_title(title, note, p))
            .children(unreadable)
            .child(Textarea::new(&self.skill_text).w_full())
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
                        .min_w_0()
                        .when(chars > MAX_SKILL_TEXT, |d| d.text_color(p.red)),
                    )
                    .when(dirty, |d| {
                        d.child(
                            button("discard-skill-text")
                                .ghost()
                                .label("Discard")
                                .on_click(discard),
                        )
                    })
                    .child(
                        button("save-skill-text")
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
        CORE, Shown, based_on, core_tab, entry_detail, entry_title, grouped, is_live, out_of_date,
        out_of_date_title, reset_question, rewrite_question, saved_text, shown_changes,
        state_label, when, written_at,
    };
    use chrono::{Local, NaiveDate, TimeZone};
    use workspace_core::{Command, Role, Skill, SkillState};
    use workspace_host::{
        Host,
        skills::{Change, Edits, Entry, Line, Recorded, stock},
    };

    fn state(host: &mut Host, command: Command) -> SkillState {
        serde_json::from_value(host.execute(command).unwrap()).unwrap()
    }

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
        assert_eq!(history[1].entry.as_ref().unwrap().edits.rules, rules);

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
            edits: Edits {
                rules: "Read the tests first".into(),
                rewrite: None,
            },
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

    #[test]
    fn starting_a_rewrite_from_the_page_asks_first_and_copies_the_rules_to_its_end() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        let [ask, copied, _] = rewrite_question(&host.skills().state(skill), false);
        assert_eq!(ask, "Rewrite Coordinating's stock text?");
        assert_eq!(copied, "You'll edit the stock text itself.");
        let rules = "Create tickets only once I say go.\nName every ticket after its outcome.";
        host.skills().save_rules(skill, rules).unwrap();
        let [_, copied, _] = rewrite_question(&host.skills().state(skill), false);
        assert_eq!(
            copied,
            "You'll edit the stock text itself, and your 2 rules are copied to its end so nothing is lost."
        );

        let started = state(&mut host, Command::StartSkillRewrite { skill });
        assert_eq!(state_label(&started), "Rewritten");
        assert_eq!(
            saved_text(&started),
            format!(
                "{}\n## Your own rules for Coordinating\n{rules}",
                stock(skill)
            )
        );
        let text = "Plan first.\nCreate tickets only once I say go.".to_owned();
        let saved = state(&mut host, Command::SaveSkillRewrite { skill, text });
        assert_eq!(
            saved_text(&saved),
            "Plan first.\nCreate tickets only once I say go."
        );
        let instructions = host.instructions(Role::ProjectOrchestrator);
        assert!(instructions.ends_with(saved_text(&saved)));
        assert!(!instructions.contains("## Your own rules"));
    }

    #[test]
    fn a_rewrite_waits_for_unsaved_rules_to_be_saved_or_discarded() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        host.skills()
            .save_rules(Skill::Coordinating, "Rule")
            .unwrap();
        let state = host.skills().state(Skill::Coordinating);
        assert_eq!(
            rewrite_question(&state, true),
            [
                "Save or discard your rules first",
                "Your rules have changes you haven't saved, and a rewrite copies only your saved rules.",
                "Save or discard them, then start the rewrite.",
            ]
        );
    }

    #[test]
    fn a_rewrite_whose_base_is_missing_names_an_unknown_version() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        state(&mut host, Command::StartSkillRewrite { skill });
        std::fs::remove_file(home.path().join("skills/coordinating/base.md")).unwrap();
        let text = "My way".to_owned();
        let saved = state(&mut host, Command::SaveSkillRewrite { skill, text });
        let base_version = &saved.rewrite.as_ref().unwrap().base_version;
        assert!(out_of_date(&saved));
        assert_eq!(
            out_of_date_title(&saved, base_version),
            "Written against an unknown version of Coordinating. This release has v1."
        );
        assert_eq!(based_on(base_version), "an unknown stock version");
        assert_eq!(based_on("v1"), "stock v1");
        let history = listing(&mut host, skill);
        assert_eq!(
            entry_detail(history[0].entry.as_ref().unwrap()),
            "Based on an unknown stock version · 6 characters"
        );
    }

    #[test]
    fn resetting_a_rewritten_skill_from_the_page_names_the_rewrite() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        let rewritten = state(&mut host, Command::StartSkillRewrite { skill });
        assert_eq!(
            reset_question(&rewritten),
            [
                "Reset Coordinating to stock?",
                "Your rewrite stops applying from each agent's next turn. The project coordinator gets the stock text only.",
                "Your rewrite stays in the history, where you can restore it.",
            ]
        );
        let reset = state(&mut host, Command::ResetSkill { skill });
        assert_eq!(state_label(&reset), "Stock");
        let history = listing(&mut host, skill);
        assert_eq!(titles(&history), ["Reset to stock", "Started a rewrite"]);
    }

    #[test]
    fn a_release_marks_a_rewrite_out_of_date_until_the_human_marks_it_up_to_date() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        state(&mut host, Command::StartSkillRewrite { skill });
        // A stand-in for the stock text of the release the rewrite was written against.
        let body = stock(skill).split_once('\n').unwrap().1;
        let base = format!("# Coordinating — v0\n{body}A line v1 removed.\n");
        std::fs::write(home.path().join("skills/coordinating/base.md"), base).unwrap();

        let snapshot = host.snapshot().unwrap();
        let coordinating = &snapshot.skills[0];
        assert_eq!(state_label(coordinating), "Rewritten · out of date");
        assert!(snapshot.skills.iter().any(out_of_date));
        assert_eq!(
            out_of_date_title(coordinating, "v0"),
            "Written against Coordinating v0. This release has v1."
        );
        let lines: Vec<Line> =
            serde_json::from_value(host.execute(Command::SkillStockChanges { skill }).unwrap())
                .unwrap();
        let shown = shown_changes(&lines);
        assert!(
            matches!(&shown[..2], [Shown::Line(Line::Removed(v0)), Shown::Line(Line::Added(_))] if v0 == "# Coordinating — v0")
        );
        assert_eq!(
            shown.last(),
            Some(&Shown::Line(&Line::Removed("A line v1 removed.".into())))
        );

        let marked = state(&mut host, Command::MarkSkillUpToDate { skill });
        assert_eq!(state_label(&marked), "Rewritten");
        assert!(!host.snapshot().unwrap().skills.iter().any(out_of_date));
    }

    #[test]
    fn a_changed_stock_text_with_the_same_version_says_this_release_changed_it() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        let state = host.skills().state(Skill::Reviewing);
        assert_eq!(
            out_of_date_title(&state, "v1"),
            "Written against an earlier Reviewing v1. This release changed it."
        );
    }

    #[test]
    fn unchanged_lines_far_from_a_change_are_left_out() {
        let same = |text: &str| Line::Unchanged(text.into());
        let lines = [
            same("a"),
            same("b"),
            same("c"),
            Line::Removed("d".into()),
            Line::Added("e".into()),
            same("f"),
            same("g"),
        ];
        assert_eq!(
            shown_changes(&lines),
            [
                Shown::Gap(2),
                Shown::Line(&lines[2]),
                Shown::Line(&lines[3]),
                Shown::Line(&lines[4]),
                Shown::Line(&lines[5]),
                Shown::Gap(1),
            ]
        );
    }

    #[test]
    fn the_newest_entry_is_current_only_while_it_matches_what_is_in_effect() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        let skill = Skill::Implementing;
        host.skills().save_rules(skill, "Rule").unwrap();
        let newest = || {
            let (_, entry) = host.skills().history(skill).pop().unwrap();
            entry
        };
        let live = |host: &Host| is_live(&newest(), &host.skills().state(skill));
        assert!(live(&host));
        let rules = home.path().join("skills/implementing/additions.md");
        std::fs::write(&rules, "Rule\nEdited outside the app").unwrap();
        assert!(!live(&host));
        std::fs::write(&rules, b"\xff not text").unwrap();
        assert!(!live(&host));
        std::fs::write(&rules, "Rule").unwrap();

        // A rewrite whose base is out of date is current while it is what agents get.
        host.skills().start_rewrite(skill).unwrap();
        std::fs::write(
            home.path().join("skills/implementing/base.md"),
            "# Implementing — v0\n",
        )
        .unwrap();
        host.skills().save_rewrite(skill, "My way").unwrap();
        assert!(live(&host));
        // A base edited outside the app, at the same version, is not what the entry holds.
        std::fs::write(
            home.path().join("skills/implementing/base.md"),
            "# Implementing — v0\nEdited outside the app\n",
        )
        .unwrap();
        assert!(!live(&host));
        let entry = newest();
        assert_eq!(entry_title(&entry), "Saved your rewrite");
        assert_eq!(entry_detail(&entry), "Based on stock v0 · 6 characters");
    }
}
