//! The router (stage 2): before a message the user sends to the assistant, a small model answers
//! two fixed questions, which thread and which Space, each with a confidence. Fixed rules act on
//! the answer, or ask the user with one-click choices when it is unsure.
use super::MAX_TITLE_CHARS;
use crate::SpaceId;
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

/// The confidence at or above which the host acts on an answer, unless the user changed it.
pub const DEFAULT_THRESHOLD: f64 = 0.8;
pub const THRESHOLDS: RangeInclusive<f64> = 0.5..=1.0;
/// The current thread's messages the router reads: at most six, of at most 1,500 characters.
pub const MAX_LAST_MESSAGES: usize = 6;
pub const MAX_LAST_CHARS: usize = 1500;
pub const MAX_ALTERNATIVES: usize = 2;

pub fn validate_threshold(threshold: f64) -> anyhow::Result<f64> {
    anyhow::ensure!(
        THRESHOLDS.contains(&threshold),
        "The router asks below a confidence from 0.5 to 1.0"
    );
    Ok(threshold)
}

/// Everything the router sees: never brains, outputs, scratch files, other threads' text or
/// Space instructions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouterInput {
    pub message: String,
    /// `None` for the first message and one sent from an empty tab: continuing is no choice.
    pub current_thread: Option<CurrentThread>,
    /// The other threads in the tab strip.
    pub open_threads: Vec<OpenThread>,
    pub spaces: Vec<RouterSpace>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentThread {
    pub id: String,
    pub title: String,
    pub space_id: SpaceId,
    /// Its latest messages, oldest first, each as "User: …" or "Assistant: …".
    pub last_messages: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenThread {
    pub id: String,
    pub title: String,
    pub space_id: SpaceId,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouterSpace {
    pub id: SpaceId,
    pub name: String,
    pub description: String,
}

/// A thread's latest messages as the router reads them, from `messages` given newest first as
/// whether the user wrote it and its text: at most six, cut to 1,500 characters in all, keeping
/// the newest.
pub fn last_messages<'a>(messages: impl IntoIterator<Item = (bool, &'a str)>) -> Vec<String> {
    let mut left = MAX_LAST_CHARS;
    let mut lines = vec![];
    for (from_user, text) in messages.into_iter().take(MAX_LAST_MESSAGES) {
        if left == 0 {
            break;
        }
        let who = if from_user { "User" } else { "Assistant" };
        let line: String = format!("{who}: {}", text.trim())
            .chars()
            .take(left)
            .collect();
        left -= line.chars().count();
        lines.push(line);
    }
    lines.reverse();
    lines
}

/// The thread question's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThreadPick {
    Continue,
    Switch { thread_id: String },
    New,
}
/// A pick with its confidence, from 0 to 1, and at most two alternatives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice<T> {
    pub pick: T,
    pub confidence: f64,
    pub alternatives: Vec<Alternative<T>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alternative<T> {
    pub pick: T,
    pub confidence: f64,
}
impl<T> Choice<T> {
    fn sure(pick: T) -> Self {
        Self {
            pick,
            confidence: 1.0,
            alternatives: vec![],
        }
    }
    /// The pick, then its alternatives, each with its confidence.
    fn candidates(&self) -> impl Iterator<Item = (&T, f64)> {
        std::iter::once((&self.pick, self.confidence))
            .chain(self.alternatives.iter().map(|a| (&a.pick, a.confidence)))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouterAnswer {
    pub thread: Choice<ThreadPick>,
    pub space: Choice<SpaceId>,
    /// A new thread's title, at most 60 characters; empty otherwise.
    pub title: String,
}

/// The answer's shape, as the router's run is told to return it. Every field is required and
/// no other is allowed, as Codex's output schemas need; `thread_id` is empty unless `kind` is
/// `switch`.
pub const ANSWER_SCHEMA: &str = r##"{"type":"object","additionalProperties":false,"required":["thread","space","title"],"properties":{"thread":{"type":"object","additionalProperties":false,"required":["pick","confidence","alternatives"],"properties":{"pick":{"$ref":"#/$defs/thread_pick"},"confidence":{"type":"number"},"alternatives":{"type":"array","maxItems":2,"items":{"type":"object","additionalProperties":false,"required":["pick","confidence"],"properties":{"pick":{"$ref":"#/$defs/thread_pick"},"confidence":{"type":"number"}}}}}},"space":{"type":"object","additionalProperties":false,"required":["pick","confidence","alternatives"],"properties":{"pick":{"type":"string"},"confidence":{"type":"number"},"alternatives":{"type":"array","maxItems":2,"items":{"type":"object","additionalProperties":false,"required":["pick","confidence"],"properties":{"pick":{"type":"string"},"confidence":{"type":"number"}}}}}},"title":{"type":"string"}},"$defs":{"thread_pick":{"type":"object","additionalProperties":false,"required":["kind","thread_id"],"properties":{"kind":{"type":"string","enum":["continue","switch","new"]},"thread_id":{"type":"string"}}}}}"##;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire<T> {
    pick: T,
    confidence: f64,
    alternatives: Vec<WireAlternative<T>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAlternative<T> {
    pick: T,
    confidence: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WirePick {
    kind: String,
    #[serde(default)]
    thread_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAnswer {
    thread: Wire<WirePick>,
    space: Wire<SpaceId>,
    title: String,
}
impl<T> Wire<T> {
    fn into_choice<U>(self, convert: impl Fn(T) -> Result<U, String>) -> Result<Choice<U>, String> {
        let valid = |c: f64| (0.0..=1.0).contains(&c);
        let confidences =
            std::iter::once(self.confidence).chain(self.alternatives.iter().map(|a| a.confidence));
        if !confidences.into_iter().all(valid) {
            return Err("a confidence is outside 0 to 1".into());
        }
        if self.alternatives.len() > MAX_ALTERNATIVES {
            return Err("it gave more than two alternatives".into());
        }
        let alternatives = (self.alternatives.into_iter())
            .map(|a| {
                Ok(Alternative {
                    pick: convert(a.pick)?,
                    confidence: a.confidence,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Choice {
            pick: convert(self.pick)?,
            confidence: self.confidence,
            alternatives,
        })
    }
}
impl RouterAnswer {
    /// The answer a run returned, or what is wrong with its shape.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        let wire: WireAnswer = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        let thread = wire.thread.into_choice(|pick| match pick.kind.as_str() {
            "continue" => Ok(ThreadPick::Continue),
            "new" => Ok(ThreadPick::New),
            "switch" if !pick.thread_id.is_empty() => Ok(ThreadPick::Switch {
                thread_id: pick.thread_id,
            }),
            other => Err(format!("it picked the thread kind \"{other}\"")),
        })?;
        let space = wire.space.into_choice(Ok)?;
        let title = wire.title.trim().to_owned();
        if title.chars().count() > MAX_TITLE_CHARS || title.chars().any(char::is_control) {
            return Err("its title is not one line of at most 60 characters".into());
        }
        Ok(Self {
            thread,
            space,
            title,
        })
    }
}

/// Where a message goes: a thread, or a new thread in a Space.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    Thread { thread_id: String },
    NewThread { space_id: SpaceId },
}
/// What the user is asked when the answer is unsure, and what Change offers once it was not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// Why the router was unsure, such as "Not sure where this belongs (Sage 62%, Personal
    /// 31%)."; empty once a message was sent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// One button each, before "Somewhere else…".
    pub choices: Vec<Destination>,
}
/// What the rules made of an answer: where to send the message, or `None` to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct Routed {
    pub send: Option<Destination>,
    pub question: Question,
}

impl RouterInput {
    /// The destination when there is nothing to decide: one Space, and no thread to continue
    /// or switch to.
    pub fn forced(&self) -> Option<Destination> {
        match self.spaces.as_slice() {
            [only] if self.current_thread.is_none() && self.open_threads.is_empty() => {
                Some(Destination::NewThread {
                    space_id: only.id.clone(),
                })
            }
            _ => None,
        }
    }
    fn thread_space(&self, id: &str) -> Option<&SpaceId> {
        let current = (self.current_thread.iter())
            .find(|t| t.id == id)
            .map(|t| &t.space_id);
        current.or_else(|| {
            (self.open_threads.iter())
                .find(|t| t.id == id)
                .map(|t| &t.space_id)
        })
    }
    fn thread_title(&self, id: &str) -> &str {
        let current = (self.current_thread.iter()).find(|t| t.id == id);
        let title = current.map(|t| t.title.as_str()).or_else(|| {
            (self.open_threads.iter())
                .find(|t| t.id == id)
                .map(|t| t.title.as_str())
        });
        title.unwrap_or_default()
    }
    fn space_name(&self, id: &SpaceId) -> &str {
        (self.spaces.iter())
            .find(|s| &s.id == id)
            .map_or("", |s| s.name.as_str())
    }
    fn knows_space(&self, id: &SpaceId) -> bool {
        self.spaces.iter().any(|s| &s.id == id)
    }
    pub fn destination_space<'a>(&'a self, destination: &'a Destination) -> Option<&'a SpaceId> {
        match destination {
            Destination::Thread { thread_id } => self.thread_space(thread_id),
            Destination::NewThread { space_id } => Some(space_id),
        }
    }
    /// Whether every thread and Space the answer names is one the router was shown.
    fn knows(&self, answer: &RouterAnswer) -> bool {
        let threads = answer.thread.candidates().all(|(pick, _)| match pick {
            ThreadPick::Switch { thread_id } => self.thread_space(thread_id).is_some(),
            ThreadPick::Continue | ThreadPick::New => true,
        });
        threads && answer.space.candidates().all(|(s, _)| self.knows_space(s))
    }

    /// The one-click choices: each thread pick and alternative, a new thread taking each Space
    /// candidate, a new thread in each Space candidate no choice is in yet, and continuing the
    /// current thread. Without an answer, a new thread in each Space and continuing.
    fn choices(&self, answer: Option<&RouterAnswer>) -> Vec<Destination> {
        let mut choices: Vec<Destination> = vec![];
        let add = |choices: &mut Vec<Destination>, choice: Destination| {
            if !choices.contains(&choice) {
                choices.push(choice);
            }
        };
        let current = self.current_thread.as_ref().map(|t| Destination::Thread {
            thread_id: t.id.clone(),
        });
        let spaces: Vec<&SpaceId> = match answer {
            Some(answer) => (answer.space.candidates())
                .map(|(s, _)| s)
                .filter(|s| self.knows_space(s))
                .collect(),
            None => self.spaces.iter().map(|s| &s.id).collect(),
        };
        let new_in = |space: &SpaceId| Destination::NewThread {
            space_id: space.clone(),
        };
        for (pick, _) in answer.iter().flat_map(|a| a.thread.candidates()) {
            match pick {
                ThreadPick::Continue => current.iter().for_each(|c| add(&mut choices, c.clone())),
                ThreadPick::Switch { thread_id } if self.thread_space(thread_id).is_some() => add(
                    &mut choices,
                    Destination::Thread {
                        thread_id: thread_id.clone(),
                    },
                ),
                ThreadPick::Switch { .. } => {}
                ThreadPick::New => spaces.iter().for_each(|s| add(&mut choices, new_in(s))),
            }
        }
        for space in spaces {
            let offered = (choices.iter()).any(|c| self.destination_space(c) == Some(space));
            if !offered {
                add(&mut choices, new_in(space));
            }
        }
        if let Some(current) = current {
            add(&mut choices, current);
        }
        choices
    }

    /// The host's rules, in order: no answer, or one naming a thread or Space the router was
    /// not shown, or continuing with no current thread, is unsure; with one Space the host
    /// answers the Space question; an answer below `threshold` on either question, or a thread
    /// in another Space than the Space answer, is unsure. `answer` is the run's answer, or a
    /// sentence on why there is none.
    pub fn route(&self, answer: Result<RouterAnswer, String>, threshold: f64) -> Routed {
        let mut answer = match answer {
            Ok(answer) => answer,
            Err(why) => return self.ask(why, None),
        };
        if let [only] = self.spaces.as_slice() {
            answer.space = Choice::sure(only.id.clone());
        }
        if !self.knows(&answer) {
            let why = "The router named a thread or Space that is not open here.";
            return self.ask(why.into(), Some(&answer));
        }
        let send = match &answer.thread.pick {
            ThreadPick::Continue => match &self.current_thread {
                Some(current) => Destination::Thread {
                    thread_id: current.id.clone(),
                },
                None => {
                    let why = "The router answered continue, but no thread is open here.";
                    return self.ask(why.into(), Some(&answer));
                }
            },
            ThreadPick::Switch { thread_id } => Destination::Thread {
                thread_id: thread_id.clone(),
            },
            ThreadPick::New => Destination::NewThread {
                space_id: answer.space.pick.clone(),
            },
        };
        if answer.space.confidence < threshold {
            let named = (answer.space.candidates())
                .map(|(s, c)| format!("{} {}", self.space_name(s), percent(c)));
            let why = format!("Not sure where this belongs ({}).", list(named));
            return self.ask(why, Some(&answer));
        }
        if answer.thread.confidence < threshold {
            let named = (answer.thread.candidates()).map(|(pick, c)| {
                let thread = match pick {
                    ThreadPick::Continue => self.current_thread.as_ref().map(|t| t.id.as_str()),
                    ThreadPick::Switch { thread_id } => Some(thread_id.as_str()),
                    ThreadPick::New => None,
                };
                match thread {
                    Some(id) => format!("“{}” {}", self.thread_title(id), percent(c)),
                    None => format!("a new thread {}", percent(c)),
                }
            });
            let why = format!("Not sure which thread this belongs to ({}).", list(named));
            return self.ask(why, Some(&answer));
        }
        if self.destination_space(&send) != Some(&answer.space.pick) {
            let Destination::Thread { thread_id } = &send else {
                unreachable!("a new thread takes the Space answer")
            };
            let space = self.thread_space(thread_id).cloned().unwrap_or_default();
            let why = format!(
                "The router picked “{}” in {} but answered {}.",
                self.thread_title(thread_id),
                self.space_name(&space),
                self.space_name(&answer.space.pick)
            );
            return self.ask(why, Some(&answer));
        }
        Routed {
            send: Some(send),
            question: Question {
                why: String::new(),
                choices: self.choices(Some(&answer)),
            },
        }
    }
    fn ask(&self, why: String, answer: Option<&RouterAnswer>) -> Routed {
        Routed {
            send: None,
            question: Question {
                why,
                choices: self.choices(answer),
            },
        }
    }
}
fn percent(confidence: f64) -> String {
    format!("{:.0}%", confidence * 100.0)
}
fn list(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn space(id: &str, name: &str) -> RouterSpace {
        RouterSpace {
            id: SpaceId(id.into()),
            name: name.into(),
            description: format!("{name} work"),
        }
    }
    /// A Sage thread about release notes is current; a Personal thread is open.
    fn input() -> RouterInput {
        RouterInput {
            message: "and the watch app?".into(),
            current_thread: Some(CurrentThread {
                id: "notes".into(),
                title: "Release notes".into(),
                space_id: SpaceId("sage".into()),
                last_messages: vec!["User: draft the release notes".into()],
            }),
            open_threads: vec![OpenThread {
                id: "dentist".into(),
                title: "Dentist Tuesday".into(),
                space_id: SpaceId("personal".into()),
            }],
            spaces: vec![space("sage", "Sage"), space("personal", "Personal")],
        }
    }
    fn answer(thread: ThreadPick, at: f64, space: &str, sure: f64) -> RouterAnswer {
        RouterAnswer {
            thread: Choice {
                pick: thread,
                confidence: at,
                alternatives: vec![],
            },
            space: Choice {
                pick: SpaceId(space.into()),
                confidence: sure,
                alternatives: vec![],
            },
            title: String::new(),
        }
    }
    fn to(thread: &str) -> Destination {
        Destination::Thread {
            thread_id: thread.into(),
        }
    }
    fn new_in(space: &str) -> Destination {
        Destination::NewThread {
            space_id: SpaceId(space.into()),
        }
    }

    #[test]
    fn a_clear_answer_continues_switches_or_opens_a_new_thread() {
        let input = input();
        let go = |a| input.route(Ok(a), DEFAULT_THRESHOLD).send;
        assert_eq!(
            go(answer(ThreadPick::Continue, 0.95, "sage", 0.9)),
            Some(to("notes"))
        );
        let dentist = ThreadPick::Switch {
            thread_id: "dentist".into(),
        };
        assert_eq!(
            go(answer(dentist, 0.9, "personal", 0.9)),
            Some(to("dentist"))
        );
        assert_eq!(
            go(answer(ThreadPick::New, 0.93, "personal", 0.9)),
            Some(new_in("personal"))
        );
    }

    #[test]
    fn below_the_threshold_asks_with_the_picks_alternatives_and_continuing() {
        let mut unsure = answer(ThreadPick::New, 0.9, "sage", 0.62);
        unsure.space.alternatives.push(Alternative {
            pick: SpaceId("personal".into()),
            confidence: 0.31,
        });
        let routed = input().route(Ok(unsure), DEFAULT_THRESHOLD);
        assert_eq!(routed.send, None);
        assert_eq!(
            routed.question.why,
            "Not sure where this belongs (Sage 62%, Personal 31%)."
        );
        assert_eq!(
            routed.question.choices,
            [new_in("sage"), new_in("personal"), to("notes")]
        );
        let low_thread = answer(ThreadPick::Continue, 0.55, "sage", 0.9);
        let routed = input().route(Ok(low_thread), DEFAULT_THRESHOLD);
        assert_eq!(
            routed.question.why,
            "Not sure which thread this belongs to (“Release notes” 55%)."
        );
        // A lower threshold acts on the same answer.
        let low_thread = answer(ThreadPick::Continue, 0.55, "sage", 0.9);
        assert_eq!(input().route(Ok(low_thread), 0.5).send, Some(to("notes")));
    }

    #[test]
    fn a_thread_in_another_space_than_the_space_answer_is_unsure() {
        let dentist = ThreadPick::Switch {
            thread_id: "dentist".into(),
        };
        let routed = input().route(Ok(answer(dentist, 0.9, "sage", 0.9)), DEFAULT_THRESHOLD);
        assert_eq!(routed.send, None);
        assert_eq!(
            routed.question.why,
            "The router picked “Dentist Tuesday” in Personal but answered Sage."
        );
        assert_eq!(
            routed.question.choices,
            [to("dentist"), new_in("sage"), to("notes")]
        );
    }

    #[test]
    fn unknown_ids_continuing_nothing_and_no_answer_are_unsure() {
        let unknown = ThreadPick::Switch {
            thread_id: "elsewhere".into(),
        };
        let routed = input().route(Ok(answer(unknown, 0.99, "sage", 0.99)), 0.8);
        assert_eq!(routed.send, None);
        assert!(routed.question.why.contains("not open here"));
        let routed = input().route(Ok(answer(ThreadPick::New, 0.99, "vyttle", 0.99)), 0.8);
        assert_eq!(routed.send, None);

        let mut first = input();
        first.current_thread = None;
        let routed = first.route(Ok(answer(ThreadPick::Continue, 0.99, "sage", 0.99)), 0.8);
        assert_eq!(routed.send, None);
        assert!(routed.question.why.contains("no thread is open"));

        let failed = input().route(Err("The router took longer than 10 seconds.".into()), 0.8);
        assert_eq!(failed.send, None);
        assert_eq!(
            failed.question.why,
            "The router took longer than 10 seconds."
        );
        assert_eq!(
            failed.question.choices,
            [new_in("sage"), new_in("personal"), to("notes")]
        );
    }

    #[test]
    fn with_one_space_the_host_answers_the_space_question() {
        let mut one = input();
        one.spaces.truncate(1);
        one.open_threads.clear();
        let routed = one.route(Ok(answer(ThreadPick::New, 0.9, "unheard-of", 0.1)), 0.8);
        assert_eq!(routed.send, Some(new_in("sage")));
        one.current_thread = None;
        assert_eq!(one.forced(), Some(new_in("sage")));
        assert_eq!(input().forced(), None);
    }

    #[test]
    fn an_answer_out_of_shape_is_refused() {
        let good = json!({
            "thread": {"pick": {"kind": "switch", "thread_id": "dentist"}, "confidence": 0.9,
                       "alternatives": [{"pick": {"kind": "new", "thread_id": ""}, "confidence": 0.1}]},
            "space": {"pick": "personal", "confidence": 0.9, "alternatives": []},
            "title": ""
        });
        let parsed = RouterAnswer::from_value(&good).unwrap();
        assert_eq!(
            parsed.thread.pick,
            ThreadPick::Switch {
                thread_id: "dentist".into()
            }
        );
        let broken = |path: &str, value: serde_json::Value| {
            let mut bad = good.clone();
            *bad.pointer_mut(path).unwrap() = value;
            RouterAnswer::from_value(&bad)
        };
        assert!(broken("/thread/confidence", json!(1.2)).is_err());
        assert!(broken("/thread/pick/kind", json!("maybe")).is_err());
        assert!(broken("/thread/pick/thread_id", json!("")).is_err());
        assert!(broken("/title", json!("x".repeat(61))).is_err());
        assert!(broken("/space/pick", json!(3)).is_err());
        let three = json!([{"pick":"sage","confidence":0.1},{"pick":"sage","confidence":0.1},{"pick":"sage","confidence":0.1}]);
        assert!(broken("/space/alternatives", three).is_err());
        assert!(RouterAnswer::from_value(&json!({"thread": 1})).is_err());
        serde_json::from_str::<serde_json::Value>(ANSWER_SCHEMA).unwrap();
    }

    #[test]
    fn the_router_reads_six_messages_of_at_most_1500_characters_newest_kept() {
        let long = "x".repeat(1000);
        let newest_first = [
            (true, "and the watch app?"),
            (false, long.as_str()),
            (true, long.as_str()),
            (false, "older"),
        ];
        let lines = last_messages(newest_first);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], "User: and the watch app?");
        assert_eq!(lines.iter().map(|l| l.chars().count()).sum::<usize>(), 1500);
        let many: Vec<(bool, &str)> = (0..10).map(|_| (true, "hi")).collect();
        assert_eq!(last_messages(many).len(), MAX_LAST_MESSAGES);
    }
}
