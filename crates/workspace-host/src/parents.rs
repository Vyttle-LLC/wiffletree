//! Parent projects: a project may be filed under one loose project of its Space, one level
//! deep. The link decides which coordinators may message each other, where a child
//! coordinator's reports go and when a project may be archived or restored.
use crate::*;
use std::collections::BTreeMap;

const MOVED_UNDER: &str = "project_moved_under";
const MOVED_OUT: &str = "project_moved_out";
/// The SQL condition for an unanswered Inbox item with the human, rather than with a parent.
pub(crate) const WITH_THE_HUMAN: &str =
    "json_extract(data,'$.answer') IS NULL AND json_extract(data,'$.route.with') IS NOT 'parent'";
/// The refusal for the human's answer to a question still with a parent project.
pub(crate) const WITH_PARENT: &str =
    "This question is with its parent project's coordinator, which answers it or sends it to you";

impl Host {
    /// The projects filed under `project`, in the stored order.
    pub(crate) fn child_projects(&self, project: &str) -> Result<Vec<Project>> {
        self.list_data(
            "SELECT data FROM projects WHERE json_extract(data,'$.parent_id')=?1 ORDER BY position",
            [project],
        )
    }
    /// Refuses a link of `project` under `parent` that breaks a rule of the tree, naming it.
    pub(crate) fn check_parent(&self, project: &Project, parent: &Project) -> Result<()> {
        ensure!(
            parent.id != project.id,
            "A project cannot be its own parent"
        );
        for side in [project, parent] {
            ensure!(
                side.assistant_work.is_none(),
                "\"{}\" is the Assistant's work, which lives under the Assistant and is never filed under or over another project",
                side.name
            );
        }
        ensure!(
            parent.space == project.space,
            "A parent must be in the same Space: \"{}\" and \"{}\" are in different Spaces",
            parent.name,
            project.name
        );
        ensure!(
            parent.parent_id.is_none(),
            "\"{}\" is a child, and a child cannot have children",
            parent.name
        );
        ensure!(
            self.child_projects(&project.id)?.is_empty(),
            "\"{}\" has children, and a project with children cannot have a parent",
            project.name
        );
        ensure!(
            !self.project_archived(&parent.id)?,
            "\"{}\" is archived; restore it before filing projects under it",
            parent.name
        );
        Ok(())
    }
    /// The human's Move under…: files `project` under `parent`, or makes it loose with none,
    /// recording the change on the project and on each parent it joined or left.
    pub fn set_project_parent(&mut self, project: &str, parent: Option<&str>) -> Result<Project> {
        let mut project = self.project(project)?;
        let parent = parent.map(|id| self.project(id)).transpose()?;
        if let Some(parent) = &parent {
            self.check_parent(&project, parent)?;
        }
        if project.parent_id == parent.as_ref().map(|p| p.id.clone()) {
            return Ok(project);
        }
        self.atomically(|host| {
            host.leave_parent(&mut project)?;
            if let Some(parent) = &parent {
                host.join_parent(&mut project, parent)?;
            }
            Ok(project)
        })
    }
    /// Files a loose `project` under `parent`, already checked, and records it on both.
    pub(crate) fn join_parent(&self, project: &mut Project, parent: &Project) -> Result<()> {
        project.parent_id = Some(parent.id.clone());
        self.save_project(project)?;
        Self::event(&self.db, &project.id, None, MOVED_UNDER, &parent.name)?;
        Self::event(&self.db, &parent.id, None, MOVED_UNDER, &project.name)
    }
    /// Makes `project` loose, if it has a parent, and records it on both. Its questions still
    /// with the old parent go to the human in the same change, and the old parent's wakes for
    /// them that have not reached a turn are cancelled.
    pub(crate) fn leave_parent(&self, project: &mut Project) -> Result<()> {
        let Some(old) = project.parent_id.take() else {
            return Ok(());
        };
        self.save_project(project)?;
        for mut item in self.questions_with_parent_of(&project.id)? {
            item.route = QuestionRoute::Human;
            self.save_attention(&item)?;
            self.db.execute(
                "UPDATE messages SET receipt='cancelled' WHERE id=?1 AND receipt IN ('queued','held')",
                [format!("{CHILD_QUESTION_PREFIX}{}", item.id)],
            )?;
        }
        let old = self.project(&old)?;
        Self::event(&self.db, &project.id, None, MOVED_OUT, &old.name)?;
        Self::event(&self.db, &old.id, None, MOVED_OUT, &project.name)
    }
    /// The human's Standing flag, which keeps a project out of Ready to close.
    pub fn set_project_standing(&mut self, project: &str, standing: bool) -> Result<Project> {
        let mut project = self.project(project)?;
        project.standing = standing;
        self.save_project(&project)?;
        Ok(project)
    }
    pub(crate) fn save_project(&self, project: &Project) -> Result<()> {
        self.db.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![project.id, encode(project)?],
        )?;
        Ok(())
    }
    /// The coordinator of the parent of `session`'s project, when `session` is that project's
    /// coordinator and the project has a parent.
    pub(crate) fn parent_coordinator(&self, session: &Session) -> Result<Option<String>> {
        if !is_project_coordinator(session) {
            return Ok(None);
        }
        match self.project(&session.project_id)?.parent_id {
            Some(parent) => self.coordinator_of(&parent),
            None => Ok(None),
        }
    }
    /// Whether `from` and `to` are the coordinators of a parent and one of its children.
    pub(crate) fn linked_coordinators(&self, from: &Session, to: &Session) -> Result<bool> {
        if !is_project_coordinator(from) || !is_project_coordinator(to) {
            return Ok(false);
        }
        let (a, b) = (
            self.project(&from.project_id)?,
            self.project(&to.project_id)?,
        );
        Ok(a.parent_id.as_ref() == Some(&b.id) || b.parent_id.as_ref() == Some(&a.id))
    }
    /// Whom `session` may message, for a refusal to say.
    pub(crate) fn who_may_be_messaged(&self, session: &Session) -> Result<String> {
        let mut whom = vec![];
        if session.parent_id.is_some() {
            whom.push("your parent");
        }
        if is_project_coordinator(session) {
            let project = self.project(&session.project_id)?;
            if project.parent_id.is_some() {
                whom.push("your parent project's coordinator");
            }
            if !self.child_projects(&project.id)?.is_empty() {
                whom.push("your child projects' coordinators");
            }
        }
        // Only a coordinator owns agents; a worker answers to its parent alone.
        if session.role == Role::ProjectOrchestrator {
            whom.push("your own agents");
        }
        Ok(match whom.split_last() {
            None => "no one".into(),
            Some((only, [])) => (*only).into(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        })
    }
    /// A coordinator's `ask_user`. A child project's question waits with its parent, whose
    /// coordinator is woken with it; any other, or one whose parent is not started and so
    /// could not wake, reaches the human. A retried request returns the item it first made,
    /// wherever it is now.
    pub(crate) fn ask_user(
        &mut self,
        session: &Session,
        request: &str,
        question: &str,
        options: &[String],
    ) -> Result<Attention> {
        let asked_before = (self.db)
            .query_row(
                "SELECT 1 FROM attention WHERE session_id=?1 AND operation_id=?2",
                params![session.id, request],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let parent = match self.parent_coordinator(session)? {
            Some(coordinator) if !asked_before => Some(self.session(&coordinator)?),
            _ => None,
        };
        let live = self.live_projects()?;
        let Some(parent) = parent.filter(|p| live.contains(&p.project_id)) else {
            return self.request_attention(&session.id, "local", request, question, options);
        };
        let child = self.project(&session.project_id)?;
        self.atomically(|host| {
            let mut item =
                host.request_attention(&session.id, "local", request, question, options)?;
            item.route = QuestionRoute::Parent {
                parent_id: parent.project_id.clone(),
            };
            host.save_attention(&item)?;
            let choices = match options.is_empty() {
                true => String::new(),
                false => format!("\nChoices: {}", options.join(" | ")),
            };
            host.send(
                &format!("{CHILD_QUESTION_PREFIX}{}", item.id),
                None,
                &parent.id,
                &format!(
                    "Your child project \"{}\" asks, as question {}:\n{question}{choices}\nAnswer it with answer_question only from decisions already made, or send it to the human with escalate_question. Escalate every product, security and approval question.",
                    child.name, item.id
                ),
            )?;
            Ok(item)
        })
    }
    /// The parent coordinator `caller`'s `answer_question`: closes its child's question and
    /// resumes the child with the answer, from the parent.
    pub(crate) fn answer_question(
        &mut self,
        caller: &Session,
        question: &str,
        answer: &str,
    ) -> Result<Attention> {
        text(answer, 4096)?;
        let item = self.question_with(caller, question)?;
        let parent = self.project(&caller.project_id)?;
        self.atomically(|host| {
            let item = host.resolve_attention(&item.id, answer)?;
            host.send(
                &format!("{PARENT_ANSWER_PREFIX}{}", item.id),
                Some(&caller.id),
                &item.session_id,
                &format!(
                    "Answer from your parent project \"{}\" to {}: {answer}",
                    parent.name, item.prompt
                ),
            )?;
            Ok(item)
        })
    }
    /// The parent coordinator `caller`'s `escalate_question`: moves its child's question to the
    /// human's Inbox, labelled as coming through the parent, with `note`.
    pub(crate) fn escalate_question(
        &mut self,
        caller: &Session,
        question: &str,
        note: Option<&str>,
    ) -> Result<Attention> {
        if let Some(note) = note {
            text(note, 1000)?;
        }
        let mut item = self.question_with(caller, question)?;
        item.route = QuestionRoute::Escalated {
            parent_id: caller.project_id.clone(),
            note: note
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(Into::into),
        };
        self.save_attention(&item)?;
        Ok(item)
    }
    /// The open question `id` waiting with `caller`'s project, or the refusal that says why it
    /// is not: never the caller's, already answered or already with the human.
    fn question_with(&self, caller: &Session, id: &str) -> Result<Attention> {
        let item = self.attention_item(id).ok();
        let routed_by = |item: &Attention| match &item.route {
            QuestionRoute::Parent { parent_id } | QuestionRoute::Escalated { parent_id, .. } => {
                Some(parent_id.clone())
            }
            QuestionRoute::Human => None,
        };
        let item = item
            .filter(|item| {
                is_project_coordinator(caller)
                    && routed_by(item).as_ref() == Some(&caller.project_id)
            })
            .with_context(|| {
                format!("Question {id} is not one of your child projects' questions")
            })?;
        ensure!(item.answer.is_none(), "Question {id} is already answered");
        ensure!(
            !item.route.is_with_human(),
            "Question {id} is already with the human"
        );
        Ok(item)
    }
    /// Tells the parent's coordinator, without starting its turn, that the human answered or
    /// messaged `child`, a session, with `words`; `id`, the answered item's or the message's,
    /// keeps it to one notice per contact. Best-effort: the human's answer or message is
    /// already saved, and a failed notice must not stop it from reaching the child.
    pub(crate) fn tell_parent_of_human(
        &mut self,
        child: &str,
        contact: HumanContact,
        id: &str,
        words: &str,
    ) {
        if let Err(error) = self.queue_parent_notice(child, contact, id, words) {
            eprintln!("Parent notice for {child}: {error:#}");
        }
    }
    fn queue_parent_notice(
        &mut self,
        child: &str,
        contact: HumanContact,
        id: &str,
        words: &str,
    ) -> Result<()> {
        let child = self.session(child)?;
        let Some(parent) = self.parent_coordinator(&child)? else {
            return Ok(());
        };
        let project = self.project(&child.project_id)?;
        let first_line = words.lines().map(str::trim).find(|l| !l.is_empty());
        let line: String = first_line.unwrap_or("").chars().take(300).collect();
        let what = match contact {
            HumanContact::Answered => "answered a question of",
            HumanContact::Messaged => "messaged",
        };
        // Past the cap, so a full queue does not drop it. Human clicks bound these.
        self.queue(
            &format!("{CHILD_HEARD_PREFIX}{id}"),
            None,
            &parent,
            &format!(
                "The human {what} your child project \"{}\": {line}",
                project.name
            ),
            &[],
            &[],
            Delivery::QuietPastCap,
        )?;
        Ok(())
    }
    /// `project`'s open questions waiting with its parent.
    fn questions_with_parent_of(&self, project: &str) -> Result<Vec<Attention>> {
        self.list_data(
            "SELECT data FROM attention WHERE project_id=?1 AND json_extract(data,'$.answer') IS NULL AND json_extract(data,'$.route.with')='parent' ORDER BY rowid",
            [project],
        )
    }
    /// By child project, how many of its questions wait with its parent.
    pub(crate) fn questions_with_parents(&self) -> Result<BTreeMap<String, u32>> {
        Ok(self.db.prepare("SELECT project_id,count(*) FROM attention WHERE json_extract(data,'$.answer') IS NULL AND json_extract(data,'$.route.with')='parent' GROUP BY project_id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn attention_item(&self, id: &str) -> Result<Attention> {
        Ok(self
            .db
            .query_row("SELECT data FROM attention WHERE id=?1", [id], |r| {
                decode(r, 0)
            })?)
    }
    fn save_attention(&self, item: &Attention) -> Result<()> {
        self.db.execute(
            "UPDATE attention SET data=?2 WHERE id=?1",
            params![item.id, encode(item)?],
        )?;
        Ok(())
    }
    /// Refuses to archive a project with an unarchived child, or to restore a child whose
    /// parent is archived, naming the projects to handle first. `coordinators` are the project
    /// coordinators the change archives or restores.
    pub(crate) fn check_close(&self, coordinators: &[&Session], archived: bool) -> Result<()> {
        for coordinator in coordinators {
            let project = self.project(&coordinator.project_id)?;
            if archived {
                let mut open = vec![];
                for child in self.child_projects(&project.id)? {
                    if !self.project_archived(&child.id)? {
                        open.push(format!("\"{}\"", child.name));
                    }
                }
                ensure!(
                    open.is_empty(),
                    "\"{}\" has open child projects; archive {} first",
                    project.name,
                    open.join(", ")
                );
            } else if let Some(parent) = &project.parent_id {
                let parent = self.project(parent)?;
                ensure!(
                    !self.project_archived(&parent.id)?,
                    "\"{}\" is archived; restore \"{}\" first",
                    parent.name,
                    parent.name
                );
            }
        }
        Ok(())
    }
    /// A project's `workspace_context` entries for its parent and its children.
    pub(crate) fn family_context(&self, session: &Session, context: &mut Value) -> Result<()> {
        let project = self.project(&session.project_id)?;
        if let Some(parent) = &project.parent_id {
            let parent = self.project(parent)?;
            context["parent_project"] = json!({"id":parent.id,"name":parent.name,
                "coordinator_id":self.coordinator_of(&parent.id)?});
        }
        let children = self.child_projects(&project.id)?;
        if children.is_empty() {
            return Ok(());
        }
        let repositories = self.repositories()?;
        let tickets = self.shown_tickets()?;
        let mut entries = vec![];
        for child in children {
            let coordinator = self.coordinator_of(&child.id)?;
            let tickets: Vec<Value> = (tickets.iter())
                .filter(|t| self.ticket_in_project(t, &child.id))
                .map(|t| Self::short_ticket(t, &repositories))
                .collect();
            entries.push(
                json!({"id":child.id,"name":child.name,"coordinator_id":coordinator,
                "standing":child.standing,"tickets":tickets}),
            );
        }
        context["child_projects"] = Value::Array(entries);
        Ok(())
    }
}

/// How the human reached a child project's coordinator, for its parent's notice.
pub(crate) enum HumanContact {
    Answered,
    Messaged,
}

/// A project's own coordinator, which project links connect, rather than one of its workers.
fn is_project_coordinator(session: &Session) -> bool {
    session.role == Role::ProjectOrchestrator && session.parent_id.is_none()
}
