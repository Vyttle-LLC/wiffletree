//! A child project's questions go to its parent first: the parent's coordinator answers or
//! escalates them, approvals skip it, and the parent hears when the human talks to a child.
use serde_json::{Value, json};
use workspace_core::*;
use workspace_host::Host;

struct Fixture {
    home: tempfile::TempDir,
    host: Host,
}

/// A parent with one child, and their coordinators.
struct Family {
    parent: Project,
    child: Project,
    parent_coordinator: Session,
    child_coordinator: Session,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path().join("home")).unwrap();
        host.set_workspaces_dir(home.path().join("workspaces").to_str().unwrap())
            .unwrap();
        Self { home, host }
    }
    /// A started project.
    fn project(&mut self, name: &str) -> Project {
        let project = self.host.create_project(name).unwrap();
        self.host.set_live(&project.id, true).unwrap();
        project
    }
    fn coordinator(&self, project: &Project) -> Session {
        (self.host.sessions().unwrap().into_iter())
            .find(|s| s.project_id == project.id && s.role == Role::ProjectOrchestrator)
            .unwrap()
    }
    fn file(&mut self, project: &Project, parent: Option<&Project>) {
        let parent = parent.map(|p| p.id.as_str());
        self.host.set_project_parent(&project.id, parent).unwrap();
    }
    fn family(&mut self) -> Family {
        let parent = self.project("Accessible contrast");
        let child = self.project("Contrast warnings");
        self.file(&child, Some(&parent));
        Family {
            parent_coordinator: self.coordinator(&parent),
            child_coordinator: self.coordinator(&child),
            parent,
            child,
        }
    }
    fn tool(&mut self, caller: &Session, name: &str, args: Value) -> anyhow::Result<Value> {
        self.host.agent_tool(&caller.id, name, args)
    }
    fn ask(&mut self, caller: &Session, request: &str) -> Attention {
        let args = json!({"request_id": request, "question": "Warn on comments with no minimum?",
            "options": ["Yes", "No"]});
        serde_json::from_value(self.tool(caller, "ask_user", args).unwrap()).unwrap()
    }
    fn answer(
        &mut self,
        caller: &Session,
        item: &Attention,
        answer: &str,
    ) -> anyhow::Result<Value> {
        let args = json!({"question_id": item.id, "answer": answer});
        self.tool(caller, "answer_question", args)
    }
    fn escalate(&mut self, caller: &Session, item: &Attention) -> anyhow::Result<Value> {
        let args = json!({"question_id": item.id, "note": "Product call on user-facing copy"});
        self.tool(caller, "escalate_question", args)
    }
    fn human_answers(&mut self, item: &Attention, answer: &str) -> anyhow::Result<Value> {
        self.host.execute(Command::ResolveAttention {
            id: item.id.clone(),
            answer: answer.into(),
        })
    }
    /// The ids of the items in the human's Inbox.
    fn inbox(&self) -> Vec<String> {
        let snapshot = self.host.snapshot().unwrap();
        snapshot.attention.into_iter().map(|a| a.id).collect()
    }
    fn with_parent(&self, project: &Project) -> u32 {
        let snapshot = self.host.snapshot().unwrap();
        snapshot
            .questions_with_parent
            .get(&project.id)
            .copied()
            .unwrap_or(0)
    }
    fn received(&self, session: &Session, prefix: &str) -> Vec<Message> {
        (self
            .host
            .messages(&session.id, None, 100)
            .unwrap()
            .into_iter())
        .filter(|m| m.id.starts_with(prefix))
        .collect()
    }
    /// Input waiting to start `session`'s next turn; quiet notices are not counted.
    fn waking(&self, session: &Session) -> u32 {
        let snapshot = self.host.snapshot().unwrap();
        (snapshot.undelivered.iter())
            .find(|u| u.session_id == session.id)
            .map_or(0, |u| u.queued)
    }
    /// An unanswered item, wherever it waits.
    fn item(&self, id: &str) -> Attention {
        (self.host.projects().unwrap().iter())
            .flat_map(|p| self.host.open_attention(&p.id).unwrap())
            .find(|a| a.id == id)
            .unwrap()
    }
}

fn refusal(result: anyhow::Result<Value>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn a_childs_question_waits_with_its_parent_which_is_woken_with_it() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "warn-comments");

    assert!(f.inbox().is_empty());
    assert_eq!(f.host.attention_count().unwrap(), 0);
    assert_eq!(f.with_parent(&family.child), 1);
    let asked = f.received(&family.parent_coordinator, "child-question:");
    assert_eq!(asked.len(), 1);
    assert_eq!(host_notice(&asked[0].id), Some("Wiffletree"));
    for part in [
        "\"Contrast warnings\"",
        item.id.as_str(),
        "Warn on comments with no minimum?",
        "Yes | No",
    ] {
        assert!(asked[0].body.contains(part), "{part}: {}", asked[0].body);
    }
    assert_eq!(f.waking(&family.parent_coordinator), 1);

    // A retried request returns the same item and asks the parent once.
    assert_eq!(
        f.ask(&family.child_coordinator, "warn-comments").id,
        item.id
    );
    assert_eq!(
        f.received(&family.parent_coordinator, "child-question:")
            .len(),
        1
    );
}

#[test]
fn the_parents_answer_closes_the_question_and_resumes_the_child_as_its_own() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "warn-comments");
    let answered: Attention = serde_json::from_value(
        f.answer(
            &family.parent_coordinator,
            &item,
            "Yes, WCAG AA is always the floor.",
        )
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        answered.answer.as_deref(),
        Some("Yes, WCAG AA is always the floor.")
    );
    assert!(f.inbox().is_empty());
    assert_eq!(f.with_parent(&family.child), 0);
    let resumed = f.received(&family.child_coordinator, "parent-answer:");
    assert_eq!(resumed.len(), 1);
    assert_eq!(
        resumed[0].sender.as_ref(),
        Some(&family.parent_coordinator.id)
    );
    assert!(resumed[0].body.contains("\"Accessible contrast\""));
    assert!(
        resumed[0]
            .body
            .contains("Yes, WCAG AA is always the floor.")
    );
    assert_eq!(f.waking(&family.child_coordinator), 1);
}

#[test]
fn an_escalated_question_reaches_the_human_and_their_answer_tells_the_parent_quietly() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "copy");
    f.escalate(&family.parent_coordinator, &item).unwrap();

    assert_eq!(f.inbox(), std::slice::from_ref(&item.id));
    assert_eq!(f.host.attention_count().unwrap(), 1);
    assert_eq!(f.with_parent(&family.child), 0);
    assert_eq!(
        f.item(&item.id).route,
        QuestionRoute::Escalated {
            parent_id: family.parent.id.clone(),
            note: Some("Product call on user-facing copy".into()),
        }
    );

    let waking = f.waking(&family.parent_coordinator);
    f.human_answers(&item, "Short, one line.\nNo ratio.")
        .unwrap();
    let heard = f.received(&family.parent_coordinator, "child-heard:");
    assert_eq!(heard.len(), 1);
    assert_eq!(
        heard[0].body,
        "The human answered a question of your child project \"Contrast warnings\": Short, one line."
    );
    assert_eq!(f.waking(&family.parent_coordinator), waking, "no turn");

    // Answered twice: the parent is refused, and the child has only the human's answer.
    let late = refusal(f.answer(&family.parent_coordinator, &item, "Long"));
    assert!(late.contains("already answered"), "{late}");
    assert!(
        f.received(&family.child_coordinator, "parent-answer:")
            .is_empty()
    );
}

#[test]
fn a_race_either_way_is_refused_with_where_the_question_went() {
    let mut f = Fixture::new();
    let family = f.family();
    let escalated = f.ask(&family.child_coordinator, "one");
    f.escalate(&family.parent_coordinator, &escalated).unwrap();
    for refused in [
        refusal(f.answer(&family.parent_coordinator, &escalated, "Yes")),
        refusal(f.escalate(&family.parent_coordinator, &escalated)),
    ] {
        assert!(refused.contains("already with the human"), "{refused}");
    }

    let answered = f.ask(&family.child_coordinator, "two");
    f.answer(&family.parent_coordinator, &answered, "Yes")
        .unwrap();
    for refused in [
        refusal(f.escalate(&family.parent_coordinator, &answered)),
        refusal(f.answer(&family.parent_coordinator, &answered, "No")),
        refusal(f.human_answers(&answered, "No")),
    ] {
        assert!(refused.contains("already"), "{refused}");
    }
}

#[test]
fn only_the_parent_resolves_a_question_with_it() {
    let mut f = Fixture::new();
    let family = f.family();
    let stranger = f.project("Unrelated");
    let stranger = f.coordinator(&stranger);
    let item = f.ask(&family.child_coordinator, "warn-comments");

    for caller in [&stranger, &family.child_coordinator] {
        let refused = refusal(f.answer(caller, &item, "Yes"));
        assert!(
            refused.contains("not one of your child projects"),
            "{refused}"
        );
        assert!(f.escalate(caller, &item).is_err());
    }
    // The parent's own question reaches the human, and is not its to answer.
    let own = f.ask(&family.parent_coordinator, "own");
    assert_eq!(f.inbox(), std::slice::from_ref(&own.id));
    assert!(f.answer(&family.parent_coordinator, &own, "Yes").is_err());
    // Nor is the child's the human's until it is escalated.
    let early = refusal(f.human_answers(&item, "Yes"));
    assert!(early.contains("with its parent project"), "{early}");
    assert!(
        f.host
            .execute(Command::DismissAttention {
                id: item.id.clone()
            })
            .is_err()
    );
    assert_eq!(f.item(&item.id).answer, None);
    assert_eq!(f.with_parent(&family.child), 1);
}

#[test]
fn a_question_reaches_the_human_when_its_parent_is_not_started() {
    let mut f = Fixture::new();
    let parent = f.host.create_project("Not started").unwrap();
    let child = f.project("Started child");
    f.file(&child, Some(&parent));
    let coordinator = f.coordinator(&child);
    let item = f.ask(&coordinator, "warn-comments");

    assert_eq!(f.inbox(), [item.id]);
    assert_eq!(f.with_parent(&child), 0);
    assert!(
        f.received(&f.coordinator(&parent), "child-question:")
            .is_empty()
    );
}

#[test]
fn the_humans_answer_is_kept_when_the_parents_queue_is_full() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "copy");
    f.escalate(&family.parent_coordinator, &item).unwrap();
    let parent = &family.parent_coordinator.id;
    let queued = f.waking(&family.parent_coordinator);
    for n in queued..1024 {
        f.host
            .send(&format!("fill-{n}"), None, parent, "x")
            .unwrap();
    }
    assert!(f.host.send("one-more", None, parent, "x").is_err());

    f.human_answers(&item, "Short").unwrap();
    assert_eq!(
        f.received(&family.parent_coordinator, "child-heard:").len(),
        1
    );
}

#[test]
fn an_approval_skips_the_parent() {
    let mut f = Fixture::new();
    let family = f.family();
    let approval = f
        .host
        .request_attention(
            &family.child_coordinator.id,
            "local",
            &format!("{PUBLISH_PREFIX}t:push"),
            "Push?",
            &[],
        )
        .unwrap();
    assert_eq!(approval.route, QuestionRoute::Human);
    assert_eq!(f.inbox(), [approval.id]);
    assert!(
        f.received(&family.parent_coordinator, "child-question:")
            .is_empty()
    );
}

#[test]
fn unfiling_a_child_sends_its_waiting_questions_to_the_human() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "warn-comments");
    f.file(&family.child, None);

    assert_eq!(f.inbox(), std::slice::from_ref(&item.id));
    assert_eq!(f.item(&item.id).route, QuestionRoute::Human);
    assert_eq!(f.with_parent(&family.child), 0);
    assert!(f.answer(&family.parent_coordinator, &item, "Yes").is_err());
}

#[test]
fn refiling_a_child_sends_its_waiting_questions_to_the_human_not_the_new_parent() {
    let mut f = Fixture::new();
    let family = f.family();
    let light = f.project("Light themes");
    let light_coordinator = f.coordinator(&light);
    let item = f.ask(&family.child_coordinator, "warn-comments");
    f.file(&family.child, Some(&light));

    assert_eq!(f.inbox(), std::slice::from_ref(&item.id));
    assert!(f.answer(&family.parent_coordinator, &item, "Yes").is_err());
    assert!(f.answer(&light_coordinator, &item, "Yes").is_err());
    assert!(f.received(&light_coordinator, "child-question:").is_empty());

    // The child's next question goes to its new parent.
    let next = f.ask(&family.child_coordinator, "next");
    assert_eq!(f.received(&light_coordinator, "child-question:").len(), 1);
    f.answer(&light_coordinator, &next, "No").unwrap();
}

#[test]
fn moving_a_child_to_another_space_sends_its_waiting_questions_to_the_human() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "warn-comments");
    let vyttle = f.host.create_space("Vyttle", "", "", None).unwrap().space;
    f.host
        .move_project_to_space(&family.child.id, &vyttle.id)
        .unwrap();
    assert_eq!(f.inbox(), [item.id]);
}

#[test]
fn filing_keeps_the_humans_questions_and_routes_the_next_to_the_parent() {
    let mut f = Fixture::new();
    let parent = f.project("Sponsorships");
    let loose = f.project("Redo sponsor page");
    let coordinator = f.coordinator(&loose);
    let before = f.ask(&coordinator, "before");
    f.file(&loose, Some(&parent));
    let after = f.ask(&coordinator, "after");

    assert_eq!(f.inbox(), [before.id]);
    assert!(matches!(
        f.item(&after.id).route,
        QuestionRoute::Parent { .. }
    ));
}

#[test]
fn the_parent_hears_of_the_humans_message_to_a_child_without_a_turn() {
    let mut f = Fixture::new();
    let family = f.family();
    let loose = f.project("Loose");
    let loose = f.coordinator(&loose);
    for (id, recipient) in [("m1", &family.child_coordinator), ("m2", &loose)] {
        f.host
            .execute(Command::Send {
                id: id.into(),
                sender: None,
                recipient: recipient.id.clone(),
                body: "For the warning popup, keep the wording short.\nOne line, no ratio.".into(),
                attachments: vec![],
                uploads: vec![],
            })
            .unwrap();
    }
    let heard = f.received(&family.parent_coordinator, "child-heard:");
    assert_eq!(heard.len(), 1);
    assert_eq!(
        heard[0].body,
        "The human messaged your child project \"Contrast warnings\": For the warning popup, keep the wording short."
    );
    assert_eq!(f.waking(&family.parent_coordinator), 0);
}

#[test]
fn a_childs_blocked_report_waits_until_its_parents_answer_reaches_it() {
    let mut f = Fixture::new();
    let family = f.family();
    let blocked = json!({"message_id": "b1", "kind": "blocked", "body": "Which minimum?"});
    f.tool(&family.child_coordinator, "report", blocked)
        .unwrap();
    let waiting = |f: &Fixture| {
        let snapshot = f.host.snapshot().unwrap();
        snapshot.questions_waiting(&family.child_coordinator.id)
    };
    assert_eq!(waiting(&f), 1);

    let item = f.ask(&family.child_coordinator, "minimum");
    f.answer(&family.parent_coordinator, &item, "AA").unwrap();
    assert_eq!(waiting(&f), 1, "queued, not yet taken");
    f.host
        .advance_receipt(&format!("parent-answer:{}", item.id), Receipt::Delivered)
        .unwrap();
    assert_eq!(waiting(&f), 0);
}

#[test]
fn a_childs_blocked_report_waits_until_the_humans_escalated_answer_reaches_it() {
    let mut f = Fixture::new();
    let family = f.family();
    let blocked = json!({"message_id": "b1", "kind": "blocked", "body": "Which copy?"});
    f.tool(&family.child_coordinator, "report", blocked)
        .unwrap();
    let item = f.ask(&family.child_coordinator, "copy");
    f.escalate(&family.parent_coordinator, &item).unwrap();
    f.human_answers(&item, "Short").unwrap();
    // The service sends the human's answer as `answer:<id>`, from no one.
    let answer = format!("answer:{}", item.id);
    f.host
        .send(&answer, None, &family.child_coordinator.id, "Short")
        .unwrap();
    f.host.advance_receipt(&answer, Receipt::Delivered).unwrap();

    let snapshot = f.host.snapshot().unwrap();
    assert_eq!(snapshot.questions_waiting(&family.child_coordinator.id), 0);
}

#[test]
fn a_question_with_the_parent_keeps_its_route_across_a_restart() {
    let mut f = Fixture::new();
    let family = f.family();
    let item = f.ask(&family.child_coordinator, "warn-comments");

    let Fixture { home, host } = f;
    drop(host);
    let host = Host::open_set_up(home.path().join("home")).unwrap();
    let mut f = Fixture { home, host };
    assert!(f.inbox().is_empty());
    assert_eq!(f.with_parent(&family.child), 1);
    f.answer(&family.parent_coordinator, &item, "Yes").unwrap();
    assert_eq!(f.with_parent(&family.child), 0);
}
