//! The manual order of projects (store schema 7): one flat list, the same for every client.
use crate::*;
use std::collections::BTreeSet;

/// Adds each project's position, smallest first. Existing projects keep the newest-first order
/// they were listed in until something is moved.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    let present: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name='position'",
        [],
        |r| r.get(0),
    )?;
    if present == 0 {
        let tx = db.unchecked_transaction()?;
        tx.execute_batch(
            "ALTER TABLE projects ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
             UPDATE projects SET position = -rowid;",
        )?;
        tx.commit()?;
    }
    db.execute_batch("PRAGMA user_version = 7;")?;
    Ok(())
}

/// The position that puts a new project above every other.
pub(crate) fn top(db: &Connection) -> Result<i64> {
    Ok(db.query_row(
        "SELECT COALESCE(MIN(position),0)-1 FROM projects",
        [],
        |r| r.get(0),
    )?)
}

impl Host {
    /// Moves the projects, kept in their relative order, to just before `before`, or to the end
    /// of the order, and returns every project in the new order.
    pub fn move_projects(&mut self, ids: &[String], before: Option<&str>) -> Result<Vec<Project>> {
        let order: Vec<String> = self.projects()?.into_iter().map(|p| p.id).collect();
        let moving: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        ensure!(!ids.is_empty(), "Name at least one project to move");
        ensure!(moving.len() == ids.len(), "A project is listed twice");
        ensure!(
            moving.iter().all(|id| order.iter().any(|p| p == id)),
            "Project not found"
        );
        let (block, mut rest): (Vec<_>, Vec<_>) = order
            .iter()
            .map(String::as_str)
            .partition(|id| moving.contains(id));
        let at = match before {
            Some(before) => {
                ensure!(
                    !moving.contains(before),
                    "A project cannot move before itself"
                );
                rest.iter()
                    .position(|id| *id == before)
                    .context("Project not found")?
            }
            None => rest.len(),
        };
        rest.splice(at..at, block);
        let tx = self.db.transaction()?;
        for (position, id) in rest.iter().enumerate() {
            tx.execute(
                "UPDATE projects SET position=?2 WHERE id=?1",
                params![id, position as i64],
            )?;
        }
        tx.commit()?;
        self.projects()
    }
}
