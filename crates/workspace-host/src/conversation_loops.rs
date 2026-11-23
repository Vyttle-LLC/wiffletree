//! The Assistant's loop tools over every Space's loops (jobs, as their recipes still call them).
//! A change applies at once in a turn that is not held, with a card offering Undo; in a held
//! turn it waits on a loop card for the human's Apply (scheduled-jobs, "Only the human's word
//! saves a loop").
use crate::conversation::{TurnGuard, without_space};
use crate::*;
use workspace_core::assistant::conversation::{
    CardKind, ConversationCard, LoopCard, LoopCardState, LoopChange,
};
use workspace_core::job::{JobKey, Recipe};

/// The recipe fields `create_loop` and `edit_loop` take.
const RECIPE_FIELDS: [&str; 8] = [
    "name",
    "prompt",
    "cron",
    "timezone",
    "sources",
    "output_title",
    "output_type",
    "sharing",
];

impl Host {
    /// `list_loops`: every Space's loops with their schedule, next runs and last result.
    pub(crate) fn list_loops(&self) -> Result<Value> {
        let loops = (self.job_states()?.into_iter())
            .filter(|state| !state.recipe.key().is_built_in())
            .map(|state| {
                let last = state
                    .runs
                    .first()
                    .map(|run| json!({"outcome": run.outcome, "started_at": run.started_at}));
                json!({
                    "space": state.recipe.space_id,
                    "job_id": state.recipe.id,
                    "name": state.recipe.name,
                    "schedule": state.schedule,
                    "enabled": state.recipe.enabled,
                    "next_runs": state.next_runs,
                    "sources": state.recipe.sources,
                    "last_run": last,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"loops": loops}))
    }
    /// A loop tool: the change applied at once with a card offering Undo, or in a held turn a
    /// card waiting for the human's Apply, naming what the turn read.
    pub(crate) fn loop_tool(
        &mut self,
        name: &str,
        args: &Value,
        guard: &TurnGuard,
    ) -> Result<Value> {
        let space = self.named_space(args, guard)?;
        let args = without_space(args);
        let existing = |host: &Self| -> Result<Recipe> {
            let id = args["job_id"].as_str().context("Missing job_id")?;
            host.recipe(&JobKey::recipe(&space.id, id))
        };
        let (change, before, after) = match name {
            "create_loop" => {
                service::only_arguments(name, &args, &RECIPE_FIELDS)?;
                let recipe = assistant::proposed_recipe(&space.id, &args)?;
                self.refuse_taken_job(&recipe)?;
                (LoopChange::Create, None, Some(recipe))
            }
            "edit_loop" => {
                let fields: Vec<&str> = RECIPE_FIELDS.iter().copied().chain(["job_id"]).collect();
                service::only_arguments(name, &args, &fields)?;
                let before = existing(self)?;
                (
                    LoopChange::Edit,
                    Some(before.clone()),
                    Some(edited(&before, &args)?),
                )
            }
            "pause_loop" | "resume_loop" => {
                service::only_arguments(name, &args, &["job_id"])?;
                let before = existing(self)?;
                let after = Recipe {
                    enabled: name == "resume_loop",
                    ..before.clone()
                };
                let change = match after.enabled {
                    true => LoopChange::Resume,
                    false => LoopChange::Pause,
                };
                (change, Some(before), Some(after))
            }
            "delete_loop" => {
                service::only_arguments(name, &args, &["job_id"])?;
                (LoopChange::Delete, Some(existing(self)?), None)
            }
            _ => {
                service::only_arguments(name, &args, &["job_id"])?;
                let recipe = existing(self)?;
                (LoopChange::Run, Some(recipe.clone()), Some(recipe))
            }
        };
        if let Some(after) = &after {
            jobs::validate(after)?;
            jobs::refuse_acts_as_reads(&space, after)?;
        }
        let job_id = (after.as_ref().or(before.as_ref()))
            .map(|recipe| recipe.id.clone())
            .unwrap_or_default();
        let mut card = LoopCard {
            space_id: space.id,
            job_id,
            change,
            before: before.map(Box::new),
            after: after.map(Box::new),
            state: LoopCardState::Waiting,
        };
        if guard.held() {
            let card = self.raise_card(CardKind::Loop(card), guard)?;
            return Ok(json!({
                "card": card.id,
                "applied": false,
                "note": format!("Nothing changed: this turn read text written by others ({}), so the human sees a loop card and applies it with Apply.", guard.read.join("; ")),
            }));
        }
        let applied = self.apply_loop_change(&mut card)?;
        card.state = LoopCardState::Applied;
        let card = self.raise_card(CardKind::Loop(card), guard)?;
        Ok(json!({"card": card.id, "applied": true, "loop": applied}))
    }
    /// Makes the change; a saved loop is kept on the card as `save_job` stored it, with the
    /// runner it may have added, so Undo finds it unchanged.
    fn apply_loop_change(&mut self, card: &mut LoopCard) -> Result<Value> {
        let recipe = || card.after.as_deref().cloned().context("No loop to save");
        Ok(match card.change {
            LoopChange::Create | LoopChange::Edit | LoopChange::Pause | LoopChange::Resume => {
                let saved = self.save_job(recipe()?)?;
                card.after = Some(Box::new(saved.recipe.clone()));
                serde_json::to_value(saved)?
            }
            LoopChange::Delete => {
                self.delete_job(&card.space_id, &card.job_id)?;
                Value::Null
            }
            LoopChange::Run => {
                let key = JobKey::recipe(&card.space_id, &card.job_id);
                serde_json::to_value(self.run_job_now(&key)?)?
            }
        })
    }
    fn loop_card(&self, id: &str) -> Result<(ConversationCard, LoopCard)> {
        let card = self.conversation_card(id)?;
        let CardKind::Loop(change) = &card.kind else {
            bail!("This card changes no loop");
        };
        let change = change.clone();
        Ok((card, change))
    }
    fn set_loop_card(
        &self,
        mut card: ConversationCard,
        change: LoopCard,
    ) -> Result<ConversationCard> {
        card.kind = CardKind::Loop(change);
        self.save_card(&card)?;
        Ok(card)
    }
    /// The human's Apply on a loop card a held turn raised.
    pub fn apply_loop_card(&mut self, id: &str) -> Result<ConversationCard> {
        let (card, mut change) = self.loop_card(id)?;
        ensure!(
            change.state == LoopCardState::Waiting,
            "This loop change is not waiting"
        );
        self.apply_loop_change(&mut change)?;
        change.state = LoopCardState::Applied;
        self.set_loop_card(card, change)
    }
    /// The human's Decline on a loop card a held turn raised: nothing changes.
    pub fn decline_loop_card(&mut self, id: &str) -> Result<ConversationCard> {
        let (card, mut change) = self.loop_card(id)?;
        ensure!(
            change.state == LoopCardState::Waiting,
            "This loop change is not waiting"
        );
        change.state = LoopCardState::Declined;
        self.set_loop_card(card, change)
    }
    /// Undo on an applied loop change: the loop as it was before, or none for a loop it
    /// created. Offered until the loop changes again; a run cannot be undone.
    pub fn undo_loop_change(&mut self, id: &str) -> Result<ConversationCard> {
        let (card, mut change) = self.loop_card(id)?;
        ensure!(
            change.state == LoopCardState::Applied && change.change != LoopChange::Run,
            "This loop change cannot be undone"
        );
        let key = JobKey::recipe(&change.space_id, &change.job_id);
        let now_saved = self.recipe(&key).ok();
        ensure!(
            now_saved.as_ref() == change.after.as_deref(),
            "The loop changed since; change it on its page"
        );
        match change.before.as_deref() {
            Some(before) => {
                self.save_job(before.clone())?;
            }
            None => self.delete_job(&change.space_id, &change.job_id)?,
        }
        change.state = LoopCardState::Undone;
        self.set_loop_card(card, change)
    }
}

/// `before` with the fields `args` gives, keeping its id, model, runner and state.
fn edited(before: &Recipe, args: &Value) -> Result<Recipe> {
    let mut fields = json!({
        "name": before.name,
        "prompt": before.prompt,
        "cron": before.schedule.cron,
        "timezone": before.schedule.timezone,
        "sources": before.sources,
        "sharing": before.sharing,
    });
    if let Some(output) = &before.output {
        fields["output_title"] = json!(output.title);
        fields["output_type"] = json!(output.file_type);
    }
    for key in RECIPE_FIELDS {
        if let Some(value) = args.get(key).filter(|value| !value.is_null()) {
            fields[key] = value.clone();
        }
    }
    let parsed = assistant::proposed_recipe(&before.space_id, &fields)?;
    Ok(Recipe {
        id: before.id.clone(),
        model: before.model.clone(),
        runner: before.runner.clone(),
        enabled: before.enabled,
        extra: before.extra.clone(),
        ..parsed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::tests::{assistant_with_space, human_turn};

    fn card_state(host: &Host, card: &Value) -> LoopCardState {
        match host
            .conversation_card(card["card"].as_str().unwrap())
            .unwrap()
            .kind
        {
            CardKind::Loop(change) => change.state,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn each_loop_change_applies_at_once_in_the_humans_turn_and_undo_restores_what_was_before() {
        let (_home, mut host, _, celadon) = assistant_with_space();
        let guard = human_turn(&mut host, "m1", "every friday tell me which PRs are open");
        let key = JobKey::recipe(&celadon, "open-prs-sweep");
        let created = host.loop_tool("create_loop", &json!({"space": celadon, "name": "Open PRs sweep", "prompt": "List open PRs", "cron": "0 16 * * 5", "timezone": "America/New_York", "sources": []}), &guard).unwrap();
        assert_eq!(created["applied"], true);
        let edited = host
            .loop_tool(
                "edit_loop",
                &json!({"space": celadon, "job_id": "open-prs-sweep", "cron": "0 9 * * 1"}),
                &guard,
            )
            .unwrap();
        assert_eq!(host.recipe(&key).unwrap().schedule.cron, "0 9 * * 1");
        assert_eq!(
            host.recipe(&key).unwrap().prompt,
            "List open PRs",
            "the rest is kept"
        );
        // Undo is offered until the loop changes again.
        assert!(
            host.undo_loop_change(created["card"].as_str().unwrap())
                .is_err()
        );
        host.undo_loop_change(edited["card"].as_str().unwrap())
            .unwrap();
        assert_eq!(host.recipe(&key).unwrap().schedule.cron, "0 16 * * 5");
        assert_eq!(card_state(&host, &edited), LoopCardState::Undone);
        let paused = host
            .loop_tool(
                "pause_loop",
                &json!({"space": celadon, "job_id": "open-prs-sweep"}),
                &guard,
            )
            .unwrap();
        assert!(!host.recipe(&key).unwrap().enabled);
        host.undo_loop_change(paused["card"].as_str().unwrap())
            .unwrap();
        assert!(host.recipe(&key).unwrap().enabled);
        let deleted = host
            .loop_tool(
                "delete_loop",
                &json!({"space": celadon, "job_id": "open-prs-sweep"}),
                &guard,
            )
            .unwrap();
        assert!(host.recipe(&key).is_err());
        host.undo_loop_change(deleted["card"].as_str().unwrap())
            .unwrap();
        assert_eq!(host.recipe(&key).unwrap().name, "Open PRs sweep");
    }

    #[test]
    fn a_loop_only_reads_and_a_held_turn_changes_nothing_until_apply() {
        let (_home, mut host, _, celadon) = assistant_with_space();
        let mut space = host.space(&celadon).unwrap().space;
        space.sources = vec![serde_json::from_value(json!({"kind":"mcp","server":"gmail","tools":["search_threads"],"act":["send_message"]})).unwrap()];
        host.update_space(space).unwrap();
        let guard = human_turn(&mut host, "m1", "a loop please");
        let recipe = |sources: Value| json!({"space": celadon, "name": "Mail digest", "prompt": "Summarise", "cron": "0 7 * * *", "timezone": "UTC", "sources": sources});
        let sends = host.loop_tool(
            "create_loop",
            &recipe(json!([{"kind":"mcp","server":"gmail","tools":["send_message"]}])),
            &guard,
        );
        assert!(sends.unwrap_err().to_string().contains("send_message"));
        let acts = host.loop_tool("create_loop", &recipe(json!([{"kind":"mcp","server":"gmail","tools":["search_threads"],"act":["send_message"]}])), &guard);
        assert!(acts.unwrap_err().to_string().contains("A loop only reads"));
        // The human's own save, from a loop's page, is held to the same reads.
        let mut by_hand = assistant::proposed_recipe(&celadon, &recipe(json!([]))).unwrap();
        by_hand.sources = vec![
            serde_json::from_value(json!({"kind":"mcp","server":"gmail","tools":["send_message"]}))
                .unwrap(),
        ];
        let refused = host.save_job(by_hand).unwrap_err().to_string();
        assert!(refused.contains("send_message"), "{refused}");
        let comments = host.loop_tool(
            "create_loop",
            &recipe(json!([{"kind":"cli","command":"gh","prefixes":["gh pr comment"]}])),
            &guard,
        );
        assert!(comments.unwrap_err().to_string().contains("gh pr comment"));

        let mut held = guard.clone();
        held.note("Celadon's brain".into());
        let card = host
            .loop_tool(
                "create_loop",
                &recipe(json!([{"kind":"mcp","server":"gmail","tools":["search_threads"]}])),
                &held,
            )
            .unwrap();
        assert_eq!(card["applied"], false);
        assert!(host.recipes().unwrap().is_empty());
        host.apply_loop_card(card["card"].as_str().unwrap())
            .unwrap();
        let saved = host.recipes().unwrap().pop().unwrap();
        assert_eq!(saved.sources[0].reads(), ["search_threads"]);
        assert!(saved.sources[0].acts().is_empty());
        let run = host
            .loop_tool(
                "run_loop",
                &json!({"space": celadon, "job_id": saved.id}),
                &held,
            )
            .unwrap();
        assert_eq!(card_state(&host, &run), LoopCardState::Waiting);
        host.decline_loop_card(run["card"].as_str().unwrap())
            .unwrap();
        assert_eq!(card_state(&host, &run), LoopCardState::Declined);
        assert!(host.job_runs(&saved.key(), 10).unwrap().is_empty());
    }
}
