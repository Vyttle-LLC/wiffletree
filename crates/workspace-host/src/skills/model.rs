//! The model's part of a merge: what it is told and sent, and the check of its answer before
//! the host puts any of it into the plain merge.
use anyhow::{Context as _, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
pub use workspace_core::skills::PassageAnswer;
use workspace_core::{MergePassage, Skill};

/// The merge run's instructions, which hold none of the fixed core.
pub(crate) const MERGING: &str = include_str!("../../skills/merging.md");
pub(crate) const ANSWER_SCHEMA: &str = r#"{"type":"object","properties":{"passages":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"merged":{"type":"string"},"question":{"type":"string"}},"required":["id"]}}},"required":["passages"]}"#;
const MAX_QUESTION: usize = 300;

/// What the model is sent: the skill's name, its three texts and each overlapping passage,
/// numbered from 1.
pub(crate) fn input(
    skill: Skill,
    base: &str,
    stock: &str,
    yours: &str,
    passages: &[MergePassage],
) -> Vec<u8> {
    let passages: Vec<_> = (passages.iter().zip(1..))
        .map(|(passage, id)| {
            let MergePassage { base, stock, yours } = passage;
            json!({"id": id.to_string(), "base": base, "stock": stock, "yours": yours})
        })
        .collect();
    let input = json!({"skill": skill.name(), "base": base, "stock": stock, "yours": yours, "passages": passages});
    input.to_string().into_bytes()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    passages: Vec<Answered>,
}
/// One passage's answer: exactly a text or a question, as the schema says.
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Answered {
    Merged { id: String, merged: String },
    Question { id: String, question: String },
}

/// Each passage's answer, in order, once the answer is checked. Nothing of an answer is used
/// when it doesn't answer every passage exactly once with a text or a question, when a text is
/// longer than the passage's stock and human versions together, or when a question is longer
/// than 300 characters.
pub(crate) fn answers(
    stock_version: &str,
    passages: &[MergePassage],
    answer: &Value,
) -> Result<Vec<PassageAnswer>> {
    const ONCE: &str = "The model's answer didn't answer each passage exactly once.";
    let Answer { passages: answered } = (serde_json::from_value(answer.clone()).ok())
        .context("The model's answer didn't have the shape it was asked for.")?;
    let mut by_passage: Vec<Option<Answered>> = passages.iter().map(|_| None).collect();
    for answered in answered {
        let (Answered::Merged { id, .. } | Answered::Question { id, .. }) = &answered;
        let index = (id.parse::<usize>().ok()).and_then(|id| id.checked_sub(1));
        match index.and_then(|index| by_passage.get_mut(index)) {
            Some(slot @ None) => *slot = Some(answered),
            _ => bail!(ONCE),
        }
    }
    let mut answers = vec![];
    for (passage, (answered, number)) in passages.iter().zip(by_passage.into_iter().zip(1..)) {
        answers.push(match answered.context(ONCE)? {
            Answered::Merged { merged: text, .. } => {
                let longest = passage.stock.chars().count() + passage.yours.chars().count();
                ensure!(
                    text.chars().count() <= longest,
                    "The model's text for passage {number} was longer than {stock_version}'s version and yours together."
                );
                PassageAnswer::Merged(text)
            }
            Answered::Question { question, .. } => {
                ensure!(
                    question.chars().count() <= MAX_QUESTION,
                    "The model's question about passage {number} was longer than {MAX_QUESTION} characters."
                );
                PassageAnswer::Question(question)
            }
        });
    }
    Ok(answers)
}
