//! The cards the Assistant's tools put in the chat (OpenSpec change `assistant-does-the-work`,
//! tasks 3.2 and 3.5): work with its stage and pull requests, a decision only the human makes,
//! an approval that settles its Inbox item, a loop change with Undo, an action waiting for Run
//! or done without asking, an issue written, a saved output and a suggested Space. A card
//! raised in a turn that read text written by others says what it read.
use super::*;
use work::{Stage, StepMark, WorkItem};
use workspace_core::assistant::conversation::{
    ActCall, ActionCard, ActionState, CardKind as Kind, Decision, LoopCard, LoopCardState,
    LoopChange,
};
use workspace_core::job::{Cron, Recipe};
use workspace_core::{IssueId, PrState, WorkState};

/// The schedule in words with its timezone, such as "Fridays at 16:00 (America/Chicago)".
pub(crate) fn schedule_words(recipe: &Recipe) -> String {
    let cron = &recipe.schedule.cron;
    match Cron::parse(cron) {
        Ok(parsed) => format!("{} ({})", parsed.words(cron), recipe.schedule.timezone),
        Err(_) => format!("“{cron}” ({})", recipe.schedule.timezone),
    }
}
/// Each source a loop reads, with its read tools or commands.
fn source_lines(recipe: &Recipe) -> Vec<String> {
    (recipe.sources.iter())
        .map(|source| {
            format!(
                "{}: {}, read-only",
                source.name(),
                source.reads().join(", ")
            )
        })
        .collect()
}
/// The exact content of an action, one line per argument, as Run will send it.
pub(crate) fn action_lines(call: &ActCall) -> Vec<(String, String)> {
    match call {
        ActCall::Cli { command } => vec![("Command".into(), command.clone())],
        ActCall::Mcp { tool, arguments } => {
            let mut lines = vec![("Tool".into(), tool.clone())];
            match arguments.as_object() {
                Some(fields) => lines.extend(fields.iter().map(|(key, value)| {
                    let text = match value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    (key.clone(), text)
                })),
                None if !arguments.is_null() => {
                    lines.push(("Arguments".into(), arguments.to_string()))
                }
                None => {}
            }
            lines
        }
    }
}
/// Whether an applied loop change can still be undone: no later change of the same loop was
/// applied since.
pub(crate) fn undoable(cards: &[ConversationCard], card: &ConversationCard) -> bool {
    let Kind::Loop(change) = &card.kind else {
        return false;
    };
    let later = cards
        .iter()
        .skip_while(|c| c.id != card.id)
        .skip(1)
        .any(|c| matches!(&c.kind, Kind::Loop(other) if other.job_id == change.job_id && other.state == LoopCardState::Applied));
    change.state == LoopCardState::Applied && !later
}

/// A key and its value, as a card lists them.
fn line(label: impl Into<SharedString>, value: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .gap_3()
        .text_size(px(font::CAPTION))
        .child(
            div()
                .flex_none()
                .w(px(96.))
                .text_color(p.subtle)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(value))
}
fn title(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::SEMIBOLD).child(text.into())
}
fn actions() -> Div {
    div().flex().gap_2().justify_end()
}
/// What text written by others the card's turn read, when it read any.
fn read_note(card: &ConversationCard, p: Palette) -> Option<Div> {
    (!card.read.is_empty()).then(|| {
        hint(
            format!(
                "This turn read text written by others: {}. It waits for you.",
                card.read.join(", ")
            ),
            p,
        )
        .text_color(p.yellow)
    })
}

impl AssistantPage {
    pub(super) fn chat_card(
        &self,
        card: &ConversationCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let body = match &card.kind {
            Kind::Work { project_id } => self.work_card(&card.id, project_id, p, cx),
            Kind::Decision(decision) => self.decision_card(&card.id, decision, p, cx),
            Kind::Approval {
                project_id,
                attention_id,
            } => self.approval_card(project_id, attention_id, p, cx),
            Kind::Loop(change) => self.loop_card(card, change, p, cx),
            Kind::Action(action) => self.action_card(&card.id, action, p, cx),
            Kind::IssueWritten {
                space_id,
                issue_id,
                written,
            } => self.issue_card(space_id, issue_id, written, p, cx),
            Kind::SavedOutput { title, version, .. } => card_frame(p)
                .flex_row()
                .items_center()
                .gap_2()
                .child(icon("file-text").size(px(14.)))
                .child(div().flex_1().child(saved_line(title, *version)))
                .child(
                    button(SharedString::from(format!("boards-{}", card.id)))
                        .ghost()
                        .xsmall()
                        .label("Open Boards")
                        .on_click(cx.listener(|_, _, _, c| c.emit(Intent::OpenPage(Page::Boards)))),
                ),
            Kind::SuggestSpace { name, description } => {
                let (name, description) = (name.clone(), description.clone());
                card_frame(p)
                    .child(title(format!("A new Space: {name}")))
                    .child(hint(description.clone(), p))
                    .child(hint("Nothing is created until you save it in Spaces.", p))
                    .child(
                        actions().child(
                            button(SharedString::from(format!("suggested-{}", card.id)))
                                .xsmall()
                                .label("Open Spaces")
                                .on_click(cx.listener(move |_, _, _, c| {
                                    c.emit(Intent::SuggestSpace {
                                        name: name.clone(),
                                        description: description.clone(),
                                    })
                                })),
                        ),
                    )
            }
        };
        body.children(read_note(card, p))
    }

    fn work_item<'a>(&'a self, project_id: &str) -> Option<WorkItem<'a>> {
        let index = self.shared.index.as_deref()?;
        work::work_items(index)
            .into_iter()
            .find(|item| item.project.id == project_id)
    }

    /// A work item: its title and Space with Stop and Open details, its stage or how it
    /// ended, and its pull requests.
    fn work_card(
        &self,
        card_id: &str,
        project_id: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(item) = self.work_item(project_id) else {
            return card_frame(p).child(hint("This work is no longer in Wiffletree.", p));
        };
        let spaces = self.spaces().to_vec();
        let state = item.work.state;
        let mut head = div()
            .flex()
            .items_center()
            .gap_2()
            .when(state == WorkState::Done, |d| {
                d.child(div().text_color(p.green).child("✓"))
            })
            .child(title(item.project.name.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(font::META))
                    .text_color(p.subtle)
                    .child(space_mark(space_color(&spaces, &item.project.space, p)))
                    .child(self.space_name(&item.project.space)),
            )
            .child(div().flex_1());
        if item.running() {
            let stopped = project_id.to_owned();
            head = head.child(
                button(SharedString::from(format!("stop-work-{card_id}")))
                    .ghost()
                    .xsmall()
                    .label("Stop")
                    .on_click(cx.listener(move |v, _, w, c| v.stop_work(stopped.clone(), w, c))),
            );
        }
        if let Some(coordinator) = item.coordinator.map(|c| c.id.clone()) {
            head = head.child(
                button(SharedString::from(format!("open-work-{card_id}")))
                    .xsmall()
                    .label("Open details")
                    .on_click(
                        cx.listener(move |_, _, _, c| c.emit(Intent::Select(coordinator.clone()))),
                    ),
            );
        }
        let steps = match state {
            WorkState::Running => steps_row(item.stage, p),
            WorkState::Paused => div().flex().child(ui::pill("Paused", p.yellow)),
            WorkState::Stopped => div().flex().child(ui::pill("Stopped", p.red)),
            WorkState::Done => div().flex().child(ui::pill("Done", p.green)),
        };
        let mut pulls = div().flex().flex_wrap().gap_2();
        for pull in &item.pull_requests {
            let url = pull.url.clone();
            let state = match pull.state {
                PrState::Open => "open",
                PrState::Merged => "merged",
                PrState::Closed => "closed",
            };
            pulls = pulls.child(
                div()
                    .id(SharedString::from(format!("pr-{card_id}-{}", pull.number)))
                    .text_size(px(font::CAPTION))
                    .text_color(p.accent)
                    .cursor_pointer()
                    .child(format!(
                        "{} #{} {state}",
                        repository_of(&pull.url),
                        pull.number
                    ))
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        card_frame(p)
            .max_w(relative(0.88))
            .child(head)
            .child(steps)
            .when(!item.pull_requests.is_empty(), |d| d.child(pulls))
            .when(item.work.small, |d| {
                d.child(hint("Small change, so it takes the fast path.", p))
            })
    }
    fn stop_work(&mut self, project_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ask(
            ask::stop_work(project_id),
            window,
            cx,
            |_, done, window, cx| {
                if let Err(error) = done {
                    toast_error(error, window, cx);
                }
            },
        );
    }

    /// A decision only the human can make: the question, the Assistant's recommendation, a
    /// button per option and a free answer.
    fn decision_card(
        &self,
        card_id: &str,
        decision: &Decision,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let about = (decision.project_id.as_deref())
            .and_then(|id| self.work_project(id))
            .map(|project| format!("{} · needs you", project.name));
        let frame = card_frame(p)
            .border_l_3()
            .border_color(p.focus)
            .max_w(relative(0.88))
            .children(about.map(|about| hint(about, p)))
            .child(title(decision.question.clone()))
            .when(!decision.recommendation.is_empty(), |d| {
                d.child(hint(decision.recommendation.clone(), p))
            });
        if let Some(answer) = &decision.answer {
            return frame.child(hint(format!("You answered: {answer}"), p));
        }
        let mut choices = div().flex().flex_wrap().gap_2();
        for (at, option) in decision.options.iter().enumerate() {
            let (card, answer) = (card_id.to_owned(), option.clone());
            choices = choices.child(
                button(SharedString::from(format!("decide-{card_id}-{at}")))
                    .xsmall()
                    .when(at == 0, |b| b.primary())
                    .label(option.clone())
                    .on_click(cx.listener(move |v, _, w, c| {
                        v.answer_decision(card.clone(), answer.clone(), w, c)
                    })),
            );
        }
        let answering = self.answering.as_deref() == Some(card_id);
        let card = card_id.to_owned();
        choices = choices.child(
            button(SharedString::from(format!("decide-own-{card_id}")))
                .ghost()
                .xsmall()
                .label("Answer in my words")
                .on_click(cx.listener(move |v, _, w, c| v.start_answer(card.clone(), w, c))),
        );
        frame.child(choices).when(answering, |d| {
            d.child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.answer).small()))
                    .child(
                        button("send-own-answer")
                            .primary()
                            .xsmall()
                            .label("Send")
                            .on_click(cx.listener(|v, _, w, c| v.send_answer(w, c))),
                    ),
            )
        })
    }
    fn start_answer(&mut self, card: String, window: &mut Window, cx: &mut Context<Self>) {
        self.answering = Some(card);
        self.answer.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }
    pub(super) fn send_answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = self.answer.read(cx).value().trim().to_owned();
        let Some(card) = self.answering.clone().filter(|_| !answer.is_empty()) else {
            return;
        };
        self.answering = None;
        self.answer_decision(card, answer, window, cx);
    }
    fn answer_decision(
        &mut self,
        card: String,
        answer: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask(
            ask::answer_decision(card, answer),
            window,
            cx,
            |_, done, window, cx| {
                if let Err(error) = done {
                    toast_error(error, window, cx);
                }
            },
        );
    }

    /// An approval only the human gives for work: answering here settles the Inbox item too.
    fn approval_card(
        &self,
        project_id: &str,
        attention_id: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let about = self
            .work_project(project_id)
            .map_or("Work".to_owned(), |project| project.name.clone());
        let attention = (self.shared.snapshot())
            .and_then(|s| s.attention.iter().find(|a| a.id == attention_id));
        let frame = card_frame(p)
            .border_l_3()
            .border_color(p.accent)
            .max_w(relative(0.88))
            .child(hint(format!("{about} · needs your approval"), p));
        let Some(attention) = attention else {
            return frame.child(hint("Settled. Nothing waits here or in the Inbox.", p));
        };
        let options = match attention.options.is_empty() {
            true => vec!["Approve".to_owned(), "Decline".to_owned()],
            false => attention.options.clone(),
        };
        let mut choices = actions();
        for (at, option) in options.into_iter().enumerate() {
            let (id, answer) = (attention.id.clone(), option.clone());
            choices = choices.child(
                button(SharedString::from(format!("approve-{attention_id}-{at}")))
                    .xsmall()
                    .when(at == 0, |b| b.primary())
                    .label(option)
                    .on_click(cx.listener(move |v, _, w, c| {
                        let resolve = Command::ResolveAttention {
                            id: id.clone(),
                            answer: answer.clone(),
                        };
                        v.request(resolve, w, c)
                    })),
            );
        }
        frame
            .child(div().child(attention.prompt.clone()))
            .child(hint("Also in the Inbox; answering either settles both.", p))
            .child(choices)
    }

    /// A loop change: what it made or changed, its schedule in words with the next run, its
    /// sources, and Undo, Edit and Run now; or, in a held turn, Apply and Decline.
    fn loop_card(
        &self,
        card: &ConversationCard,
        change: &LoopCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(recipe) = change.after.as_deref().or(change.before.as_deref()) else {
            return card_frame(p).child(title(change.change.label()));
        };
        let key = recipe.key();
        let job = (self.shared.snapshot())
            .and_then(|s| s.jobs.iter().find(|job| job.recipe.key() == key));
        let next = job.and_then(|job| job.next_runs.first().cloned());
        let waiting = change.state == LoopCardState::Waiting;
        let heading = match waiting {
            true => format!("{}? {}", change.change.label(), recipe.name),
            false => format!("{}: {}", change.change.label(), recipe.name),
        };
        let mut when = schedule_words(recipe);
        if let Some(next) = next.filter(|_| change.change != LoopChange::Delete) {
            when.push_str(&format!(" · next {next}"));
        }
        let mut sources = div().flex().flex_col();
        for source in source_lines(recipe) {
            sources = sources.child(source);
        }
        if recipe.sources.is_empty() {
            sources = sources.child("No app: local files and the web");
        }
        let result = (recipe.output.as_ref()).map_or("A short note in its run".to_owned(), |o| {
            format!(
                "“{}” · {} on Boards",
                o.title,
                jobs::file_type_label(&o.file_type)
            )
        });
        let id = card.id.clone();
        let cards = self.conversation_state().map_or(&[][..], |c| &c.cards);
        let buttons = match change.state {
            LoopCardState::Waiting => {
                let (apply, decline) = (id.clone(), id.clone());
                actions()
                    .child(
                        button(SharedString::from(format!("decline-loop-{id}")))
                            .ghost()
                            .xsmall()
                            .label("Decline")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.card_command(ask::decline_loop_card(decline.clone()), w, c)
                            })),
                    )
                    .child(
                        button(SharedString::from(format!("apply-loop-{id}")))
                            .primary()
                            .xsmall()
                            .label("Apply")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.card_command(ask::apply_loop_card(apply.clone()), w, c)
                            })),
                    )
            }
            LoopCardState::Applied => {
                let mut row = actions();
                if undoable(cards, card) {
                    let undo = id.clone();
                    row = row.child(
                        button(SharedString::from(format!("undo-loop-{id}")))
                            .xsmall()
                            .label("Undo")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.card_command(ask::undo_loop_change(undo.clone()), w, c)
                            })),
                    );
                }
                if job.is_some() {
                    let (edited, run) = (key.clone(), key.clone());
                    row = row
                        .child(
                            button(SharedString::from(format!("edit-loop-{id}")))
                                .ghost()
                                .xsmall()
                                .label("Edit")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    v.open_job(Some(edited.clone()), w, c)
                                })),
                        )
                        .child(
                            button(SharedString::from(format!("run-loop-{id}")))
                                .ghost()
                                .xsmall()
                                .label("Run now")
                                .on_click(cx.listener(move |v, _, w, c| {
                                    v.ask(ask::run_job_now(run.clone()), w, c, |_, ran, w, c| {
                                        match ran {
                                            Ok(_) => toast("Started a run", w, c),
                                            Err(error) => toast_error(error, w, c),
                                        }
                                    })
                                })),
                        );
                }
                row
            }
            LoopCardState::Undone => actions().child(hint("Undone", p)),
            LoopCardState::Declined => actions().child(hint("Declined. Nothing changed.", p)),
        };
        card_frame(p)
            .max_w(relative(0.88))
            .child(title(heading))
            .child(line("When", when, p))
            .child(line("Reads", sources, p))
            .child(line("Result", result, p))
            .child(line("Space", self.space_name(&recipe.space_id), p))
            .child(buttons)
    }
    /// Sends a card's command and shows a refusal.
    fn card_command<R: serde::de::DeserializeOwned + 'static>(
        &mut self,
        ask: ask::Ask<R>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask(ask, window, cx, |_, done, window, cx| {
            if let Err(error) = done {
                toast_error(error, window, cx);
            }
        });
    }

    /// An action on another app: its target and exact content, Run and Decline, then its
    /// result; or a done card for a kind marked Do without asking.
    fn action_card(
        &self,
        card_id: &str,
        action: &ActionCard,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let heading = match (&action.state, action.without_asking) {
            (ActionState::Done { .. }, true) => format!("Done without asking · {}", action.source),
            (ActionState::Done { .. }, false) => format!("Done · {}", action.source),
            _ => format!("Wants to act · {}", action.source),
        };
        let mut content = div().flex().flex_col().gap_1();
        for (label, value) in action_lines(&action.call) {
            content = content.child(line(label, exact(value, p), p));
        }
        let id = card_id.to_owned();
        let state = match &action.state {
            ActionState::Waiting => {
                let (run, decline) = (id.clone(), id.clone());
                actions()
                    .child(
                        button(SharedString::from(format!("decline-action-{id}")))
                            .ghost()
                            .xsmall()
                            .label("Decline")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.card_command(ask::decline_action(decline.clone()), w, c)
                            })),
                    )
                    .child(
                        button(SharedString::from(format!("run-action-{id}")))
                            .primary()
                            .xsmall()
                            .label("Run")
                            .on_click(cx.listener(move |v, _, w, c| {
                                v.card_command(ask::run_action(run.clone()), w, c)
                            })),
                    )
            }
            ActionState::Running => div()
                .flex()
                .items_center()
                .gap_2()
                .child(ui::Spinner::new().xsmall().color(p.accent))
                .child(hint("Running…", p)),
            ActionState::Done { result } => line("Result", exact(result.clone(), p), p),
            ActionState::Failed { error } => {
                line("Error", div().text_color(p.red).child(error.clone()), p)
            }
            ActionState::Declined => hint("Declined. Nothing ran.", p),
        };
        card_frame(p)
            .border_l_3()
            .border_color(match action.state {
                ActionState::Waiting => p.accent,
                _ => p.edge,
            })
            .max_w(relative(0.88))
            .child(title(heading))
            .child(line(
                "Target",
                format!("{} · {}", self.space_name(&action.space_id), action.source),
                p,
            ))
            .child(content)
            .child(state)
    }

    /// What the Assistant wrote to a Space's tracker, with a link to the issue.
    fn issue_card(
        &self,
        space_id: &SpaceId,
        issue_id: &str,
        written: &str,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let issue = IssueId::parse(issue_id).ok();
        card_frame(p)
            .flex_row()
            .items_center()
            .gap_2()
            .child(icon("list-checks").size(px(14.)))
            .child(div().flex_1().child(format!(
                "{written} {issue_id} in {} as you, marked as written by your assistant",
                self.space_name(space_id)
            )))
            .children(issue.map(|issue| {
                button(SharedString::from(format!("open-issue-{issue_id}")))
                    .ghost()
                    .xsmall()
                    .label("Open")
                    .on_click(
                        cx.listener(move |_, _, _, c| c.emit(Intent::OpenIssue(issue.clone()))),
                    )
            }))
            .text_color(p.text)
    }
}

/// Text exactly as it will run or ran, wrapped in full rather than cut short.
fn exact(text: String, p: Palette) -> Div {
    div()
        .min_w_0()
        .font_family(palette::mono_font())
        .text_size(px(font::CAPTION))
        .text_color(p.text)
        .whitespace_normal()
        .child(text)
}
/// The frame every chat card shares.
fn card_frame(p: Palette) -> Div {
    card(p).gap_2().bg(p.base)
}
/// The steps of running work, the one it is at marked.
fn steps_row(stage: Stage, p: Palette) -> Div {
    let mut row = div().flex().flex_wrap().gap_1();
    for (label, mark) in stage.steps() {
        let color = match mark {
            StepMark::Done => p.green,
            StepMark::Now => p.accent,
            StepMark::Later => p.subtle,
        };
        row = row.child(ui::pill(label, color));
    }
    row
}
/// "celadon-docs" from a pull request's URL.
fn repository_of(url: &str) -> &str {
    url.split("/pull/")
        .next()
        .and_then(|repo| repo.rsplit('/').next())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        ActCall, ConversationCard, Kind, LoopCard, LoopCardState, LoopChange, Recipe, action_lines,
        repository_of, schedule_words, undoable,
    };
    use serde_json::json;
    use workspace_core::SpaceId;

    #[test]
    fn an_action_card_shows_the_exact_command_or_each_argument() {
        let cli = ActCall::Cli {
            command: "gws gmail send --to sam@example.com".into(),
        };
        assert_eq!(
            action_lines(&cli),
            [(
                "Command".to_owned(),
                "gws gmail send --to sam@example.com".to_owned()
            )]
        );
        let mcp = ActCall::Mcp {
            tool: "send_message".into(),
            arguments: json!({"to": "Sam <sam@example.com>", "body": "Thursday works.", "cc": []}),
        };
        let lines = action_lines(&mcp);
        assert_eq!(lines[0], ("Tool".into(), "send_message".into()));
        assert!(lines.contains(&("to".into(), "Sam <sam@example.com>".into())));
        assert!(lines.contains(&("body".into(), "Thursday works.".into())));
        assert!(lines.contains(&("cc".into(), "[]".into())));
    }

    fn recipe(cron: &str) -> Recipe {
        serde_json::from_value(json!({
            "id": "sweep", "name": "Open PRs sweep", "space_id": "celadon", "prompt": "",
            "schedule": {"cron": cron, "timezone": "America/Chicago"}, "enabled": true,
        }))
        .unwrap()
    }

    #[test]
    fn a_loop_card_says_its_schedule_in_words() {
        assert_eq!(
            schedule_words(&recipe("0 16 * * 5")),
            "Fridays at 16:00 (America/Chicago)"
        );
    }

    #[test]
    fn undo_is_offered_until_the_loop_changes_again() {
        let change = |id: &str, state| ConversationCard {
            id: id.into(),
            created_at: 0,
            read: vec![],
            kind: Kind::Loop(LoopCard {
                space_id: SpaceId("celadon".into()),
                job_id: "sweep".into(),
                change: LoopChange::Create,
                before: None,
                after: Some(Box::new(recipe("0 16 * * 5"))),
                state,
            }),
        };
        let first = change("first", LoopCardState::Applied);
        let declined = change("declined", LoopCardState::Declined);
        assert!(undoable(&[first.clone(), declined.clone()], &first));
        let later = change("later", LoopCardState::Applied);
        let cards = [first.clone(), declined, later.clone()];
        assert!(!undoable(&cards, &first));
        assert!(undoable(&cards, &later));
    }

    #[test]
    fn a_pull_request_names_its_repository() {
        assert_eq!(
            repository_of("https://github.com/celadon-theme/celadon-docs/pull/88"),
            "celadon-docs"
        );
    }
}
