//! The Skills page: the four skills and the fixed core, and each skill's page, where the human
//! adds their own rules after the stock text or rewrites it, resets it to stock, restores an
//! earlier version from its history, and reviews or undoes a merge of a new stock version. No
//! agent tool reaches what this page changes.
use super::*;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{card, eyebrow, hint, icon, pill};
use workspace_host::{
    provider::CORE,
    skills::{
        Change, Entry, Line, MergeReview, Recorded, Reviewed, Rewritten, merge::Source, merge_note,
        version,
    },
};

/// What the Skills page shows: the list, the fixed core at one of its parts, one skill, its
/// history or a review of its merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SkillView {
    List,
    Core(usize),
    Skill(Skill),
    History(Skill),
    Review(Skill),
}

/// What a skill's pages ask before they act.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    Reset,
    Rewrite,
    /// The merge `note` names; `later` when the skill has changed since.
    Undo {
        note: MergeNote,
        later: bool,
    },
    /// A rewrite from an entry older than the merge of stock version `merged`.
    Restore {
        entry: String,
        merged: String,
    },
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
const REWRITE_COST: &str = "When an update changes the stock text, Wiffletree merges the change into your rewrite and leaves a note you can review or undo. Where you also changed what the update changes, the PR keeper's Claude model merges that passage, or, without one, you choose the wording. Adding your own rules never needs a merge.";
const OUT_OF_DATE: &str = "Your rewrite keeps running exactly as you wrote it. Look at what changed, then merge it in, or copy in what you want and mark it up to date.";
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
            gets: "Coordinators get",
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

/// "Stock", "Stock and N of your rules", "Rewritten", "Rewritten · updated" or "Rewritten · out
/// of date".
fn state_label(state: &SkillState) -> String {
    match (&state.rewrite, rule_count(&state.rules)) {
        (Some(_), _) if out_of_date(state) => "Rewritten · out of date".into(),
        (Some(_), _) if state.merge_note.is_some() => "Rewritten · updated".into(),
        (Some(_), _) => "Rewritten".into(),
        (None, 0) => "Stock".into(),
        (None, rules) => format!("Stock and {rules} of your rules"),
    }
}

/// Grey at stock, yellow out of date, green updated, and in the focus color otherwise.
fn state_pill(state: &SkillState, p: Palette) -> Div {
    let color = match (&state.rewrite, rule_count(&state.rules)) {
        _ if out_of_date(state) => p.yellow,
        _ if state.merge_note.is_some() => p.green,
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

/// What an out-of-date rewrite's banner offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Offers {
    /// Nothing, while a merge waits or runs, or for a base from a newer release.
    Nothing,
    /// Merge in, Mark as up to date and Reset to stock.
    MergeIn,
    /// Try again and Mark as up to date.
    TryAgain,
    /// Choosing a model, or a wording for each passage.
    Wordings,
}

/// The banner of a rewrite that is out of date, or based on a newer release: its title and
/// text, and what it offers.
fn rewrite_banner(state: &SkillState) -> Option<([String; 2], Offers)> {
    let rewrite = state.rewrite.as_ref()?;
    let (name, stock, base) = (
        state.skill.name(),
        &state.stock_version,
        &rewrite.base_version,
    );
    let banner = match (&state.merge_standing, rewrite.out_of_date) {
        (Some(MergeStanding::Undone), true) => (
            [
                format!("You undid the merge of {stock}"),
                format!(
                    "Your rewrite is as it was before the merge, and {stock} won't be merged into it again on its own."
                ),
            ],
            Offers::MergeIn,
        ),
        (Some(MergeStanding::WaitingForSync), true) => {
            let waiting = format!(
                "Waiting for this launch's first sync, so your Macs merge {stock} only once. If another Mac already merged it, this Mac takes that."
            );
            ([out_of_date_title(state, base), waiting], Offers::Nothing)
        }
        (Some(MergeStanding::Merging { passages }), true) => (
            [
                format!("Merging {stock} into your rewrite"),
                format!(
                    "The PR keeper's model is merging the {} you and {stock} both changed. Your rewrite runs as you wrote it until the merge applies.",
                    counted(*passages, "passage")
                ),
            ],
            Offers::Nothing,
        ),
        (Some(MergeStanding::Failed { reason }), true) => (
            [
                format!("The merge of {stock} couldn't finish"),
                format!(
                    "{reason} Nothing was changed, and your rewrite runs as you wrote it. Wiffletree tries again at the next start."
                ),
            ],
            Offers::TryAgain,
        ),
        (Some(MergeStanding::NeedsModel { passages, codex }), true) => {
            let (it, which) = match passages.len() {
                1 => ("it", "the passage"),
                _ => ("them", "each passage"),
            };
            let chosen = match codex {
                true => "the one chosen is a Codex model",
                false => "none is chosen",
            };
            (
                [
                    format!(
                        "{stock} and your rewrite both change {}",
                        counted(passages.len(), "passage")
                    ),
                    format!(
                        "Merging {it} needs a Claude model for the PR keeper, and {chosen}. Your rewrite runs as you wrote it. Choose a model, or choose a wording for {which}."
                    ),
                ],
                Offers::Wordings,
            )
        }
        (_, true) => (
            [out_of_date_title(state, base), OUT_OF_DATE.into()],
            Offers::MergeIn,
        ),
        (Some(MergeStanding::NewerBase { machine }), false) => {
            let merged = machine.as_ref().map_or(String::new(), |_| {
                format!("That Mac runs a newer Wiffletree and merged {base} into your rewrite. ")
            });
            let from = machine
                .as_deref()
                .unwrap_or("a Mac with a newer Wiffletree");
            (
                [
                    format!("Based on {name} {base}, from {from}"),
                    format!(
                        "{merged}This Mac has {stock} and sends the rewrite as written. It isn't out of date here, and nothing is merged here."
                    ),
                ],
                Offers::Nothing,
            )
        }
        _ => return None,
    };
    Some(banner)
}

/// Who uses the skill, and when a merge the page still notes came in: "Project coordinator · v2
/// merged into your rewrite today at 08:01", or "on 14 Oct at 08:01".
fn list_who(state: &SkillState) -> String {
    let roles = about(state.skill).roles;
    let Some(note) = &state.merge_note else {
        return roles.into();
    };
    let when = match written_at(&note.entry) {
        Some(at) if at.date_naive() == Local::now().date_naive() => {
            format!("today at {}", at.format("%H:%M"))
        }
        Some(at) => format!("on {}", at.format("%-d %b at %H:%M")),
        None => format!("at {}", note.entry),
    };
    format!("{roles} · {} merged into your rewrite {when}", note.to)
}

/// "1 change" or "3 changes".
fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        count => format!("{count} {noun}s"),
    }
}

/// "3 changes from v2, 1 passage merged by Claude · haiku · medium", the model as `model` names
/// it.
fn merge_counts(note: &MergeNote, model: Option<&str>) -> String {
    let merged = match (note.by_model, model) {
        (0, _) | (_, None) => String::new(),
        (passages, Some(model)) => format!(", {} merged by {model}", counted(passages, "passage")),
    };
    format!(
        "{} from {}{merged}",
        counted(note.from_update, "change"),
        note.to
    )
}

/// " · 6,240 tokens" when a model merged, or nothing.
fn merge_tokens(note: &MergeNote) -> String {
    (note.model.as_ref()).map_or(String::new(), |model| {
        format!(" · {} tokens", grouped(model.tokens as usize))
    })
}

/// "This Mac", or the name of the Mac that merged.
fn merging_mac(note: &MergeNote) -> &str {
    note.machine.as_deref().unwrap_or("This Mac")
}

/// The merge note's title and what came in, and how, naming the model when one merged.
fn merge_note_text(state: &SkillState, note: &MergeNote, model: Option<&str>) -> [String; 2] {
    let skill = state.skill;
    let by_model = match note.by_model {
        0 => String::new(),
        passages => {
            let (passage, one) = match passages {
                1 => ("passage", "that one"),
                _ => ("passages", "those"),
            };
            format!(
                " You had also changed {passages} {passage} {} changes, so {} merged {one}, keeping your wording where both could stand.",
                note.to,
                model.unwrap_or("the model")
            )
        }
    };
    [
        format!("Merged {} {} into your rewrite", skill.name(), note.to),
        format!(
            "{} from {} came in as plain text.{by_model} {} the merged text from their next turn.",
            counted(note.from_update, "change"),
            note.to,
            about(skill).gets
        ),
    ]
}

/// Undo's question, what goes back, and what stays in the history; `later` when the skill has
/// changed since the merge.
fn undo_question(state: &SkillState, note: &MergeNote, later: bool) -> [String; 3] {
    let MergeNote {
        entry, from, to, ..
    } = note;
    let (since, kept) = if later {
        (
            ", and your changes since stop applying",
            "The merge and your changes since stay",
        )
    } else {
        ("", "The merge stays")
    };
    [
        format!("Undo the merge of {to}?"),
        format!(
            "Your rewrite goes back to exactly what it was before {}, based on {}{since}. {} it from their next turn, on every Mac.",
            time_or_date(entry),
            based_on(from),
            about(state.skill).gets
        ),
        format!("{to} won't be merged in again on its own. {kept} in the history."),
    ]
}

/// Restore's question for a rewrite from before the merge of `merged`, which it stops as an
/// Undo does.
fn restore_question(state: &SkillState, entry: &str, merged: &str) -> [String; 3] {
    [
        format!("Restore the version of {}?", entry_time(entry)),
        format!(
            "It is from before the merge of {merged}, so no automatic merge brings {merged} in until a newer version arrives. You can still merge it in from {}'s page.",
            state.skill.name()
        ),
        "The merge and every later change stay in the history.".into(),
    ]
}

/// What `confirm` asks, and the command its button sends.
fn question(state: &SkillState, confirm: &Confirm, unsaved: bool) -> ([String; 3], Command) {
    let skill = state.skill;
    match confirm {
        Confirm::Reset => (reset_question(state), Command::ResetSkill { skill }),
        Confirm::Rewrite => (
            rewrite_question(state, unsaved),
            Command::StartSkillRewrite { skill },
        ),
        Confirm::Undo { note, later } => (
            undo_question(state, note, *later),
            Command::UndoSkillMerge {
                skill,
                entry: note.entry.clone(),
            },
        ),
        Confirm::Restore { entry, merged } => (
            restore_question(state, entry, merged),
            Command::RestoreSkill {
                skill,
                entry: entry.clone(),
            },
        ),
    }
}

/// A line of what changed, or a run of unchanged lines left out.
#[derive(Debug, PartialEq, Eq)]
enum Shown<'a, T> {
    Line(&'a T),
    Gap(usize),
}

fn is_change(line: &Line) -> bool {
    !matches!(line, Line::Unchanged(_))
}

/// What changed, with one unchanged line either side of each change and the rest left out.
fn shown_changes<T>(lines: &[T], is_change: impl Fn(&T) -> bool) -> Vec<Shown<'_, T>> {
    let changed = |index: usize| lines.get(index).is_some_and(&is_change);
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
    let rewrite = (state.rewrite.as_ref()).map(|Rewrite { text, base, .. }| Rewritten {
        text: text.clone(),
        base: base.clone(),
    });
    state.unreadable.is_none() && entry.edits.rules == state.rules && entry.edits.rewrite == rewrite
}

pub(super) fn saved_toast(command: &Command) -> &'static str {
    match command {
        Command::SaveSkillRewrite { .. } => "Your rewrite is saved",
        Command::MarkSkillUpToDate { .. } => "Marked as up to date",
        _ => "Your rules are saved",
    }
}

/// "08:01" for an entry written today, otherwise as `entry_time`.
fn time_or_date(name: &str) -> String {
    match written_at(name) {
        Some(at) if at.date_naive() == Local::now().date_naive() => at.format("%H:%M").to_string(),
        _ => entry_time(name),
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
        Change::Merged { from, to, .. } => format!("Merged {from} → {to}"),
        Change::UndidMerge { to, .. } => format!("Undid the merge of {to}"),
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

/// A banner above a skill's text, in `color`.
fn banner(color: Hsla) -> Div {
    div()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(color.opacity(0.45))
        .bg(color.opacity(0.09))
        .text_size(px(12.))
}

/// A banner that only says where things stand, with no color of its own.
fn quiet_banner(p: Palette) -> Div {
    banner(p.edge).border_color(p.edge.opacity(0.7)).bg(p.base)
}

/// A banner's first line: its `glyph` in `color`, then the title.
fn banner_head(glyph: &'static str, color: Hsla, title: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(icon(glyph).size(px(14.)).text_color(color))
        .child(div().font_weight(FontWeight::MEDIUM).child(title))
}

/// Lines of what changed, in a monospaced box.
fn diff_block(p: Palette) -> Div {
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
}

/// A line of what changed: removed lines struck through, added ones in `added`.
fn diff_row(line: &Line, added: Hsla, p: Palette) -> Div {
    let row = div().px_2();
    let text = |text: &str| SharedString::from(if text.is_empty() { " " } else { text }.to_owned());
    match line {
        Line::Unchanged(line) => row.text_color(p.subtle).child(text(line)),
        Line::Removed(line) => (row.bg(p.red.opacity(0.11)))
            .text_color(p.subtle)
            .line_through()
            .child(text(line)),
        Line::Added(line) => row.bg(added.opacity(0.13)).child(text(line)),
    }
}

/// "⋯ 4 lines unchanged ⋯".
fn gap_row(count: usize, p: Palette) -> Div {
    let unchanged = match count {
        1 => "1 line unchanged".into(),
        count => format!("{count} lines unchanged"),
    };
    (div().px_2().bg(p.overlay))
        .text_color(p.subtle)
        .child(format!("⋯ {unchanged} ⋯"))
}

/// For a rewrite in a history listed newest first, the stock version the newest merge after it
/// brought in, which restoring it stops.
fn merged_since(history: &[Recorded], index: usize) -> Option<String> {
    fn entry(recorded: &Recorded) -> Option<&Entry> {
        recorded.entry.as_ref().ok()
    }
    (entry(&history[index])?.edits.rewrite.as_ref())?;
    history[..index]
        .iter()
        .find_map(|recorded| match &entry(recorded)?.change {
            Change::Merged { to, .. } => Some(to.clone()),
            _ => None,
        })
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
        self.confirming = None;
        self.merge_review = None;
        self.skill_wordings.clear();
        if let SkillView::Skill(skill) = view {
            let state = self.skill_state(skill).cloned();
            let text = state.as_ref().map_or("", saved_text).to_owned();
            self.skill_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
            self.stock_unfolded = false;
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

    /// Review changes from the note, which hides it on this Mac, or from a history entry.
    fn open_review(
        &mut self,
        skill: Skill,
        entry: String,
        from_note: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_skill(SkillView::Review(skill), window, cx);
        let review = Command::SkillMergeReview {
            skill,
            entry: entry.clone(),
        };
        self.request(review, window, cx);
        if from_note {
            self.request(Command::DismissSkillMerge { skill, entry }, window, cx);
        }
    }

    fn shows_skill(&self, skill: Skill) -> bool {
        matches!(self.skill_view, SkillView::Skill(s) | SkillView::History(s) if s == skill)
    }

    /// The host's answer to a reset, a restore, a rewrite's start, a merge or its Undo: the
    /// editor takes the skill's new text, and an open history gains the entry. An Undo from its
    /// review goes back to the skill's page.
    pub(super) fn skill_replaced(
        &mut self,
        state: SkillState,
        command: &Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let skill = state.skill;
        if self.skill_view == SkillView::Review(skill) {
            self.skill_view = SkillView::Skill(skill);
        }
        if self.shows_skill(skill) {
            let text = saved_text(&state).to_owned();
            (self.skill_text).update(cx, |input, cx| input.set_value(text, window, cx));
            if out_of_date(&state) {
                self.request(Command::SkillStockChanges { skill }, window, cx);
            }
        }
        let restored_out_of_date = out_of_date(&state);
        let state_stock_version = state.stock_version.clone();
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
            Command::UndoSkillMerge { .. } => "Undid the merge".into(),
            Command::MergeSkillUpdate { .. } => {
                format!("Merged {} into your rewrite", state_stock_version)
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

    pub(super) fn review_arrived(&mut self, skill: Skill, review: MergeReview) {
        if self.skill_view == SkillView::Review(skill) {
            self.merge_review = Some(review);
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

    /// The view, under what it asks to confirm.
    pub(super) fn skills_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let page = match self.skill_view {
            SkillView::Core(part) => self.core_page(part, p, cx),
            SkillView::Skill(skill) => match self.skill_state(skill) {
                Some(state) => self.skill_page(state, p, cx),
                None => self.skills_list(p, cx),
            },
            SkillView::History(skill) => self.history_page(skill, p, cx),
            SkillView::Review(skill) => self.review_page(skill, p, cx),
            SkillView::List => self.skills_list(p, cx),
        };
        let shown = match self.skill_view {
            SkillView::Skill(skill) | SkillView::History(skill) | SkillView::Review(skill) => {
                self.skill_state(skill)
            }
            SkillView::Core(_) | SkillView::List => None,
        };
        let (Some(confirm), Some(state)) = (self.confirming.clone(), shown) else {
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
                    list_who(state),
                    p,
                    cx,
                )
            }))
            .child(eyebrow("ALWAYS INCLUDED", p).pt_2())
            .child(self.skill_row(
                SkillView::Core(0),
                ("Fixed core", "lock", pill("Read only", p.subtle)),
                CORE_PURPOSE,
                "Every agent · sent before its skill".into(),
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
        who: String,
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
        let (skill, below) = match self.skill_view {
            SkillView::History(skill) => (skill, "· History"),
            SkillView::Review(skill) => (skill, "· Review changes"),
            _ => return crumb.child(format!("· {name}")),
        };
        crumb
            .child("·")
            .child(link("skill-crumb", SkillView::Skill(skill), name))
            .child(below)
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
            .child(Self::modes(state, p, cx))
            .children(
                (state.merge_note.as_ref()).map(|note| self.merge_note_banner(state, note, p, cx)),
            )
            .children(self.rewrite_banner_view(state, p, cx));
        let page = match &state.rewrite {
            Some(rewrite) if rewrite.out_of_date => page.child(
                div()
                    .flex()
                    .gap_3()
                    .child(self.stock_changes_view(state, &rewrite.base_version, p))
                    .child(self.skill_editor(state, p, cx).flex_1().min_w_0()),
            ),
            Some(_) => page.child(self.skill_editor(state, p, cx)),
            None => self.additions(page, state, p, cx),
        };
        page.when(state.rewrite.is_some(), |d| {
            d.children(self.skill_links(skill, p, cx))
        })
        .child(Self::enforced_boxes(skill, p))
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

    /// Reset, Rewrite, Undo and a restore from before a merge ask first; Cancel sends nothing.
    fn confirm_dialog(
        state: &SkillState,
        confirm: Confirm,
        unsaved: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let ([ask, what, kept], command) = question(state, &confirm, unsaved);
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
            Confirm::Undo { .. } => act.primary().label("Undo merge"),
            Confirm::Restore { .. } => act.primary().label("Restore"),
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

    /// The note of the merge the skill holds now, with Review changes, Undo and Dismiss.
    fn merge_note_banner(
        &self,
        state: &SkillState,
        note: &MergeNote,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let skill = state.skill;
        let [title, text] = merge_note_text(state, note, self.merge_model(note).as_deref());
        let at = written_at(&note.entry)
            .map_or(note.entry.clone(), |at| when(at, Local::now().date_naive()));
        let (review, undo, dismiss) = (note.entry.clone(), note.clone(), note.entry.clone());
        banner(p.green)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(banner_head("git-merge", p.green, title).flex_1())
                    .child(icon("laptop").size(px(13.)).text_color(p.green))
                    .child(
                        div()
                            .text_color(p.green)
                            .child(format!("{} · {at}", merging_mac(note))),
                    ),
            )
            .child(hint(text, p))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("review-merge")
                            .primary()
                            .label("Review changes")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.open_review(skill, review.clone(), true, w, c)
                            })),
                    )
                    .child(
                        button("undo-merge")
                            .ghost()
                            .icon(icon("undo").size(px(13.)))
                            .label("Undo")
                            .on_click(cx.listener(move |v, _, _, c| {
                                let note = undo.clone();
                                v.confirm(Confirm::Undo { note, later: false }, c)
                            })),
                    )
                    .child(
                        button("dismiss-merge")
                            .link()
                            .label("Dismiss")
                            .on_click(cx.listener(move |v, _, w, c| {
                                let entry = dismiss.clone();
                                v.request(Command::DismissSkillMerge { skill, entry }, w, c)
                            })),
                    ),
            )
    }

    /// "Claude · Haiku · Medium", as pickers show the model that merged.
    fn merge_model(&self, note: &MergeNote) -> Option<String> {
        (note.model.as_ref()).map(|model| models::profile_label(&self.model_catalog, &model.model))
    }

    /// An out-of-date rewrite's banner, with what it offers; or, for a base from a newer
    /// release, what this Mac does.
    fn rewrite_banner_view(
        &self,
        state: &SkillState,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let ([title, text], offers) = rewrite_banner(state)?;
        let skill = state.skill;
        let mergeable = (state.rewrite.as_ref()).is_some_and(|r| !r.base_version.is_empty());
        let undone = state.merge_standing == Some(MergeStanding::Undone);
        let merge = |label: String| {
            button("merge-update").label(label).on_click(
                cx.listener(move |v, _, w, c| v.request(Command::MergeSkillUpdate { skill }, w, c)),
            )
        };
        let mark =
            button("mark-up-to-date")
                .ghost()
                .label("Mark as up to date")
                .on_click(cx.listener(move |v, _, w, c| {
                    v.request(Command::MarkSkillUpToDate { skill }, w, c)
                }));
        let actions = match offers {
            Offers::Nothing => None,
            Offers::MergeIn => Some(
                div()
                    .flex()
                    .gap_2()
                    .when(mergeable, |d| {
                        let merge = merge(format!("Merge {} in", state.stock_version));
                        d.child(if undone {
                            merge.ghost()
                        } else {
                            merge.primary()
                        })
                    })
                    .child(mark)
                    .child(
                        button("banner-reset")
                            .ghost()
                            .label("Reset to stock")
                            .on_click(cx.listener(|v, _, _, c| v.confirm(Confirm::Reset, c))),
                    ),
            ),
            Offers::TryAgain => Some(
                div()
                    .flex()
                    .gap_2()
                    .child(merge("Try again".into()).primary())
                    .child(mark),
            ),
            Offers::Wordings => Some(self.wordings_view(state, p, cx)),
        };
        let (banner, head) = match &state.merge_standing {
            Some(MergeStanding::Undone) => (quiet_banner(p), Some(("undo", p.subtle))),
            Some(MergeStanding::NewerBase { .. }) => (quiet_banner(p), Some(("laptop", p.subtle))),
            Some(MergeStanding::WaitingForSync | MergeStanding::Merging { .. }) => {
                (quiet_banner(p), Some(("git-merge", p.subtle)))
            }
            Some(MergeStanding::Failed { .. } | MergeStanding::NeedsModel { .. }) => {
                (banner(p.yellow), Some(("alert", p.yellow)))
            }
            None => (banner(p.yellow), None),
        };
        let title = match head {
            Some((glyph, color)) => banner_head(glyph, color, title),
            None => div().font_weight(FontWeight::MEDIUM).child(title),
        };
        Some((banner.child(title).child(hint(text, p))).children(actions))
    }

    /// Choose a model, or a wording for each passage that overlaps: with one passage, a choice
    /// applies at once; with several, Apply does once each has one.
    fn wordings_view(&self, state: &SkillState, p: Palette, cx: &mut Context<Self>) -> Div {
        let Some(MergeStanding::NeedsModel { passages, .. }) = &state.merge_standing else {
            return div();
        };
        let (skill, stock) = (state.skill, &state.stock_version);
        let answer = |id: &'static str, label: String, wordings: Option<Vec<Wording>>| {
            button(id)
                .label(label)
                .disabled(wordings.is_none())
                .on_click(cx.listener(move |v, _, w, c| {
                    let wordings = wordings.clone().unwrap_or_default();
                    v.request(Command::AnswerSkillQuestion { skill, wordings }, w, c)
                }))
        };
        let models = button("choose-model")
            .ghost()
            .label("Choose a model in Models")
            .on_click(cx.listener(|v, _, w, c| v.open_page(Page::Models, w, c)));
        let (take, keep) = (format!("Take {stock}'s wording"), "Keep my wording");
        if let [_] = &passages[..] {
            return div()
                .flex()
                .gap_2()
                .child(models)
                .child(answer("take-stock", take, Some(vec![Wording::Stock])).ghost())
                .child(answer("keep-mine", keep.into(), Some(vec![Wording::Yours])).ghost());
        }
        let chosen = |index: usize| self.skill_wordings.get(index).copied().flatten();
        let rows = passages.iter().enumerate().map(|(index, passage)| {
            let pick = |wording: Wording, label: String| {
                button(("wording", index * 2 + wording as usize))
                    .ghost()
                    .label(label)
                    .selected(chosen(index) == Some(wording))
                    .on_click(cx.listener(move |v, _, _, c| {
                        v.skill_wordings
                            .resize(v.skill_wordings.len().max(index + 1), None);
                        v.skill_wordings[index] = Some(wording);
                        c.notify();
                    }))
            };
            let wording = |text: &str| Self::read_only_text(text.trim_end().to_owned(), p).flex_1();
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(hint(format!("Passage {}", index + 1), p))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(wording(&passage.stock))
                        .child(wording(&passage.yours)),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(pick(Wording::Stock, take.clone()))
                        .child(pick(Wording::Yours, keep.into())),
                )
        });
        let wordings = (0..passages.len()).map(chosen).collect();
        div().flex().flex_col().gap_3().children(rows).child(
            div()
                .flex()
                .gap_2()
                .child(answer("apply-wordings", "Apply".into(), wordings).primary())
                .child(models),
        )
    }

    /// What changed from the rewrite's base to the stock text, as removed and added lines.
    fn stock_changes_view(&self, state: &SkillState, base_version: &str, p: Palette) -> Div {
        let shown = shown_changes(&self.stock_changes, is_change);
        let rows = shown.into_iter().map(|shown| match shown {
            Shown::Line(line) => diff_row(line, p.green, p),
            Shown::Gap(count) => gap_row(count, p),
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
            .child(diff_block(p).children(rows))
    }

    /// The rewrite before and after a merge, line by line, each change labelled with where it
    /// came from, with Undo and Done.
    fn review_page(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Div {
        let page = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.skills_crumb(skill.name(), p, cx));
        let Some(review) = &self.merge_review else {
            return page;
        };
        let note = &review.note;
        let at = written_at(&note.entry)
            .map_or(note.entry.clone(), |at| when(at, Local::now().date_naive()));
        let versions = format!("{} → {} · {} · {at}", note.from, note.to, merging_mac(note));
        let model = self.merge_model(note);
        let counts = merge_counts(note, model.as_deref()) + &merge_tokens(note);
        let label = |source| match source {
            Source::Update => format!("From {}", note.to),
            Source::Model => "Merged by the model".into(),
            Source::Human => "Chosen by you".into(),
        };
        let added = |source| match source {
            Some(Source::Update) | None => p.green,
            Some(_) => p.focus,
        };
        let shown = shown_changes(&review.lines, |reviewed: &Reviewed| {
            reviewed.source.is_some()
        });
        let mut rows = vec![];
        for (index, item) in shown.iter().enumerate() {
            let Reviewed { line, source } = match item {
                Shown::Line(reviewed) => reviewed,
                Shown::Gap(count) => {
                    rows.push(gap_row(*count, p));
                    continue;
                }
            };
            rows.push(diff_row(line, added(*source), p));
            // A run of one source's changes ends with its label.
            let next = match shown.get(index + 1) {
                Some(Shown::Line(next)) => next.source,
                _ => None,
            };
            if let Some(source) = source.filter(|source| next != Some(*source)) {
                let tag = hint(label(source), p)
                    .px_2()
                    .text_color(added(Some(source)));
                rows.push(div().flex().justify_end().child(tag));
            }
        }
        let undo = Confirm::Undo {
            note: note.clone(),
            later: !review.live,
        };
        page.child(
            Self::skill_heading(
                "history",
                "What the merge changed in your rewrite",
                hint(versions, p),
            )
            .child(div().flex_1())
            .child(
                button("review-undo")
                    .ghost()
                    .label("Undo")
                    .on_click(cx.listener(move |v, _, _, c| v.confirm(undo.clone(), c))),
            )
            .child(button("review-done").primary().label("Done").on_click(
                cx.listener(move |v, _, w, c| v.open_skill(SkillView::Skill(skill), w, c)),
            )),
        )
        .child(hint(counts, p))
        .child(diff_block(p).children(rows))
    }

    /// Newest first, each entry with View and Restore, except the newest when it holds what is
    /// in effect now; a merge's entry with Review and Undo this merge instead of View.
    fn history_page(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Div {
        let today = Local::now().date_naive();
        let crumb = self.skills_crumb(skill.name(), p, cx);
        let state = self.skill_state(skill);
        let entries: Vec<_> = (self.skill_history.iter().enumerate())
            .map(|(index, recorded)| {
                let at =
                    written_at(&recorded.name).map_or(recorded.name.clone(), |at| when(at, today));
                let live = (recorded.entry.as_ref().ok())
                    .zip(state)
                    .is_some_and(|(entry, state)| is_live(entry, state));
                let merged_since = merged_since(&self.skill_history, index);
                let shown = (at, index == 0 && live, live, merged_since);
                self.history_entry(skill, recorded, shown, p, cx)
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

    /// An entry written `at`, `current` when it is the newest and in effect, `live` when it is
    /// in effect, and from before the merge of `merged_since`.
    fn history_entry(
        &self,
        skill: Skill,
        recorded: &Recorded,
        (at, current, live, merged_since): (String, bool, bool, Option<String>),
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
                        match merged_since.clone() {
                            Some(merged) => v.confirm(Confirm::Restore { entry, merged }, c),
                            None => v.request(Command::RestoreSkill { skill, entry }, w, c),
                        }
                    }));
                let merge = merge_note(&name, entry);
                let merge_actions = merge.clone().map(|note| {
                    let entry = note.entry.clone();
                    let undo = Confirm::Undo { note, later: !live };
                    [
                        action("review-entry", "Review").on_click(cx.listener(
                            move |v, _, w, c| v.open_review(skill, entry.clone(), false, w, c),
                        )),
                        action("undo-entry", "Undo this merge")
                            .on_click(cx.listener(move |v, _, _, c| v.confirm(undo.clone(), c))),
                    ]
                });
                let detail = match &merge {
                    Some(note) => format!(
                        "{} · {}{}",
                        merging_mac(note),
                        merge_counts(note, self.merge_model(note).as_deref()),
                        merge_tokens(note)
                    ),
                    None => entry_detail(entry),
                };
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
                    hint(detail, p),
                    div()
                        .flex()
                        .gap_3()
                        .when(merge.is_none(), |d| d.child(view))
                        .children(merge_actions.into_iter().flatten())
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
        let newer = (state.newer_stock_version.as_ref()).map(|version| {
            let skill = state.skill.name();
            hint(
                format!("Another machine runs a newer Wiffletree: {skill} {version}."),
                p,
            )
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
            .children(newer)
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
        CORE, Confirm, OUT_OF_DATE, Offers, Shown, based_on, core_tab, entry_detail, entry_time,
        entry_title, grouped, is_change, is_live, list_who, merge_counts, merge_note_text,
        merged_since, out_of_date, out_of_date_title, question, reset_question, rewrite_banner,
        rewrite_question, saved_text, shown_changes, state_label, time_or_date, when, written_at,
    };
    use chrono::{Local, NaiveDate, TimeZone};
    use workspace_core::{
        Command, MergeModel, MergeNote, MergePassage, MergeStanding, ModelProfile, Provider, Role,
        Skill, SkillState,
    };
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
        std::fs::remove_file(home.path().join("shared/skills/coordinating/base.md")).unwrap();
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
                "Your rewrite stops applying from each agent's next turn. Coordinators get the stock text only.",
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
        std::fs::write(home.path().join("shared/skills/coordinating/base.md"), base).unwrap();

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
        let shown = shown_changes(&lines, is_change);
        assert!(
            matches!(&shown[..2], [Shown::Line(Line::Removed(v0)), Shown::Line(Line::Added(_))] if v0 == "# Coordinating — v0")
        );
        assert_eq!(
            shown.last(),
            Some(&Shown::Line(&Line::Removed("A line v1 removed.".into())))
        );

        let banner = rewrite_banner(coordinating).unwrap();
        let title = "Written against Coordinating v0. This release has v1.";
        assert_eq!(
            banner,
            ([title.into(), OUT_OF_DATE.into()], Offers::MergeIn)
        );
        let passage = MergePassage {
            base: "Ask the human when.\n\n".into(),
            stock: "Ask the human instead when.\n\n".into(),
            yours: "Ask me when.\n\n".into(),
        };
        let failed =
            "The model's text for passage 1 was longer than v1's version and yours together.";
        let standings = [
            (
                MergeStanding::WaitingForSync,
                [
                    title,
                    "Waiting for this launch's first sync, so your Macs merge v1 only once. If another Mac already merged it, this Mac takes that.",
                ],
                Offers::Nothing,
            ),
            (
                MergeStanding::Merging { passages: 1 },
                [
                    "Merging v1 into your rewrite",
                    "The PR keeper's model is merging the 1 passage you and v1 both changed. Your rewrite runs as you wrote it until the merge applies.",
                ],
                Offers::Nothing,
            ),
            (
                MergeStanding::Failed {
                    reason: failed.into(),
                },
                [
                    "The merge of v1 couldn't finish",
                    &format!(
                        "{failed} Nothing was changed, and your rewrite runs as you wrote it. Wiffletree tries again at the next start."
                    ),
                ],
                Offers::TryAgain,
            ),
            (
                MergeStanding::NeedsModel {
                    passages: vec![passage.clone()],
                    codex: false,
                },
                [
                    "v1 and your rewrite both change 1 passage",
                    "Merging it needs a Claude model for the PR keeper, and none is chosen. Your rewrite runs as you wrote it. Choose a model, or choose a wording for the passage.",
                ],
                Offers::Wordings,
            ),
            (
                MergeStanding::NeedsModel {
                    passages: vec![passage.clone(), passage],
                    codex: true,
                },
                [
                    "v1 and your rewrite both change 2 passages",
                    "Merging them needs a Claude model for the PR keeper, and the one chosen is a Codex model. Your rewrite runs as you wrote it. Choose a model, or choose a wording for each passage.",
                ],
                Offers::Wordings,
            ),
        ];
        for (standing, [title, text], offers) in standings {
            let mut shown = coordinating.clone();
            shown.merge_standing = Some(standing);
            // The list keeps its mark and the sidebar its dot.
            assert_eq!(state_label(&shown), "Rewritten · out of date");
            assert!(out_of_date(&shown));
            assert_eq!(
                rewrite_banner(&shown),
                Some(([title.into(), text.into()], offers))
            );
        }
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
            shown_changes(&lines, is_change),
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
        let rules = home.path().join("shared/skills/implementing/additions.md");
        std::fs::write(&rules, "Rule\nEdited outside the app").unwrap();
        assert!(!live(&host));
        std::fs::write(&rules, b"\xff not text").unwrap();
        assert!(!live(&host));
        std::fs::write(&rules, "Rule").unwrap();

        // A rewrite whose base is out of date is current while it is what agents get.
        host.skills().start_rewrite(skill).unwrap();
        std::fs::write(
            home.path().join("shared/skills/implementing/base.md"),
            "# Implementing — v0\n",
        )
        .unwrap();
        host.skills().save_rewrite(skill, "My way").unwrap();
        assert!(live(&host));
        // A base edited outside the app, at the same version, is not what the entry holds.
        std::fs::write(
            home.path().join("shared/skills/implementing/base.md"),
            "# Implementing — v0\nEdited outside the app\n",
        )
        .unwrap();
        assert!(!live(&host));
        // So is a rewrite based on a newer release's stock text, which is not out of date.
        std::fs::write(
            home.path().join("shared/skills/implementing/base.md"),
            "# Implementing — v9\n",
        )
        .unwrap();
        host.skills().save_rewrite(skill, "My way").unwrap();
        assert!(live(&host));
        std::fs::write(
            home.path().join("shared/skills/implementing/base.md"),
            "# Implementing — v0\nEdited outside the app\n",
        )
        .unwrap();
        host.skills().save_rewrite(skill, "My way").unwrap();
        let entry = newest();
        assert_eq!(entry_title(&entry), "Saved your rewrite");
        assert_eq!(entry_detail(&entry), "Based on stock v0 · 6 characters");
    }

    /// Coordinating rewritten against a stand-in v0 and merged into v1 at start.
    fn merged_at_start(home: &tempfile::TempDir) -> Host {
        let host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        host.skills().start_rewrite(skill).unwrap();
        let base = stock(skill).replacen("— v1", "— v0", 1);
        let text = base.replacen("Start by clarifying", "Start by restating", 1);
        host.skills().save_rewrite(skill, &text).unwrap();
        std::fs::write(
            home.path().join("shared/skills/coordinating/base.md"),
            &base,
        )
        .unwrap();
        host.skills().save_rewrite(skill, &text).unwrap();
        drop(host);
        Host::open(home.path()).unwrap()
    }

    #[test]
    fn after_an_automatic_merge_the_page_shows_its_note_and_undo_asks_first() {
        let home = tempfile::tempdir().unwrap();
        let mut host = merged_at_start(&home);
        let skill = Skill::Coordinating;
        let snapshot = host.snapshot().unwrap();
        let coordinating = &snapshot.skills[0];
        assert_eq!(state_label(coordinating), "Rewritten · updated");
        assert!(!snapshot.skills.iter().any(out_of_date));
        let note = coordinating.merge_note.clone().unwrap();
        assert_eq!(
            merge_note_text(coordinating, &note, None),
            [
                "Merged Coordinating v1 into your rewrite",
                "1 change from v1 came in as plain text. Coordinators get the merged text from their next turn.",
            ]
        );

        let undo = Confirm::Undo {
            note: note.clone(),
            later: false,
        };
        let (asked, command) = question(coordinating, &undo, false);
        let before = time_or_date(&note.entry);
        assert_eq!(
            asked,
            [
                "Undo the merge of v1?".into(),
                format!(
                    "Your rewrite goes back to exactly what it was before {before}, based on stock v0. Coordinators get it from their next turn, on every Mac."
                ),
                "v1 won't be merged in again on its own. The merge stays in the history.".into(),
            ]
        );
        let undone = state(&mut host, command);
        assert_eq!(state_label(&undone), "Rewritten · out of date");
        let (banner, offers) = rewrite_banner(&undone).unwrap();
        assert_eq!(
            banner,
            [
                "You undid the merge of v1",
                "Your rewrite is as it was before the merge, and v1 won't be merged into it again on its own.",
            ]
        );
        assert_eq!(offers, Offers::MergeIn);
        let history = listing(&mut host, skill);
        assert_eq!(
            titles(&history)[..2],
            ["Undid the merge of v1", "Merged v0 → v1"]
        );
        // After a later change, the merge's entry still offers Undo, which says so.
        let later = Confirm::Undo { note, later: true };
        let ([_, what, kept], _) = question(&undone, &later, false);
        assert!(what.ends_with(
            "based on stock v0, and your changes since stop applying. Coordinators get it from their next turn, on every Mac."
        ));
        assert_eq!(
            kept,
            "v1 won't be merged in again on its own. The merge and your changes since stay in the history."
        );
    }

    #[test]
    fn restoring_a_rewrite_from_before_a_merge_asks_first() {
        let home = tempfile::tempdir().unwrap();
        let mut host = merged_at_start(&home);
        let skill = Skill::Coordinating;
        let history = listing(&mut host, skill);
        assert_eq!(
            titles(&history),
            [
                "Merged v0 → v1",
                "Saved your rewrite",
                "Saved your rewrite",
                "Started a rewrite"
            ]
        );
        assert_eq!(merged_since(&history, 0), None);
        assert_eq!(merged_since(&history, 1).as_deref(), Some("v1"));
        let entry = history[1].name.clone();
        let restore = Confirm::Restore {
            entry: entry.clone(),
            merged: "v1".into(),
        };
        let coordinating = host.snapshot().unwrap().skills.remove(0);
        let (asked, command) = question(&coordinating, &restore, false);
        assert_eq!(
            asked,
            [
                format!("Restore the version of {}?", entry_time(&entry)),
                "It is from before the merge of v1, so no automatic merge brings v1 in until a newer version arrives. You can still merge it in from Coordinating's page.".into(),
                "The merge and every later change stay in the history.".into(),
            ]
        );
        let restored = state(&mut host, command);
        assert_eq!(restored.merge_standing, Some(MergeStanding::Undone));
        drop(host);
        let host = Host::open(home.path()).unwrap();
        let coordinating = host.snapshot().unwrap().skills.remove(0);
        assert_eq!(state_label(&coordinating), "Rewritten · out of date");
    }

    #[test]
    fn a_base_from_a_newer_release_names_it_and_offers_no_mark_as_up_to_date() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        state(&mut host, Command::StartSkillRewrite { skill });
        let base = stock(skill).replacen("— v1", "— v2", 1);
        std::fs::write(home.path().join("shared/skills/coordinating/base.md"), base).unwrap();
        let mut coordinating = host.snapshot().unwrap().skills.remove(0);
        assert_eq!(state_label(&coordinating), "Rewritten");
        let here = "This Mac has v1 and sends the rewrite as written. It isn't out of date here, and nothing is merged here.";
        assert_eq!(
            rewrite_banner(&coordinating),
            Some((
                [
                    "Based on Coordinating v2, from a Mac with a newer Wiffletree".into(),
                    here.into()
                ],
                Offers::Nothing
            ))
        );
        coordinating.merge_standing = Some(MergeStanding::NewerBase {
            machine: Some("Mike's MacBook Air".into()),
        });
        assert_eq!(
            rewrite_banner(&coordinating),
            Some((
                [
                    "Based on Coordinating v2, from Mike's MacBook Air".into(),
                    format!(
                        "That Mac runs a newer Wiffletree and merged v2 into your rewrite. {here}"
                    )
                ],
                Offers::Nothing
            ))
        );
    }

    #[test]
    fn a_note_names_the_model_that_merged_passages() {
        let note = MergeNote {
            entry: "2026-10-16T08-01-00.000Z".into(),
            from: "v1".into(),
            to: "v2".into(),
            from_update: 3,
            by_model: 1,
            model: Some(MergeModel {
                model: ModelProfile {
                    provider: Provider::Claude,
                    model: "haiku".into(),
                    effort: "medium".into(),
                },
                tokens: 6_240,
            }),
            machine: None,
        };
        assert_eq!(
            merge_counts(&note, Some("Claude · Haiku · Medium")),
            "3 changes from v2, 1 passage merged by Claude · Haiku · Medium"
        );
        assert_eq!(super::merge_tokens(&note), " · 6,240 tokens");
        let home = tempfile::tempdir().unwrap();
        let state = Host::open(home.path())
            .unwrap()
            .snapshot()
            .unwrap()
            .skills
            .remove(0);
        let [_, text] = merge_note_text(&state, &note, Some("Claude · Haiku · Medium"));
        assert_eq!(
            text,
            "3 changes from v2 came in as plain text. You had also changed 1 passage v2 changes, so Claude · Haiku · Medium merged that one, keeping your wording where both could stand. Coordinators get the merged text from their next turn."
        );
    }

    #[test]
    fn the_list_dates_the_merge_it_notes() {
        let home = tempfile::tempdir().unwrap();
        let host = merged_at_start(&home);
        let mut coordinating = host.snapshot().unwrap().skills.remove(0);
        assert!(
            list_who(&coordinating)
                .starts_with("Project coordinator · v1 merged into your rewrite today at ")
        );
        let note = coordinating.merge_note.as_mut().unwrap();
        note.entry = "2020-10-14T12-00-00.000Z".into();
        assert!(list_who(&coordinating).contains(" merged into your rewrite on 14 Oct at "));
        coordinating.merge_note = None;
        assert_eq!(list_who(&coordinating), "Project coordinator");
    }
}
