//! Parent projects: a project may be filed under one loose project of its Space, one level
//! deep. The link decides which coordinators may message each other, where a child
//! coordinator's reports go and when a project may be archived or restored.
use crate::*;

const MOVED_UNDER: &str = "project_moved_under";
const MOVED_OUT: &str = "project_moved_out";

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
    /// Makes `project` loose, if it has a parent, and records it on both.
    pub(crate) fn leave_parent(&self, project: &mut Project) -> Result<()> {
        let Some(old) = project.parent_id.take() else {
            return Ok(());
        };
        self.save_project(project)?;
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
        let agents = "your own agents";
        Ok(match whom.is_empty() {
            true => agents.into(),
            false => format!("{} and {agents}", whom.join(", ")),
        })
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
                .filter(|t| {
                    self.session(&t.coordinator_id)
                        .is_ok_and(|s| s.project_id == child.id)
                })
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

/// A project's own coordinator, which project links connect, rather than one of its workers.
fn is_project_coordinator(session: &Session) -> bool {
    session.role == Role::ProjectOrchestrator && session.parent_id.is_none()
}
