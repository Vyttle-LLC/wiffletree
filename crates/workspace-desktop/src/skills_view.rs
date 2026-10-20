//! The Skills page: the four skills and the fixed core, and each skill's page, where the human
//! adds their own rules after the stock text or rewrites it, resets it to stock, restores an
//! earlier version from its history, and reviews or undoes a merge of a new stock version. No
//! agent tool reaches what this page changes.
use super::*;
use chrono::{DateTime, Local, NaiveDateTime};
use gpui_component::{Disableable, Selectable, button::ButtonVariants};
use ui::{card, confirm_card, eyebrow, hint, icon, over_dimmed, pill, stacked_banner};
use wording::{counted, date_and_time, time_of_day, when};
use workspace_core::skills::{
    Change, CoreInstructions, CoreText, Entry, History, Line, MergeReview, Recorded, Reviewed,
    Rewritten, merge::Source, merge_note, version,
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

/// The page draws again for another workspace or catalog: merge notes and questions name their
/// models from the catalog.
fn redraws(old: &Shared, new: &Shared) -> bool {
    let (_, moved) = new.moved_from(old);
    moved || !Rc::ptr_eq(&old.catalog, &new.catalog)
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

const CORE_PURPOSE: &str = "How every agent talks to Wiffletree: its tools, its reports and findings, and the rules the app enforces whatever a skill says.";
const NO_PR_SKILL: &str = "Opening a pull request isn't a skill. Agents write pull requests by your repository's rules and your own, so put yours in Implementing's rules.";
const CORE_HINT: &str = "Wiffletree depends on these instructions: the names of its tools, how reports and findings are written, and how messages and pull requests work. They can't be edited, so no change to a skill can break them. Each agent gets the contract and its role's part, then its skill.";
const PR_RULES_HINT: &str = "Agents write pull requests by your repository's rules and your own, so put your PR rules here.";
const APPLIES: &str =
    "Applies from each agent's next turn, and a Codex agent's from its next new session";
const REWRITE_COST: &str = "When an update changes the stock text, Wiffletree merges the change into your rewrite and leaves a note you can review or undo. Where you also changed what the update changes, the PR keeper's Claude choice in Models merges that passage, or, without one, you choose the wording. Adding your own rules never needs a merge.";
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

/// "Stock", "Stock and N of your rules", "Rewritten", "Rewritten · updated", "Rewritten · needs
/// you" or "Rewritten · out of date".
fn state_label(state: &SkillState) -> String {
    match (&state.rewrite, rule_count(&state.rules)) {
        (Some(_), _) if state.question.is_some() => "Rewritten · needs you".into(),
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

/// A passage the human chooses a wording for: the model's question when it asked one, then
/// the update's wording and the human's.
struct Choice<'a> {
    question: Option<&'a str>,
    stock: &'a str,
    yours: &'a str,
}
impl<'a> Choice<'a> {
    fn asked(question: &'a SkillQuestion) -> Vec<Self> {
        let choice = |passage: &'a QuestionedPassage| Choice {
            question: Some(&passage.question),
            stock: &passage.stock,
            yours: &passage.yours,
        };
        question.passages.iter().map(choice).collect()
    }
    fn overlapping(passages: &'a [MergePassage]) -> Vec<Self> {
        let choice = |passage: &'a MergePassage| Choice {
            question: None,
            stock: &passage.stock,
            yours: &passage.yours,
        };
        passages.iter().map(choice).collect()
    }
}

/// The wordings chosen so far for one skill's passages, until Apply, with the update's and the
/// human's wording of the passages they were chosen for, so passages that change start over.
#[derive(Default)]
pub(super) struct Chosen {
    passages: Vec<(String, String)>,
    wordings: Vec<Option<Wording>>,
}
impl Chosen {
    fn passages(choices: &[Choice<'_>]) -> Vec<(String, String)> {
        let wordings = |choice: &Choice<'_>| (choice.stock.to_owned(), choice.yours.to_owned());
        choices.iter().map(wordings).collect()
    }
    fn get(&self, passages: &[(String, String)], index: usize) -> Option<Wording> {
        (self.passages == passages)
            .then(|| self.wordings.get(index).copied().flatten())
            .flatten()
    }
    fn choose(&mut self, passages: &[(String, String)], index: usize, wording: Wording) {
        if self.passages != passages {
            self.passages = passages.to_vec();
            self.wordings = vec![None; passages.len()];
        }
        self.wordings[index] = Some(wording);
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
                    "Your rewrite is exactly as it was before the merge, and {stock} won't be merged into it again on its own."
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
        (Some(MergeStanding::Failed { reason }), true) => {
            let stop = if reason.ends_with(['.', '!', '?', '…']) {
                ""
            } else {
                "."
            };
            (
                [
                    format!("The merge of {stock} couldn't finish"),
                    format!(
                        "{reason}{stop} Nothing was changed, and your rewrite runs as you wrote it. Wiffletree tries again at the next start."
                    ),
                ],
                Offers::TryAgain,
            )
        }
        (Some(MergeStanding::NeedsModel { passages, codex }), true) => {
            let (it, which) = match passages.len() {
                1 => ("it", "the passage"),
                _ => ("them", "each passage"),
            };
            let chosen = match codex {
                true => "its row has only Codex models",
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

/// "one passage" or "3 passages", as a question counts them.
fn passages_in_question(question: &SkillQuestion) -> String {
    match question.passages.len() {
        1 => "one passage".into(),
        count => format!("{count} passages"),
    }
}

/// A question's banner on the skill's page, `model` having asked it: its title and text.
fn question_banner(question: &SkillQuestion, model: &str) -> [String; 2] {
    let to = &question.to;
    let others = match question.other_changes {
        0 => String::new(),
        count => format!("{to}'s other {} merge cleanly. ", counted(count, "change")),
    };
    let it = match question.passages.len() {
        1 => "this passage",
        _ => "these passages",
    };
    [
        format!(
            "{to} and your rewrite disagree about {}",
            passages_in_question(question)
        ),
        format!(
            "{others}{model} didn't merge {it}, because the two want different things. Nothing changes until you choose."
        ),
    ]
}

/// A question's workspace note: the setting it names, as "Reviewing · v2", then its text and
/// what either choice brings in.
fn question_note(question: &SkillQuestion) -> (String, [String; 2]) {
    let skill = question.skill;
    let setting = format!("{} · {}", skill.name(), question.to);
    let gets = about(skill).gets;
    let gets = gets[..1].to_lowercase() + &gets[1..];
    let either = match question.other_changes {
        0 => String::new(),
        count => format!(
            "{}'s other {} merge either way. ",
            question.to,
            counted(count, "change")
        ),
    };
    let text = format!(
        "{setting} and your rewrite disagree about {}. Until you choose, {gets} your rewrite as you wrote it.",
        passages_in_question(question)
    );
    let either =
        format!("{either}Your choice applies on every Mac, and you can undo it like any merge.");
    (setting, [text, either])
}

/// Who uses the skill, and what the page notes: "Project coordinator · v2 merged into your
/// rewrite today at 08:01", or "on 14 Oct at 08:01"; or, for a question, "v2 and your rewrite
/// disagree about one passage".
fn list_who(state: &SkillState) -> String {
    let roles = about(state.skill).roles;
    if let Some(question) = &state.question {
        let passages = passages_in_question(question);
        return format!(
            "{roles} · {} and your rewrite disagree about {passages}",
            question.to
        );
    }
    let Some(note) = &state.merge_note else {
        return roles.into();
    };
    let when = match written_at(&note.entry) {
        Some(at) if at.date_naive() == Local::now().date_naive() => {
            format!("today at {}", time_of_day(at))
        }
        Some(at) => format!("on {}", at.format("%-d %b at %H:%M")),
        None => format!("at {}", note.entry),
    };
    format!("{roles} · {} merged into your rewrite {when}", note.to)
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

fn merge_tokens(note: &MergeNote) -> String {
    (note.model.as_ref()).map_or(String::new(), |model| {
        format!(" · {} tokens", grouped(model.tokens as usize))
    })
}

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
                1 => ("passage", "it"),
                _ => ("passages", "them"),
            };
            format!(
                " You had also changed {passages} {passage} that {} changes, so {} merged {one}, keeping your wording where both could stand.",
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
            "Your rewrite goes back to exactly what it was before {}, based on {from}{since}. {} it from their next turn, on every Mac.",
            time_or_date(entry),
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
        Some(at) if at.date_naive() == Local::now().date_naive() => time_of_day(at),
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
    written_at(name).map_or(name.into(), date_and_time)
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
        Change::Asked { to, .. } => format!("Asked you about {to}"),
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
        count => counted(count, "rule"),
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

/// A banner that only says where things stand, with no color of its own.
fn quiet_banner(p: Palette) -> Div {
    stacked_banner(p.edge)
        .border_color(p.edge.opacity(edge::FIRM))
        .bg(p.base)
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
        .border_color(p.edge.opacity(edge::BOX))
        .bg(p.base)
        .overflow_hidden()
        .font_family(".SystemMonoFont")
        .text_size(px(font::DETAIL))
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
    let unchanged = format!("{} unchanged", counted(count, "line"));
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
fn core_tab(part: &CoreInstructions) -> String {
    let label = match part.audience.as_str() {
        "every agent" => "Contract",
        "coordinators" => "Coordinator",
        "implementers" => "Implementer",
        "testers and reviewers" => "Verifier",
        "the PR keeper" => "PR keeper",
        _ => &part.name,
    };
    format!("{label} {}", part.version)
}

/// Any rewrite is out of date, which the sidebar's Skills row marks.
pub(super) fn a_rewrite_is_out_of_date(shared: &Shared) -> bool {
    (shared.snapshot()).is_some_and(|snapshot| snapshot.skills.iter().any(out_of_date))
}

/// The Skills page: each skill's rules or rewrite, its history, merges and stock changes.
pub(super) struct SkillsPage {
    bridge: Bridge,
    shared: Shared,
    skill_view: SkillView,
    /// The open skill's rules or rewrite as the human is editing them.
    skill_text: Entity<TextareaState>,
    stock_unfolded: bool,
    /// What the open skill's page is asking to confirm.
    confirming: Option<Confirm>,
    /// The open history, newest first, and the entry whose text is shown.
    skill_history: History,
    viewed_entry: Option<String>,
    /// What changed from the open rewrite's base to this release's stock text.
    stock_changes: Vec<Line>,
    /// The merge open in Review changes.
    merge_review: Option<MergeReview>,
    /// Review changes was opened from a History entry, which Done returns to.
    review_from_history: bool,
    /// By skill, the wording chosen so far for each passage, until Apply.
    skill_wordings: [Chosen; 4],
    /// The saved review levels, which the core page lists.
    verifiers: Entity<verifiers::VerifierEditor>,
    /// The running host's core instructions with their texts, read when the core page opens.
    core: Vec<CoreText>,
}
impl EventEmitter<Intent> for SkillsPage {}
impl SkillsPage {
    pub(super) fn new(
        bridge: Bridge,
        verifiers: Entity<verifiers::VerifierEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let skill_text = cx.new(|cx| {
            TextareaState::new(window, cx).auto_grow(6, 24).placeholder(
                "One rule per line, e.g. Run the repository's checks before reporting ready.",
            )
        });
        // Save and Discard follow the draft.
        cx.subscribe(&skill_text, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        Self {
            bridge,
            shared: Shared::default(),
            skill_view: SkillView::List,
            skill_text,
            stock_unfolded: false,
            confirming: None,
            skill_history: History::default(),
            viewed_entry: None,
            stock_changes: vec![],
            merge_review: None,
            review_from_history: false,
            skill_wordings: Default::default(),
            verifiers,
            core: vec![],
        }
    }
    /// An editor without unsaved edits follows the saved rules or rewrite in each snapshot.
    pub(super) fn share(&mut self, shared: Shared, window: &mut Window, cx: &mut Context<Self>) {
        let (_, moved) = shared.moved_from(&self.shared);
        let redraw = redraws(&self.shared, &shared);
        if moved && let Some(snapshot) = shared.snapshot() {
            self.follow_saved_rules(snapshot, window, cx);
        }
        self.shared = shared;
        if redraw {
            cx.notify();
        }
    }

    fn skill_state(&self, skill: Skill) -> Option<&SkillState> {
        (self.shared.snapshot()?.skills.iter()).find(|state| state.skill == skill)
    }

    fn skill_draft(&self, cx: &App) -> String {
        self.skill_text.read(cx).value().to_string()
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
        self.skill_wordings = Default::default();
        if let SkillView::Skill(skill) = view {
            let state = self.skill_state(skill).cloned();
            let text = state.as_ref().map_or("", saved_text).to_owned();
            self.skill_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
            self.stock_unfolded = false;
            self.stock_changes.clear();
            if state.as_ref().is_some_and(out_of_date) {
                self.read_stock_changes(skill, window, cx);
            }
        }
        if let SkillView::History(skill) = view {
            self.skill_history = History::default();
            self.viewed_entry = None;
            self.read_history(skill, window, cx);
        }
        if matches!(view, SkillView::Core(_)) {
            self.read_core(window, cx);
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
        self.review_from_history = !from_note;
        self.read_review(skill, entry.clone(), window, cx);
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
                self.read_stock_changes(skill, window, cx);
            }
        }
        let restored_out_of_date = out_of_date(&state);
        let state_stock_version = state.stock_version.clone();
        self.skill_saved(state, cx);
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
        toast(done, window, cx);
        if self.skill_view == SkillView::History(skill) {
            self.read_history(skill, window, cx);
        }
    }

    fn read_core(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(
            ask::core_instructions(),
            window,
            cx,
            |view, core, window, cx| match core {
                Ok(core) => view.core = core,
                Err(error) => toast_error(error, window, cx),
            },
        );
    }

    fn read_history(&mut self, skill: Skill, window: &mut Window, cx: &mut Context<Self>) {
        let ask = ask::skill_history(skill);
        self.ask(
            ask,
            window,
            cx,
            move |view, history, window, cx| match history {
                Ok(history) if view.skill_view == SkillView::History(skill) => {
                    view.skill_history = history
                }
                Ok(_) => {}
                Err(error) => toast_error(error, window, cx),
            },
        );
    }

    fn read_review(
        &mut self,
        skill: Skill,
        entry: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask = ask::skill_merge_review(skill, entry);
        self.ask(
            ask,
            window,
            cx,
            move |view, review, window, cx| match review {
                Ok(review) if view.skill_view == SkillView::Review(skill) => {
                    view.merge_review = Some(review)
                }
                Ok(_) => {}
                Err(error) => toast_error(error, window, cx),
            },
        );
    }

    fn read_stock_changes(&mut self, skill: Skill, window: &mut Window, cx: &mut Context<Self>) {
        let ask = ask::skill_stock_changes(skill);
        self.ask(
            ask,
            window,
            cx,
            move |view, lines, window, cx| match lines {
                Ok(lines) if view.skill_view == SkillView::Skill(skill) => {
                    view.stock_changes = lines
                }
                Ok(_) => {}
                Err(error) => toast_error(error, window, cx),
            },
        );
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
        let shown = self.shared.snapshot().and_then(|s| saved(&s.skills));
        if shown == Some(self.skill_draft(cx))
            && let Some(text) = saved(&snapshot.skills)
        {
            self.skill_text
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
    }

    /// The host's answer to a save, shown until the next snapshot.
    fn skill_saved(&mut self, saved: SkillState, cx: &mut Context<Self>) {
        cx.emit(Intent::Amend(Amendment::Skill(Box::new(saved))));
    }

    /// The view, under what it asks to confirm.
    fn skills_page(&self, p: Palette, cx: &mut Context<Self>) -> Div {
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
        over_dimmed(page, Self::confirm_dialog(state, confirm, unsaved, p, cx))
    }

    fn skills_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let skills = self.shared.snapshot().into_iter().flat_map(|s| &s.skills);
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
            .border_color(p.edge.opacity(edge::CARD))
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
            .child(hint(who, p).text_size(px(font::CAPTION)))
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
            .text_size(px(font::CAPTION))
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
                    .text_size(px(font::HEADING))
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
            .border_color(p.edge.opacity(edge::CARD))
            .bg(p.surface)
            .text_size(px(font::SMALL))
            .line_height(relative(1.5))
            .child(text.into())
    }

    fn core_page(&self, part: usize, p: Palette, cx: &mut Context<Self>) -> Div {
        let crumb = self.skills_crumb("Fixed core", p, cx);
        let parts = self
            .shared
            .snapshot()
            .map_or(&[][..], |s| &s.core_instructions);
        let tabs = parts.iter().enumerate().map(|(tab, about)| {
            button(SharedString::from(format!("core-part-{tab}")))
                .ghost()
                .selected(tab == part)
                .label(core_tab(about))
                .tooltip(format!("Sent to {}", about.audience))
                .on_click(cx.listener(move |v, _, w, c| v.open_skill(SkillView::Core(tab), w, c)))
        });
        let text = self.core.get(part).map_or("Loading…", |core| &core.text);
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
            .child(Self::read_only_text(text.to_owned(), p))
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
            ui::gated(
                button("reset-skill")
                    .ghost()
                    .label("Reset to stock")
                    .disabled(at_stock),
                &self.shared.host,
            )
            .on_click(cx.listener(|v, _, _, c| v.confirm(Confirm::Reset, c))),
        ];
        let roles = about(skill).roles;
        let basis = match &state.rewrite {
            Some(rewrite) if rewrite.out_of_date => format!(
                "{roles} · your rewrite, based on {} · this release has {}",
                based_on(&rewrite.base_version),
                state.stock_version
            ),
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
                .text_size(px(font::SMALL))
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
        let cancel = button("cancel-skill").on_click(cx.listener(move |v, _, _, c| close(v, c)));
        confirm_card(ask, [what, kept], cancel, act, p).w(px(380.))
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
        stacked_banner(p.green)
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
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, _, c| {
                                let note = undo.clone();
                                v.confirm(Confirm::Undo { note, later: false }, c)
                            })),
                    )
                    .child(
                        button("dismiss-merge")
                            .link()
                            .label("Dismiss")
                            .map(|b| ui::gated(b, &self.shared.host))
                            .on_click(cx.listener(move |v, _, w, c| {
                                let entry = dismiss.clone();
                                v.request(Command::DismissSkillMerge { skill, entry }, w, c)
                            })),
                    ),
            )
    }

    /// "Claude · Haiku · Medium", as pickers show the model that merged.
    fn merge_model(&self, note: &MergeNote) -> Option<String> {
        (note.model.as_ref()).map(|model| models::profile_label(&self.shared.catalog, &model.model))
    }

    /// An out-of-date rewrite's banner, with what it offers; or, for a base from a newer
    /// release, what this Mac does.
    fn rewrite_banner_view(
        &self,
        state: &SkillState,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if let (Some(question), None) = (&state.question, &state.merge_standing) {
            return Some(self.question_banner_view(question, p, cx));
        }
        let ([title, text], offers) = rewrite_banner(state)?;
        let skill = state.skill;
        let mergeable = (state.rewrite.as_ref()).is_some_and(|r| !r.base_version.is_empty());
        let undone = state.merge_standing == Some(MergeStanding::Undone);
        let merge = |label: String| {
            ui::gated(button("merge-update").label(label), &self.shared.host).on_click(
                cx.listener(move |v, _, w, c| v.request(Command::MergeSkillUpdate { skill }, w, c)),
            )
        };
        let mark = ui::gated(
            button("mark-up-to-date")
                .ghost()
                .label("Mark as up to date"),
            &self.shared.host,
        )
        .on_click(
            cx.listener(move |v, _, w, c| v.request(Command::MarkSkillUpToDate { skill }, w, c)),
        );
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
                            .map(|b| ui::gated(b, &self.shared.host))
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
            Offers::Wordings => {
                let Some(MergeStanding::NeedsModel { passages, .. }) = &state.merge_standing else {
                    unreachable!("only a merge that needs a model offers wordings");
                };
                let models = button("choose-model")
                    .ghost()
                    .label("Choose a model in Models")
                    .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Models))));
                let choices = Choice::overlapping(passages);
                let extra = ("page", vec![models]);
                Some(self.wordings_view(skill, &state.stock_version, &choices, extra, p, cx))
            }
        };
        let (banner, head) = match &state.merge_standing {
            Some(MergeStanding::Undone) => (quiet_banner(p), Some(("undo", p.subtle))),
            Some(MergeStanding::NewerBase { .. }) => (quiet_banner(p), Some(("laptop", p.subtle))),
            Some(MergeStanding::WaitingForSync | MergeStanding::Merging { .. }) => {
                (quiet_banner(p), Some(("git-merge", p.subtle)))
            }
            Some(MergeStanding::Failed { .. } | MergeStanding::NeedsModel { .. }) => {
                (stacked_banner(p.yellow), Some(("alert", p.yellow)))
            }
            None => (stacked_banner(p.yellow), None),
        };
        let title = match head {
            Some((glyph, color)) => banner_head(glyph, color, title),
            None => div().font_weight(FontWeight::MEDIUM).child(title),
        };
        Some((banner.child(title).child(hint(text, p))).children(actions))
    }

    /// A wording for each passage, `extra` beside the choices: with one passage, a choice
    /// applies at once; with several, Apply does once each has one. A passage the model asked
    /// about shows its question and both wordings, as several passages do. `place` keeps the
    /// Inbox's buttons apart from the page's.
    fn wordings_view(
        &self,
        skill: Skill,
        stock: &str,
        choices: &[Choice<'_>],
        (place, extra): (&str, Vec<Button>),
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = |name: &str| SharedString::from(format!("{place}-{}-{name}", skill.folder()));
        let answer = |name: &str, label: String, wordings: Option<Vec<Wording>>| {
            button(id(name))
                .label(label)
                .disabled(wordings.is_none())
                .map(|b| ui::gated(b, &self.shared.host))
                .on_click(cx.listener(move |v, _, w, c| {
                    let wordings = wordings.clone().unwrap_or_default();
                    v.request(Command::AnswerSkillQuestion { skill, wordings }, w, c)
                }))
        };
        let (take, keep) = (format!("Take {stock}'s wording"), "Keep my wording");
        let asked = choices.iter().any(|choice| choice.question.is_some());
        let passage = |index: usize, choice: &Choice<'_>| {
            let wording = |label: String, text: &str| {
                (div().flex_1().min_w_0().flex().flex_col().gap_1())
                    .child(hint(label, p).text_size(px(font::CAPTION)))
                    .child(Self::read_only_text(text.trim_end().to_owned(), p))
            };
            (div().flex().flex_col().gap_1())
                .when(choices.len() > 1, |d| {
                    d.child(hint(format!("Passage {}", index + 1), p))
                })
                .children(
                    (choice.question).map(|question| hint(format!("The question: {question}"), p)),
                )
                .child(
                    (div().flex().gap_2())
                        .child(wording(format!("{stock}'s wording"), choice.stock))
                        .child(wording("Your wording".into(), choice.yours)),
                )
        };
        if let [choice] = choices {
            let keep = answer("keep-mine", keep.into(), Some(vec![Wording::Yours]));
            let keep = if asked { keep.primary() } else { keep.ghost() };
            let take = answer("take-stock", take, Some(vec![Wording::Stock])).ghost();
            // The page leads with the wording to keep, as the Inbox's card ends with it.
            let choices = (div().flex().flex_wrap().items_center().gap_2()).children(extra);
            let choices = match place == "page" && asked {
                true => choices.child(keep).child(take),
                false => choices.child(take).child(keep),
            };
            return (div().flex().flex_col().gap_2())
                .when(asked, |d| d.child(passage(0, choice)))
                .child(choices);
        }
        let passages = Chosen::passages(choices);
        let chosen = |index: usize| self.skill_wordings[skill as usize].get(&passages, index);
        let rows = choices.iter().enumerate().map(|(index, choice)| {
            let pick = |wording: Wording, label: String| {
                let passages = passages.clone();
                button(id(&format!("wording-{index}-{}", wording as usize)))
                    .ghost()
                    .label(label)
                    .selected(chosen(index) == Some(wording))
                    .on_click(cx.listener(move |v, _, _, c| {
                        v.skill_wordings[skill as usize].choose(&passages, index, wording);
                        c.notify();
                    }))
            };
            passage(index, choice).child(
                (div().flex().gap_2())
                    .child(pick(Wording::Stock, take.clone()))
                    .child(pick(Wording::Yours, keep.into())),
            )
        });
        let wordings = (0..choices.len()).map(chosen).collect();
        (div().flex().flex_col().gap_3()).children(rows).child(
            (div().flex().flex_wrap().items_center().gap_2())
                .child(answer("apply-wordings", "Apply".into(), wordings).primary())
                .children(extra),
        )
    }

    /// A question on the skill's page: what disagrees, and the choices.
    fn question_banner_view(
        &self,
        question: &SkillQuestion,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let model = models::profile_label(&self.shared.catalog, &question.model);
        let [title, text] = question_banner(question, &model);
        let choices = Choice::asked(question);
        let wordings = self.wordings_view(
            question.skill,
            &question.to,
            &choices,
            ("page", vec![]),
            p,
            cx,
        );
        (stacked_banner(p.yellow).child(banner_head("alert", p.yellow, title)))
            .child(hint(text, p))
            .child(wordings)
    }

    /// A question as a workspace note at the top of every project's Inbox.
    pub(super) fn question_card(
        &self,
        question: &SkillQuestion,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let skill = question.skill;
        let (setting, [text, either]) = question_note(question);
        let open = button(SharedString::from(format!("open-{}", skill.folder())))
            .link()
            .label(format!("Open {}", skill.name()))
            .on_click(cx.listener(move |_, _, _, c| c.emit(Intent::OpenSkill(skill))));
        let choices = Choice::asked(question);
        let wordings =
            self.wordings_view(skill, &question.to, &choices, ("inbox", vec![open]), p, cx);
        card(p)
            .bg(p.surface)
            .border_color(p.yellow.opacity(0.55))
            .child(
                (div().flex().items_center().justify_between().gap_2())
                    .child(hint("Workspace · skill update", p))
                    .child(pill("Needs you", p.yellow)),
            )
            .child(
                div()
                    .text_size(px(font::TEXT))
                    .line_height(relative(1.5))
                    .child(StyledText::new(text).with_highlights([(
                        0..setting.len(),
                        HighlightStyle {
                            font_weight: Some(FontWeight::SEMIBOLD),
                            ..Default::default()
                        },
                    )])),
            )
            .child(wordings)
            .child(hint(either, p).text_size(px(font::CAPTION)))
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
        let label = |source, passage| match source {
            Source::Update => format!("From {}", note.to),
            Source::Model => format!("Merged by the model · passage {passage}"),
            Source::Human => "Chosen by you".into(),
        };
        let added = |source| match source {
            Some(Source::Update) | None => p.green,
            Some(_) => p.focus,
        };
        let swatch = |color: Hsla, text: String| {
            let square = div().size(px(8.)).rounded_sm().bg(color.opacity(0.5));
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(square)
                .child(hint(text, p))
        };
        let mut legend = div().flex().flex_wrap().gap_3().child(swatch(
            p.green,
            format!("From {}, as plain text: {}", note.to, note.from_update),
        ));
        if note.by_model > 0 {
            let by = model.as_deref().unwrap_or("the model");
            let passages = counted(note.by_model, "passage");
            let text = format!("Merged by {by}: {passages}{}", merge_tokens(note));
            legend = legend.child(swatch(p.focus, text));
        }
        let legend = legend.child(hint("Everything else is your text, unchanged", p));
        let mut passage = 0;
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
                passage += usize::from(source == Source::Model);
                let tag = hint(label(source, passage), p)
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
                "git-merge",
                "What the merge changed in your rewrite",
                hint(versions, p),
            )
            .child(div().flex_1())
            .child(
                button("review-undo")
                    .ghost()
                    .label("Undo")
                    .map(|b| ui::gated(b, &self.shared.host))
                    .on_click(cx.listener(move |v, _, _, c| v.confirm(undo.clone(), c))),
            )
            .child(
                button("review-done")
                    .primary()
                    .label("Done")
                    .on_click(cx.listener(move |v, _, w, c| {
                        let back = match v.review_from_history {
                            true => SkillView::History(skill),
                            false => SkillView::Skill(skill),
                        };
                        v.open_skill(back, w, c)
                    })),
            ),
        )
        .child(legend)
        .child(diff_block(p).children(rows))
    }

    /// Newest first, each entry with View and Restore, except the newest when it holds what is
    /// in effect now; a merge's entry with Review and Undo this merge instead of View.
    fn history_page(&self, skill: Skill, p: Palette, cx: &mut Context<Self>) -> Div {
        let today = Local::now().date_naive();
        let crumb = self.skills_crumb(skill.name(), p, cx);
        let state = self.skill_state(skill);
        let entries: Vec<_> = (self.skill_history.entries.iter().enumerate())
            .map(|(index, recorded)| {
                let at =
                    written_at(&recorded.name).map_or(recorded.name.clone(), |at| when(at, today));
                let live = (recorded.entry.as_ref().ok())
                    .zip(state)
                    .is_some_and(|(entry, state)| is_live(entry, state));
                let merged_since = merged_since(&self.skill_history.entries, index);
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
            .when(self.skill_history.entries.is_empty(), |d| {
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
                let restore = match self.shared.host.changes_refused() {
                    // Restoring saves at once, so it waits for a host that takes changes.
                    Some(_) => action("restore-entry", "Restore").opacity(0.5),
                    None => action("restore-entry", "Restore").on_click(cx.listener(
                        move |v, _, w, c| {
                            let entry = entry_name.clone();
                            match merged_since.clone() {
                                Some(merged) => v.confirm(Confirm::Restore { entry, merged }, c),
                                None => v.request(Command::RestoreSkill { skill, entry }, w, c),
                            }
                        },
                    )),
                };
                let merge = merge_note(&name, entry, &self.skill_history.machine);
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
            .border_color(p.edge.opacity(edge::SEPARATOR))
            .text_size(px(font::SMALL))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .w(px(104.))
                            .flex_none()
                            .text_size(px(font::CAPTION))
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
            .child(models_view::card_title(title, note, p))
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
                        ui::gated(
                            button("save-skill-text")
                                .primary()
                                .label("Save")
                                .disabled(!dirty),
                            &self.shared.host,
                        )
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
                    "from Models → Reviews ·",
                    lines.collect(),
                )
            }
            Skill::Coordinating => {
                let guide = (self.shared.snapshot()).map_or("", |s| s.model_selection.guide.trim());
                let guide = if guide.is_empty() {
                    "No notes yet."
                } else {
                    guide
                };
                (
                    "Your notes on models for the coordinator",
                    "from Models → Notes ·",
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
                    models_view::card_title(title, from, p).child(
                        div()
                            .id("edit-in-models")
                            .text_size(px(font::SMALL))
                            .text_color(p.focus)
                            .cursor_pointer()
                            .on_click(
                                cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Models))),
                            )
                            .child("Edit in Models"),
                    ),
                )
                .child(Self::read_only_text(lines.join("\n"), p)),
        )
    }
}

impl Requests for SkillsPage {
    fn bridge(&self) -> &Bridge {
        &self.bridge
    }
    fn accept(
        &mut self,
        command: Command,
        reply: Result<Value, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = match reply {
            Ok(value) => value,
            Err(error) => return toast_error(error, window, cx),
        };
        match command {
            Command::SaveSkillRules { .. }
            | Command::SaveSkillRewrite { .. }
            | Command::MarkSkillUpToDate { .. } => {
                match serde_json::from_value(value) {
                    Ok(saved) => self.skill_saved(saved, cx),
                    Err(error) => eprintln!("Saved skill: {error}"),
                }
                toast(skills_view::saved_toast(&command), window, cx);
            }
            Command::ResetSkill { .. }
            | Command::RestoreSkill { .. }
            | Command::StartSkillRewrite { .. }
            | Command::MergeSkillUpdate { .. }
            | Command::UndoSkillMerge { .. } => match serde_json::from_value(value) {
                Ok(state) => self.skill_replaced(state, &command, window, cx),
                Err(error) => eprintln!("Reset, restored, rewrote or merged skill: {error}"),
            },
            Command::DismissSkillMerge { .. } => match serde_json::from_value(value) {
                Ok(state) => self.skill_saved(state, cx),
                Err(error) => eprintln!("Dismissed skill merge: {error}"),
            },
            _ => {}
        }
    }
}
impl Render for SkillsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_counts::count("Skills");
        let p = palette(is_dark(cx));
        ui::page(
            "Skills",
            "How your agents work",
            self.skills_page(p, cx),
            None,
            &self.shared.host,
            p,
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's own `test` attribute would shadow the standard one.
    use super::{
        Choice, Chosen, Confirm, OUT_OF_DATE, Offers, Shown, based_on, core_tab, entry_detail,
        entry_time, entry_title, grouped, is_change, is_live, list_who, merge_counts,
        merge_note_text, merged_since, out_of_date, out_of_date_title, question, question_banner,
        question_note, redraws, reset_question, rewrite_banner, rewrite_question, saved_text,
        shown_changes, state_label, time_or_date, written_at,
    };
    use workspace_core::{
        Command, MergeModel, MergeNote, MergePassage, MergeStanding, ModelProfile, Provider,
        QuestionedPassage, Role, Skill, SkillQuestion, SkillState, Wording,
        skills::{Change, Edits, Entry, History, Line, Recorded},
    };
    use workspace_host::{Host, skills::stock};

    fn state(host: &mut Host, command: Command) -> SkillState {
        serde_json::from_value(host.execute(command).unwrap()).unwrap()
    }

    fn listing(host: &mut Host, skill: Skill) -> Vec<Recorded> {
        let reply = host.execute(Command::SkillHistory { skill }).unwrap();
        serde_json::from_value::<History>(reply).unwrap().entries
    }

    fn titles(history: &[Recorded]) -> Vec<String> {
        let title = |r: &Recorded| entry_title(r.entry.as_ref().unwrap());
        history.iter().map(title).collect()
    }

    #[test]
    fn the_page_draws_again_for_a_new_catalog_or_workspace_only() {
        use crate::shared::Shared;
        use std::rc::Rc;
        let old = Shared::default();
        assert!(!redraws(&old, &old.clone()), "nothing moved");
        let catalog = Shared {
            catalog: Rc::new(vec![]),
            ..old.clone()
        };
        assert!(
            redraws(&old, &catalog),
            "its labels name the catalog's models"
        );
        let workspace = Shared {
            index: Some(Rc::new(workspace_core::WorkspaceIndex::new(
                Default::default(),
                Default::default(),
            ))),
            ..old.clone()
        };
        assert!(redraws(&old, &workspace));
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
    fn an_entrys_name_is_the_time_it_was_written() {
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
        let parts = workspace_host::provider::core_instructions();
        let tabs: Vec<_> = parts.iter().map(core_tab).collect();
        assert_eq!(
            tabs,
            [
                "Contract v9",
                "Coordinator v3",
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
            "Written against an unknown version of Coordinating. This release has v2."
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
            "Written against Coordinating v0. This release has v2."
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
        let title = "Written against Coordinating v0. This release has v2.";
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
            "The model's text for passage 1 was longer than v2's version and yours together.";
        let standings = [
            (
                MergeStanding::WaitingForSync,
                [
                    title,
                    "Waiting for this launch's first sync, so your Macs merge v2 only once. If another Mac already merged it, this Mac takes that.",
                ],
                Offers::Nothing,
            ),
            (
                MergeStanding::Merging { passages: 1 },
                [
                    "Merging v2 into your rewrite",
                    "The PR keeper's model is merging the 1 passage you and v2 both changed. Your rewrite runs as you wrote it until the merge applies.",
                ],
                Offers::Nothing,
            ),
            (
                MergeStanding::Failed {
                    reason: failed.into(),
                },
                [
                    "The merge of v2 couldn't finish",
                    &format!(
                        "{failed} Nothing was changed, and your rewrite runs as you wrote it. Wiffletree tries again at the next start."
                    ),
                ],
                Offers::TryAgain,
            ),
            (
                MergeStanding::Failed {
                    reason: "Claude could not be started".into(),
                },
                [
                    "The merge of v2 couldn't finish",
                    "Claude could not be started. Nothing was changed, and your rewrite runs as you wrote it. Wiffletree tries again at the next start.",
                ],
                Offers::TryAgain,
            ),
            (
                MergeStanding::NeedsModel {
                    passages: vec![passage.clone()],
                    codex: false,
                },
                [
                    "v2 and your rewrite both change 1 passage",
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
                    "v2 and your rewrite both change 2 passages",
                    "Merging them needs a Claude model for the PR keeper, and its row has only Codex models. Your rewrite runs as you wrote it. Choose a model, or choose a wording for each passage.",
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

    /// Coordinating rewritten against a stand-in v0 and merged into v2 at start.
    fn merged_at_start(home: &tempfile::TempDir) -> Host {
        let host = Host::open(home.path()).unwrap();
        let skill = Skill::Coordinating;
        host.skills().start_rewrite(skill).unwrap();
        let base = stock(skill).replacen("— v2", "— v0", 1);
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

    /// Reviewing's question about v1, the model having merged one other passage beside v1's
    /// own changes.
    fn reviewing_question(passages: usize) -> SkillQuestion {
        let passage = QuestionedPassage {
            question: "v1 asks testers to run the full suite once per ticket, and your rewrite says never to run it.".into(),
            stock: "Tester: run the full suite once per ticket.\n".into(),
            yours: "Tester: never run the full suite.\n".into(),
        };
        SkillQuestion {
            skill: Skill::Reviewing,
            to: "v1".into(),
            other_changes: 2,
            model: ModelProfile {
                provider: Provider::Claude,
                model: "haiku".into(),
                effort: "medium".into(),
            },
            passages: vec![passage; passages],
        }
    }

    #[test]
    fn updated_and_needs_you_mark_the_list_and_the_dot_is_for_the_question() {
        let home = tempfile::tempdir().unwrap();
        let host = merged_at_start(&home);
        let skill = Skill::Reviewing;
        host.skills().start_rewrite(skill).unwrap();
        let base = stock(skill).replacen("— v1", "— v0", 1);
        std::fs::write(home.path().join("shared/skills/reviewing/base.md"), base).unwrap();
        let mut skills = host.snapshot().unwrap().skills;
        skills[2].question = Some(reviewing_question(1));
        let listed: Vec<_> = skills.iter().map(state_label).collect();
        assert_eq!(
            listed,
            [
                "Rewritten · updated",
                "Stock",
                "Rewritten · needs you",
                "Stock"
            ]
        );
        // The sidebar's Skills row shows its dot for any rewrite out of date, as one with a
        // question is, and for no merge note.
        let dotted: Vec<_> = skills.iter().map(out_of_date).collect();
        assert_eq!(dotted, [false, false, true, false]);
        assert_eq!(
            list_who(&skills[2]),
            "Testers and reviewers · v1 and your rewrite disagree about one passage"
        );
    }

    #[test]
    fn choices_made_for_one_question_are_not_kept_for_another() {
        let choice = |stock| Choice {
            question: None,
            stock,
            yours: "Mine.",
        };
        let asked = Chosen::passages(&[choice("A."), choice("B.")]);
        let mut chosen = Chosen::default();
        chosen.choose(&asked, 0, Wording::Stock);
        chosen.choose(&asked, 1, Wording::Yours);
        assert_eq!(
            [0, 1].map(|index| chosen.get(&asked, index)),
            [Some(Wording::Stock), Some(Wording::Yours)]
        );
        let replaced = Chosen::passages(&[choice("A."), choice("C.")]);
        assert_eq!(
            [0, 1].map(|index| chosen.get(&replaced, index)),
            [None, None]
        );
        chosen.choose(&replaced, 1, Wording::Stock);
        assert_eq!(
            [0, 1].map(|index| chosen.get(&replaced, index)),
            [None, Some(Wording::Stock)]
        );
    }

    #[test]
    fn a_question_names_both_sides_on_the_page_and_in_the_inbox() {
        let one = reviewing_question(1);
        assert_eq!(
            question_banner(&one, "Claude · haiku · medium"),
            [
                "v1 and your rewrite disagree about one passage",
                "v1's other 2 changes merge cleanly. Claude · haiku · medium didn't merge this passage, because the two want different things. Nothing changes until you choose.",
            ]
        );
        assert_eq!(
            question_note(&one),
            (
                "Reviewing · v1".into(),
                [
                    "Reviewing · v1 and your rewrite disagree about one passage. Until you choose, testers and reviewers get your rewrite as you wrote it.".into(),
                    "v1's other 2 changes merge either way. Your choice applies on every Mac, and you can undo it like any merge.".into(),
                ]
            )
        );
        let mut two = reviewing_question(2);
        two.other_changes = 0;
        assert_eq!(
            question_banner(&two, "Claude · haiku · medium")[1],
            "Claude · haiku · medium didn't merge these passages, because the two want different things. Nothing changes until you choose."
        );
        assert_eq!(
            question_note(&two).1[0],
            "Reviewing · v1 and your rewrite disagree about 2 passages. Until you choose, testers and reviewers get your rewrite as you wrote it."
        );
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
                "Merged Coordinating v2 into your rewrite",
                "1 change from v2 came in as plain text. Coordinators get the merged text from their next turn.",
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
                "Undo the merge of v2?".into(),
                format!(
                    "Your rewrite goes back to exactly what it was before {before}, based on v0. Coordinators get it from their next turn, on every Mac."
                ),
                "v2 won't be merged in again on its own. The merge stays in the history.".into(),
            ]
        );
        let undone = state(&mut host, command);
        assert_eq!(state_label(&undone), "Rewritten · out of date");
        let (banner, offers) = rewrite_banner(&undone).unwrap();
        assert_eq!(
            banner,
            [
                "You undid the merge of v2",
                "Your rewrite is exactly as it was before the merge, and v2 won't be merged into it again on its own.",
            ]
        );
        assert_eq!(offers, Offers::MergeIn);
        let history = listing(&mut host, skill);
        assert_eq!(
            titles(&history)[..2],
            ["Undid the merge of v2", "Merged v0 → v2"]
        );
        // After a later change, the merge's entry still offers Undo, which says so.
        let later = Confirm::Undo { note, later: true };
        let ([_, what, kept], _) = question(&undone, &later, false);
        assert!(what.ends_with(
            "based on v0, and your changes since stop applying. Coordinators get it from their next turn, on every Mac."
        ));
        assert_eq!(
            kept,
            "v2 won't be merged in again on its own. The merge and your changes since stay in the history."
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
                "Merged v0 → v2",
                "Saved your rewrite",
                "Saved your rewrite",
                "Started a rewrite"
            ]
        );
        assert_eq!(merged_since(&history, 0), None);
        assert_eq!(merged_since(&history, 1).as_deref(), Some("v2"));
        let entry = history[1].name.clone();
        let restore = Confirm::Restore {
            entry: entry.clone(),
            merged: "v2".into(),
        };
        let coordinating = host.snapshot().unwrap().skills.remove(0);
        let (asked, command) = question(&coordinating, &restore, false);
        assert_eq!(
            asked,
            [
                format!("Restore the version of {}?", entry_time(&entry)),
                "It is from before the merge of v2, so no automatic merge brings v2 in until a newer version arrives. You can still merge it in from Coordinating's page.".into(),
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
        let base = stock(skill).replacen("— v2", "— v3", 1);
        std::fs::write(home.path().join("shared/skills/coordinating/base.md"), base).unwrap();
        let mut coordinating = host.snapshot().unwrap().skills.remove(0);
        assert_eq!(state_label(&coordinating), "Rewritten");
        let here = "This Mac has v2 and sends the rewrite as written. It isn't out of date here, and nothing is merged here.";
        assert_eq!(
            rewrite_banner(&coordinating),
            Some((
                [
                    "Based on Coordinating v3, from a Mac with a newer Wiffletree".into(),
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
                    "Based on Coordinating v3, from Mike's MacBook Air".into(),
                    format!(
                        "That Mac runs a newer Wiffletree and merged v3 into your rewrite. {here}"
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
            "3 changes from v2 came in as plain text. You had also changed 1 passage that v2 changes, so Claude · Haiku · Medium merged it, keeping your wording where both could stand. Coordinators get the merged text from their next turn."
        );
    }

    #[test]
    fn the_list_dates_the_merge_it_notes() {
        let home = tempfile::tempdir().unwrap();
        let host = merged_at_start(&home);
        let mut coordinating = host.snapshot().unwrap().skills.remove(0);
        assert!(
            list_who(&coordinating)
                .starts_with("Project coordinator · v2 merged into your rewrite today at ")
        );
        let note = coordinating.merge_note.as_mut().unwrap();
        note.entry = "2020-10-14T12-00-00.000Z".into();
        assert!(list_who(&coordinating).contains(" merged into your rewrite on 14 Oct at "));
        coordinating.merge_note = None;
        assert_eq!(list_who(&coordinating), "Project coordinator");
    }
}
