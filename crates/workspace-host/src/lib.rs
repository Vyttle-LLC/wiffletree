//! Single-writer durable host, independent of graphics and provider inference.
pub mod attachments;
pub mod context;
mod flatten;
pub mod live;
pub mod mcp;
mod memory;
mod overlaps;
mod policies;
pub mod provider;
mod repositories;
pub mod runtime;
pub(crate) mod schedules;
pub use schedules::TimerPass;
pub mod service;
mod settings;
mod steps;
mod stream;
mod telemetry;
pub mod usage;
mod verification;
mod worktrees;
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use workspace_core::*;

pub struct Host {
    pub(crate) db: Connection,
    _lock: StoreLock,
    pub home: PathBuf,
    /// When each timer whose last fire failed may try again; see `fire_due_schedules`.
    fire_retries: std::collections::HashMap<String, i64>,
    /// Each repository's open-branch warnings from its latest check; see `overlaps`.
    branch_warnings: std::collections::HashMap<String, Vec<BranchWarnings>>,
    /// When this host last fetched each repository's base.
    base_fetched_at: std::collections::HashMap<String, i64>,
}
struct StoreLock(File);
impl Drop for StoreLock {
    fn drop(&mut self) {
        // A concurrent fork can briefly inherit the descriptor before exec closes it.
        // Release ownership explicitly after the database has been dropped.
        let _ = self.0.unlock();
    }
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
pub(crate) fn decode<T: DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn tag<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .context("Expected enum tag")?
        .to_owned())
}
/// Prefix of the inbox items the host raises for a long turn; agents and clients cannot use it.
pub(crate) const CHECK_IN: &str = "check-in:";
fn ensure_unreserved(operation: &str) -> Result<()> {
    ensure!(
        !operation.starts_with(CHECK_IN),
        "IDs starting with \"{CHECK_IN}\" are reserved for workspace check-ins"
    );
    Ok(())
}
/// How a queued message reaches its recipient.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// Starts the recipient's next turn.
    Turn,
    /// Rides along with the next turn without starting one.
    Quiet,
    /// Starts a turn and is never refused for a full queue; see `send_human_notice`.
    HumanNotice,
}
pub(crate) fn text(value: &str, maximum: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= maximum,
        "Text must contain 1–{maximum} bytes"
    );
    Ok(())
}
pub fn page_limit(limit: usize) -> Result<i64> {
    ensure!(
        limit > 0 && limit <= MAX_PAGE,
        "Page limit must be 1–{MAX_PAGE}"
    );
    Ok(limit as i64)
}

impl Host {
    pub fn open(home: impl AsRef<Path>) -> Result<Self> {
        let home = home.as_ref();
        fs::create_dir_all(home)?;
        let home = fs::canonicalize(home)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(home.join("host.lock"))?;
        lock.try_lock()
            .context("Another host already owns this store")?;
        let lock = StoreLock(lock);
        let db = Connection::open(home.join("workspace.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        )?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version <= 6, "Store schema is newer than this application");
        if version == 0 {
            db.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("schema.sql")
            ))?;
        }
        db.execute_batch(include_str!("live.sql"))?;
        db.execute_batch(include_str!("usage.sql"))?;
        db.execute_batch(include_str!("steps.sql"))?;
        db.execute_batch(include_str!("schedules.sql"))?;
        db.execute_batch(include_str!("selection.sql"))?;
        repositories::migrate_to_workspace(&db)?;
        attachments::migrate(&db)?;
        worktrees::migrate(&db)?;
        flatten::migrate(&home, &db, version)?;
        let mut host = Self {
            db,
            _lock: lock,
            home: home.to_path_buf(),
            fire_retries: Default::default(),
            branch_warnings: Default::default(),
            base_fetched_at: Default::default(),
        };
        host.recover()?;
        host.migrate_opus_defaults()?;
        host.migrate_model_selection()?;
        host.backfill_usage()?;
        Ok(host)
    }
    fn recover(&mut self) -> Result<()> {
        let sessions = self.sessions()?;
        let tx = self.db.transaction()?;
        for mut session in sessions {
            if session.status == Status::Working {
                session.status = Status::Disconnected;
                tx.execute(
                    "UPDATE sessions SET data=?2 WHERE id=?1",
                    params![session.id, encode(&session)?],
                )?;
                Self::event(
                    &tx,
                    &session.project_id,
                    Some(&session.id),
                    "recovery",
                    "Runtime disconnected; reconcile before retry",
                )?;
            }
        }
        tx.execute(
            "UPDATE messages SET receipt='held' WHERE receipt='delivered'",
            [],
        )?;
        tx.commit()?;
        self.recover_steps()
    }
    pub(crate) fn event(
        db: &Connection,
        project: &str,
        session: Option<&str>,
        kind: &str,
        detail: &str,
    ) -> Result<()> {
        db.execute("INSERT INTO activity(project_id,session_id,kind,detail,created_at) VALUES (?1,?2,?3,?4,?5)", params![project, session, kind, detail, now()])?;
        Ok(())
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.list_data("SELECT data FROM projects ORDER BY rowid", [])
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.list_data("SELECT data FROM sessions ORDER BY rowid", [])
    }
    pub fn repositories(&self) -> Result<Vec<Repository>> {
        self.list_data("SELECT data FROM repositories ORDER BY rowid", [])
    }
    fn list_data<T: DeserializeOwned, P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<T>> {
        Ok(self
            .db
            .prepare(sql)?
            .query_map(params, |row| decode(row, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn project(&self, id: &str) -> Result<Project> {
        self.db
            .query_row("SELECT data FROM projects WHERE id=?1", [id], |row| {
                decode(row, 0)
            })
            .context("Project not found")
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        self.db
            .query_row("SELECT data FROM sessions WHERE id=?1", [id], |row| {
                decode(row, 0)
            })
            .context("Session not found")
    }
    pub fn repository(&self, id: &str) -> Result<Repository> {
        self.db
            .query_row("SELECT data FROM repositories WHERE id=?1", [id], |row| {
                decode(row, 0)
            })
            .context("Repository not found")
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot { projects: self.projects()?, repositories: self.repositories()?, sessions: self.sessions()?, attention: self.list_data("SELECT data FROM attention WHERE json_extract(data,'$.answer') IS NULL ORDER BY rowid", [])?, tickets: self.tickets()?, runtimes: self.runtimes()?, live_projects: self.live_projects()?, repository_roots: self.repository_roots()?, missing_repositories: self.missing_repositories()?, schedules: self.active_schedules(None)?, model_selection: self.model_selection()?, undelivered: self.undelivered()?, branch_warnings: self.open_branch_warnings()? })
    }
    fn undelivered(&self) -> Result<Vec<UndeliveredInput>> {
        Ok(self.db.prepare("SELECT recipient,SUM(receipt='held'),SUM(receipt='queued' AND quiet=0) FROM messages WHERE receipt IN ('held','queued') GROUP BY recipient")?.query_map([], |r| Ok(UndeliveredInput { session_id: r.get(0)?, held: r.get(1)?, queued: r.get(2)? }))?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn create_project(&mut self, name: &str) -> Result<Project> {
        text(name, 128)?;
        let project = Project {
            id: new_id(),
            name: name.trim().into(),
            brain: None,
            turn_limit: 4,
            repositories: None,
            home: Some(self.new_project_home(name)?),
            slug: None,
        };
        let prefill = self
            .model_selection()?
            .prefill(Role::ProjectOrchestrator)
            .context("Enable a provider in Models first")?;
        let session = Session {
            id: new_id(),
            project_id: project.id.clone(),
            parent_id: None,
            repository_id: None,
            name: project.name.clone(),
            role: Role::ProjectOrchestrator,
            provider: prefill.provider,
            status: Status::Ready,
            archived: false,
        };
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO projects VALUES (?1,?2)",
            params![project.id, encode(&project)?],
        )?;
        Self::insert_session(&tx, &session)?;
        Self::event(
            &tx,
            &project.id,
            Some(&session.id),
            "project_created",
            &project.name,
        )?;
        tx.commit()?;
        self.pin(&session.id, prefill, Chooser::Default, "Role default")?;
        Ok(project)
    }
    pub fn rename_project(&mut self, id: &str, name: &str) -> Result<Project> {
        text(name, 128)?;
        let mut project = self.project(id)?;
        let mut root: Session = self
            .db
            .query_row(
                "SELECT data FROM sessions WHERE project_id=?1 AND parent_id IS NULL AND role=?2",
                params![id, tag(&Role::ProjectOrchestrator)?],
                |row| decode(row, 0),
            )
            .context("Project coordinator not found")?;
        project.name = name.trim().into();
        root.name = project.name.clone();
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE projects SET data=?2 WHERE id=?1",
            params![id, encode(&project)?],
        )?;
        tx.execute(
            "UPDATE sessions SET data=?2 WHERE id=?1",
            params![root.id, encode(&root)?],
        )?;
        Self::event(&tx, id, Some(&root.id), "project_renamed", &project.name)?;
        tx.commit()?;
        Ok(project)
    }
    pub fn create_session(
        &mut self,
        project: &str,
        parent: &str,
        repository: Option<&str>,
        name: &str,
        role: Role,
        provider: Provider,
    ) -> Result<Session> {
        text(name, 128)?;
        self.project(project)?;
        let owner = self.session(parent)?;
        ensure!(
            owner.project_id == project,
            "Parent belongs to another project"
        );
        ensure!(
            !owner.archived,
            "{} is archived; restore it before adding to it",
            owner.name
        );
        match role {
            Role::ProjectOrchestrator => {
                bail!("Project coordinators are created with their project")
            }
            Role::TaskOrchestrator => {
                bail!("Repository coordinators were removed; create a ticket for the repository")
            }
            _ => ensure!(
                owner.role == Role::ProjectOrchestrator,
                "Agents belong to their project's coordinator"
            ),
        }
        if let Some(repository) = repository {
            self.repository(repository)?;
            ensure!(
                self.project(project)?.uses(repository),
                "This project does not use that repository"
            );
        }
        let repository_id = repository.map(str::to_owned);
        let session = Session {
            id: new_id(),
            project_id: project.into(),
            parent_id: Some(parent.into()),
            repository_id,
            name: name.trim().into(),
            role,
            provider,
            status: Status::Ready,
            archived: false,
        };
        // A savepoint, so assigning a ticket agent can include this in its own change.
        let tx = self.db.savepoint()?;
        Self::insert_session(&tx, &session)?;
        Self::event(
            &tx,
            project,
            Some(&session.id),
            "session_created",
            &session.name,
        )?;
        tx.commit()?;
        Ok(session)
    }
    fn insert_session(db: &Connection, session: &Session) -> Result<()> {
        db.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,?4,?5)",
            params![
                session.id,
                session.project_id,
                session.parent_id,
                tag(&session.role)?,
                encode(session)?
            ],
        )?;
        Ok(())
    }
    /// The session and every session beneath it.
    pub fn session_tree(&self, id: &str) -> Result<Vec<Session>> {
        let all = self.sessions()?;
        let mut tree = vec![self.session(id)?];
        let mut next = 0;
        while next < tree.len() {
            let parent = tree[next].id.clone();
            tree.extend(
                all.iter()
                    .filter(|s| s.parent_id.as_deref() == Some(&parent))
                    .cloned(),
            );
            next += 1;
        }
        Ok(tree)
    }
    /// Archiving covers the session's whole tree and stops a project when its main coordinator is
    /// archived. Restoring also restores the owners above it, so the session is reachable again.
    /// Messages, tickets, branches and provider conversations are kept either way. Archiving
    /// removes the worktrees of tickets the tree owns; restoring re-creates those of open tickets.
    pub fn set_archived(&mut self, id: &str, archived: bool) -> Result<Vec<Session>> {
        self.change_archived(id, archived, None)
    }
    /// Like set_archived, also recording `milestone` for the session itself in the same
    /// transaction when its flag changes, so the event and the transition are never split.
    pub(crate) fn change_archived(
        &mut self,
        id: &str,
        archived: bool,
        milestone: Option<&str>,
    ) -> Result<Vec<Session>> {
        let mut affected = self.session_tree(id)?;
        ensure!(
            archived || affected[0].role != Role::TaskOrchestrator,
            "{} is a retired repository coordinator; its conversation stays read-only",
            affected[0].name
        );
        if !archived {
            let mut owner = affected[0].parent_id.clone();
            while let Some(parent) = owner {
                let session = self.session(&parent)?;
                owner = session.parent_id.clone();
                affected.push(session);
            }
        }
        let owners: Vec<&str> = affected.iter().map(|s| s.id.as_str()).collect();
        let tickets: Vec<Ticket> = self
            .tickets()?
            .into_iter()
            .filter(|t| owners.contains(&t.coordinator_id.as_str()) && (archived || t.is_open()))
            .collect();
        let restored: Vec<String> = match archived {
            true => vec![],
            false => tickets.iter().map(|t| t.id.clone()).collect(),
        };
        let save = move |host: &mut Self| -> Result<Vec<Session>> {
            let project = affected[0].project_id.clone();
            let stops_project = archived && affected[0].role == Role::ProjectOrchestrator;
            // A savepoint, so a ticket close or accept can include this in its own change.
            let tx = host.db.savepoint()?;
            for session in &mut affected {
                if session.archived == archived {
                    continue;
                }
                session.archived = archived;
                if archived && session.status == Status::Working {
                    session.status = Status::Ready;
                }
                tx.execute(
                    "UPDATE sessions SET data=?2 WHERE id=?1",
                    params![session.id, encode(&*session)?],
                )?;
                Self::event(
                    &tx,
                    &project,
                    Some(&session.id),
                    if archived { "archived" } else { "restored" },
                    &session.name,
                )?;
                if let Some(kind) = milestone.filter(|_| session.id == id) {
                    Self::event(&tx, &project, Some(id), kind, &session.name)?;
                }
            }
            if archived {
                let sessions = affected.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
                Self::stop_schedules_in(&tx, &sessions, None, "archived")?;
            }
            if stops_project {
                tx.execute(
                    "UPDATE live_projects SET enabled=0 WHERE project_id=?1",
                    [&project],
                )?;
            }
            // Restoring cancels the restored tickets' pending worktree removals.
            for ticket in &restored {
                tx.execute(
                    "DELETE FROM pending_worktree_removals WHERE ticket_id=?1",
                    [ticket],
                )?;
            }
            tx.commit()?;
            Ok(affected)
        };
        if archived {
            self.with_worktrees_removed(&tickets, save)
        } else {
            self.restore_worktrees(&tickets)?;
            save(self)
        }
    }
    /// Runs `work` as one unit: if it fails, none of its writes remain. Helpers called inside
    /// must use savepoints rather than transactions.
    pub(crate) fn atomically<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.db.execute_batch("SAVEPOINT atomically")?;
        match work(self) {
            Ok(value) => {
                self.db.execute_batch("RELEASE atomically")?;
                Ok(value)
            }
            Err(error) => {
                self.db
                    .execute_batch("ROLLBACK TO atomically; RELEASE atomically")?;
                Err(error)
            }
        }
    }
    pub fn set_status(&mut self, id: &str, status: Status) -> Result<()> {
        let mut session = self.session(id)?;
        session.status = status;
        let tx = self.db.savepoint()?;
        tx.execute(
            "UPDATE sessions SET data=?2 WHERE id=?1",
            params![id, encode(&session)?],
        )?;
        Self::event(&tx, &session.project_id, Some(id), "status", status.label())?;
        tx.commit()?;
        Ok(())
    }
    fn message_row(row: &Row<'_>) -> rusqlite::Result<Message> {
        let receipt: String = row.get(6)?;
        let receipt = serde_json::from_value(json!(receipt)).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(Message {
            sequence: row.get(0)?,
            id: row.get(1)?,
            project_id: row.get(2)?,
            sender: row.get(3)?,
            recipient: row.get(4)?,
            body: row.get(5)?,
            receipt,
            created_at: row.get(7)?,
            // By name: columns added by migrations can arrive in either order.
            attachments: decode(row, row.as_ref().column_index("attachments")?)?,
        })
    }
    pub fn message(&self, id: &str) -> Result<Message> {
        Ok(self.db.query_row(
            "SELECT * FROM messages WHERE id=?1",
            [id],
            Self::message_row,
        )?)
    }
    pub fn send(
        &mut self,
        id: &str,
        sender: Option<&str>,
        recipient: &str,
        body: &str,
    ) -> Result<Message> {
        self.send_with_files(id, sender, recipient, body, &[])
    }
    /// Sends a message whose `files` are copied into the store with it. Only the human
    /// attaches files, and with them the body may be empty.
    pub fn send_with_files(
        &mut self,
        id: &str,
        sender: Option<&str>,
        recipient: &str,
        body: &str,
        files: &[PathBuf],
    ) -> Result<Message> {
        self.queue(id, sender, recipient, body, files, Delivery::Turn)
    }
    /// Queues a message that rides along with the recipient's next turn without starting one.
    pub(crate) fn send_quietly(
        &mut self,
        id: &str,
        sender: Option<&str>,
        recipient: &str,
        body: &str,
    ) -> Result<Message> {
        self.queue(id, sender, recipient, body, &[], Delivery::Quiet)
    }
    /// Queues a notice of the human's own action past the recipient cap: human clicks bound
    /// these, and losing one would hide an emergency stop from the parent.
    pub(crate) fn send_human_notice(
        &mut self,
        id: &str,
        sender: Option<&str>,
        recipient: &str,
        body: &str,
    ) -> Result<Message> {
        self.queue(id, sender, recipient, body, &[], Delivery::HumanNotice)
    }
    fn queue(
        &mut self,
        id: &str,
        sender: Option<&str>,
        recipient: &str,
        body: &str,
        files: &[PathBuf],
        delivery: Delivery,
    ) -> Result<Message> {
        let quiet = delivery == Delivery::Quiet;
        text(id, 128)?;
        if files.is_empty() {
            text(body, MAX_TEXT_BYTES)?;
        } else {
            ensure!(sender.is_none(), "Only the human can attach files");
            ensure!(
                body.len() <= MAX_TEXT_BYTES,
                "Text must contain at most {MAX_TEXT_BYTES} bytes"
            );
        }
        let target = self.session(recipient)?;
        ensure!(
            !target.archived,
            "{} is archived; restore it before sending",
            target.name
        );
        if let Some(sender) = sender {
            ensure!(
                self.session(sender)?.project_id == target.project_id,
                "Cross-project message forbidden"
            );
        }
        if let Some(existing) = self
            .db
            .query_row(
                "SELECT * FROM messages WHERE id=?1",
                [id],
                Self::message_row,
            )
            .optional()?
        {
            let names = files.iter().map(|f| f.file_name()).collect::<Vec<_>>();
            ensure!(
                existing.sender.as_deref() == sender
                    && existing.recipient == recipient
                    && existing.body == body
                    && existing
                        .attachments
                        .iter()
                        .map(|a| a.path.file_name())
                        .eq(names),
                "Message ID reused with a different payload"
            );
            return Ok(existing);
        }
        let queued: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM messages WHERE recipient=?1 AND receipt IN ('queued','held')",
            [recipient],
            |r| r.get(0),
        )?;
        ensure!(
            queued < 1024 || delivery == Delivery::HumanNotice,
            "Recipient queue is full; accepted messages are preserved"
        );
        if files.is_empty() {
            self.insert_message(id, &target, sender, body, &[], quiet)?;
        } else {
            let directory = attachments::directory(&self.home, &target.project_id, id)?;
            let stored = attachments::store(&directory, files)?;
            if let Err(error) = self.insert_message(id, &target, sender, body, &stored, quiet) {
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
        }
        self.message(id)
    }
    fn insert_message(
        &mut self,
        id: &str,
        target: &Session,
        sender: Option<&str>,
        body: &str,
        attachments: &[Attachment],
        quiet: bool,
    ) -> Result<()> {
        let recipient = target.id.as_str();
        let tx = self.db.savepoint()?;
        tx.execute("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at,attachments,quiet) VALUES (?1,?2,?3,?4,?5,'queued',?6,?7,?8)", params![id, target.project_id, sender, recipient, body, now(), encode(&attachments)?, quiet])?;
        Self::event(
            &tx,
            &target.project_id,
            Some(recipient),
            "message_queued",
            id,
        )?;
        if sender.is_none()
            && let Some(parent) = target.parent_id.as_deref()
        {
            Self::event(
                &tx,
                &target.project_id,
                Some(parent),
                "direct_instruction",
                &format!("Instruction {id} queued for {}", target.name),
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn messages(
        &self,
        session: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        self.session(session)?;
        let mut rows = self.db.prepare("SELECT * FROM messages WHERE recipient=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")?.query_map(params![session, before.unwrap_or(i64::MAX), page_limit(limit)?], Self::message_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }
    pub fn advance_receipt(&mut self, id: &str, next: Receipt) -> Result<Message> {
        let current = self.message(id)?;
        if current.receipt == next {
            return Ok(current);
        }
        ensure!(
            matches!(
                (current.receipt, next),
                (Receipt::Queued, Receipt::Delivered)
                    | (Receipt::Delivered, Receipt::Acknowledged)
                    | (Receipt::Acknowledged, Receipt::Completed)
            ),
            "Invalid delivery transition; held messages require reconciliation"
        );
        if next == Receipt::Delivered {
            let earliest: i64 = self.db.query_row("SELECT MIN(sequence) FROM messages WHERE recipient=?1 AND receipt IN ('queued','held')", [&current.recipient], |r| r.get(0))?;
            ensure!(
                earliest == current.sequence,
                "Delivery must preserve recipient order"
            );
        }
        let tx = self.db.savepoint()?;
        tx.execute(
            "UPDATE messages SET receipt=?2 WHERE id=?1",
            params![id, tag(&next)?],
        )?;
        Self::event(
            &tx,
            &current.project_id,
            Some(&current.recipient),
            "message_receipt",
            &format!("{id}: {next:?}"),
        )?;
        tx.commit()?;
        self.message(id)
    }
    /// Explicit local adapter. The transaction models receipts without claiming provider execution.
    pub fn simulate(&mut self, id: &str) -> Result<Message> {
        let message = self.message(id)?;
        ensure!(
            message.sender.is_none(),
            "Simulator accepts human input only"
        );
        let reply_id = format!("sim:{id}");
        if message.receipt == Receipt::Completed {
            return self.message(&reply_id);
        }
        ensure!(
            message.receipt == Receipt::Queued,
            "Uncertain message requires reconciliation"
        );
        let session = self.session(&message.recipient)?;
        ensure!(
            !matches!(session.status, Status::Paused | Status::Disconnected),
            "Session must be ready before delivery"
        );
        let earliest: i64 = self.db.query_row("SELECT MIN(sequence) FROM messages WHERE recipient=?1 AND receipt IN ('queued','held')", [&message.recipient], |r| r.get(0))?;
        ensure!(
            earliest == message.sequence,
            "Earlier queued input must be delivered first"
        );
        let profile = self.turn_profile(&session)?;
        let body = format!(
            "Local simulation · {}\n\nInstruction recorded durably for {}. Selected {profile}.\n\nNo provider turn or repository change was performed. Configure providers, roles and the guide in Models; capture decisions in Memory for replacement sessions.",
            session.role.label(),
            session.name,
        );
        let tx = self.db.transaction()?;
        for receipt in [
            Receipt::Delivered,
            Receipt::Acknowledged,
            Receipt::Completed,
        ] {
            Self::event(
                &tx,
                &session.project_id,
                Some(&session.id),
                "simulation_receipt",
                &format!("{id}: {receipt:?}"),
            )?;
        }
        tx.execute("UPDATE messages SET receipt='completed' WHERE id=?1", [id])?;
        tx.execute("INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?3,?4,'completed',?5)", params![reply_id, session.project_id, session.id, body, now()])?;
        tx.commit()?;
        self.message(&reply_id)
    }
    pub fn request_attention(
        &mut self,
        session: &str,
        host: &str,
        operation: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<Attention> {
        text(host, 128)?;
        text(operation, 128)?;
        text(prompt, 4096)?;
        ensure!(options.len() <= 6, "Offer at most 6 options");
        for option in options {
            text(option, 200)?;
        }
        let session = self.session(session)?;
        if let Some(existing) = self
            .db
            .query_row(
                "SELECT data FROM attention WHERE session_id=?1 AND operation_id=?2",
                params![session.id, operation],
                |r| decode::<Attention>(r, 0),
            )
            .optional()?
        {
            ensure!(
                existing.host == host && existing.prompt == prompt && existing.options == options,
                "Operation identity reused with changed approval details"
            );
            return Ok(existing);
        }
        let attention = Attention {
            id: new_id(),
            project_id: session.project_id.clone(),
            session_id: session.id.clone(),
            host: host.into(),
            operation_id: operation.into(),
            prompt: prompt.into(),
            options: options.to_vec(),
            answer: None,
        };
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO attention VALUES (?1,?2,?3,?4,?5)",
            params![
                attention.id,
                session.project_id,
                session.id,
                operation,
                encode(&attention)?
            ],
        )?;
        Self::event(
            &tx,
            &session.project_id,
            Some(&session.id),
            "attention_requested",
            operation,
        )?;
        tx.commit()?;
        Ok(attention)
    }
    pub fn resolve_attention(&mut self, id: &str, answer: &str) -> Result<Attention> {
        text(answer, 4096)?;
        let mut attention: Attention =
            self.db
                .query_row("SELECT data FROM attention WHERE id=?1", [id], |r| {
                    decode(r, 0)
                })?;
        ensure!(
            attention.answer.is_none(),
            "Request already resolved; stale answer rejected"
        );
        attention.answer = Some(answer.into());
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE attention SET data=?2 WHERE id=?1",
            params![id, encode(&attention)?],
        )?;
        Self::event(
            &tx,
            &attention.project_id,
            Some(&attention.session_id),
            "attention_resolved",
            &attention.operation_id,
        )?;
        tx.commit()?;
        Ok(attention)
    }
    /// Unanswered inbox items across a project's sessions, oldest first.
    pub fn open_attention(&self, project: &str) -> Result<Vec<Attention>> {
        self.list_data(
            "SELECT data FROM attention WHERE project_id=?1 AND json_extract(data,'$.answer') IS NULL ORDER BY rowid",
            [project],
        )
    }
    pub fn dismiss_attention(&mut self, id: &str) -> Result<Attention> {
        let attention: Attention =
            self.db
                .query_row("SELECT data FROM attention WHERE id=?1", [id], |r| {
                    decode(r, 0)
                })?;
        ensure!(
            !attention.is_permission(),
            "Approve or deny a permission request"
        );
        self.resolve_attention(id, "Dismissed by the human")
    }
    pub fn activity(
        &self,
        project: &str,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Activity>> {
        self.project(project)?;
        Ok(self.db.prepare("SELECT * FROM activity WHERE project_id=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")?.query_map(params![project, before.unwrap_or(i64::MAX), page_limit(limit)?], |r| Ok(Activity { sequence: r.get(0)?, project_id: r.get(1)?, session_id: r.get(2)?, kind: r.get(3)?, detail: r.get(4)?, created_at: r.get(5)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn git_history(&self, repository: &str, skip: usize, limit: usize) -> Result<Value> {
        page_limit(limit)?;
        ensure!(skip <= 100_000, "History cursor too large");
        let repo = self.repository(repository)?;
        let output = runtime::git_output(
            Path::new(&repo.path),
            &[
                "log",
                "--date-order",
                &format!("--max-count={limit}"),
                &format!("--skip={skip}"),
                "--format=%H%x1f%P%x1f%an%x1f%s",
                "HEAD",
                "--",
            ],
        )?;
        let commits: Vec<_> = output.lines().filter_map(|line| { let parts: Vec<_> = line.splitn(4, '\u{1f}').collect(); (parts.len() == 4).then(|| json!({"sha":parts[0],"parents":parts[1].split_whitespace().collect::<Vec<_>>(),"author":parts[2],"subject":parts[3]})) }).collect();
        Ok(
            json!({"repository":repo,"commits":commits,"next_skip":skip+commits.len(),"observed_at":now(),"source":"local Git · read only"}),
        )
    }
    pub fn respond(&mut self, request: Request) -> Response {
        let result = if request.version != PROTOCOL_VERSION {
            Err(anyhow::anyhow!("Unsupported protocol version"))
        } else {
            self.execute(request.command)
        };
        match result {
            Ok(result) => Response {
                version: PROTOCOL_VERSION,
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => Response {
                version: PROTOCOL_VERSION,
                id: request.id,
                result: None,
                error: Some(format!("{error:#}")),
            },
        }
    }
    pub fn execute(&mut self, command: Command) -> Result<Value> {
        Ok(match command {
            Command::SetLive {
                project_id,
                enabled,
            } => {
                self.set_live(&project_id, enabled)?;
                json!({"enabled":enabled})
            }
            Command::ConfigureSession {
                session_id,
                profile,
            } => {
                self.configure_session(&session_id, profile)?;
                json!({"saved":true})
            }
            Command::ReconcileSession { session_id, retry } => {
                self.reconcile_session(&session_id, retry)?;
                json!({"saved":true})
            }
            Command::CreateTicket {
                coordinator_id,
                repository_id,
                title,
                brief,
            } => serde_json::to_value(self.create_ticket(
                &coordinator_id,
                &repository_id,
                &title,
                &brief,
            )?)?,
            Command::AssignTicket {
                ticket_id,
                role,
                profile,
                instruction,
                focus,
            } => {
                let session = self.assign_ticket(
                    &ticket_id,
                    role,
                    profile.provider,
                    &instruction,
                    focus.as_deref(),
                )?;
                // A pinned agent keeps its model; a new or unpinned one takes the human's pick.
                if self.session_runtime(&session.id)?.profile.is_none() {
                    self.pin_human_choice(&session, profile)?;
                }
                serde_json::to_value(session)?
            }
            Command::ProviderCheck => provider::check(),
            Command::Quotas => serde_json::to_value(self.quotas()?)?,
            Command::RecordQuota { reading } => {
                self.record_quota(reading)?;
                serde_json::to_value(self.quotas()?)?
            }
            Command::Usage {
                days,
                project_id,
                provider,
                timezone,
            } => serde_json::to_value(self.usage_report(
                days,
                project_id.as_deref(),
                provider,
                &timezone,
            )?)?,
            Command::Snapshot => serde_json::to_value(self.snapshot()?)?,
            Command::CreateProject { name } => serde_json::to_value(self.create_project(&name)?)?,
            Command::RenameProject { project_id, name } => {
                serde_json::to_value(self.rename_project(&project_id, &name)?)?
            }
            Command::AttachRepository {
                project_id,
                path,
                base,
            } => serde_json::to_value(match project_id {
                Some(project) => self.attach_repository(&project, &path, &base)?,
                None => self.add_repository(&path, &base)?,
            })?,
            Command::DiscoverRepositories { folders } => {
                serde_json::to_value(self.discover_repositories(&folders)?)?
            }
            Command::ImportRepositories {
                project_id,
                paths,
                roots,
                dismissed,
            } => serde_json::to_value(self.import_repositories(
                project_id.as_deref(),
                &paths,
                &roots,
                &dismissed,
            )?)?,
            Command::RefreshRepositories => serde_json::to_value(self.refresh_repositories()?)?,
            Command::RemoveRepositoryRoot { path } => {
                self.remove_repository_root(&path)?;
                Value::Null
            }
            Command::SetProjectRepositories {
                project_id,
                repository_ids,
            } => serde_json::to_value(self.set_project_repositories(&project_id, repository_ids)?)?,
            Command::RemoveRepository { repository_id } => {
                self.remove_repository(&repository_id)?;
                Value::Null
            }
            Command::CreateSession {
                project_id,
                parent_id,
                repository_id,
                name,
                role,
                profile,
            } => {
                let session = self.create_session(
                    &project_id,
                    &parent_id,
                    repository_id.as_deref(),
                    &name,
                    role,
                    profile.provider,
                )?;
                self.pin_human_choice(&session, profile)?;
                serde_json::to_value(session)?
            }
            Command::SetStatus { session_id, status } => {
                if status != Status::Paused {
                    self.clear_stop(&session_id)?;
                }
                self.set_status(&session_id, status)?;
                json!({"saved":true})
            }
            Command::SetArchived {
                session_id,
                archived,
            } => serde_json::to_value(self.set_archived(&session_id, archived)?)?,
            Command::Send {
                id,
                sender,
                recipient,
                body,
                attachments,
            } => serde_json::to_value(self.send_with_files(
                &id,
                sender.as_deref(),
                &recipient,
                &body,
                &attachments,
            )?)?,
            Command::Messages {
                session_id,
                before,
                limit,
            } => serde_json::to_value(self.messages(&session_id, before, limit)?)?,
            Command::Simulate { message_id } => serde_json::to_value(self.simulate(&message_id)?)?,
            Command::SetModelSelection { selection } => {
                serde_json::to_value(self.set_model_selection(&selection)?)?
            }
            Command::RequestAttention {
                session_id,
                host,
                operation_id,
                prompt,
            } => serde_json::to_value({
                ensure_unreserved(&operation_id)?;
                self.request_attention(&session_id, &host, &operation_id, &prompt, &[])?
            })?,
            Command::ResolveAttention { id, answer } => {
                serde_json::to_value(self.resolve_attention(&id, &answer)?)?
            }
            Command::DismissAttention { id } => serde_json::to_value(self.dismiss_attention(&id)?)?,
            Command::AppendLog { input } => serde_json::to_value(self.append_log(input)?)?,
            Command::Logs {
                project_id,
                before,
                limit,
            } => serde_json::to_value(self.logs(&project_id, before, limit)?)?,
            Command::AttachBrain { project_id, path } => {
                self.attach_brain(&project_id, &path)?;
                json!({"saved":true})
            }
            Command::ExportLogs { project_id } => {
                serde_json::to_value(self.export_logs(&project_id)?)?
            }
            Command::Runs { project_id } => serde_json::to_value(self.runs(&project_id)?)?,
            Command::Activity {
                project_id,
                before,
                limit,
            } => serde_json::to_value(self.activity(&project_id, before, limit)?)?,
            Command::GitHistory {
                repository_id,
                skip,
                limit,
            } => self.git_history(&repository_id, skip, limit)?,
            Command::Steps {
                session_id,
                run_id,
                after,
            } => serde_json::to_value(self.steps(&session_id, run_id.as_deref(), after)?)?,
            Command::RunSummaries {
                session_id,
                run_ids,
            } => serde_json::to_value(self.run_summaries(&session_id, &run_ids)?)?,
            Command::Settings => serde_json::to_value(self.settings())?,
            Command::SetWorkspacesDir { path } => {
                serde_json::to_value(self.set_workspaces_dir(&path)?)?
            }
            Command::SetVerification { verification } => {
                serde_json::to_value(self.set_verification(verification)?)?
            }
            Command::LeftoverWorktrees => serde_json::to_value(self.leftover_worktrees()?)?,
            Command::RemoveLeftoverWorktrees { ticket_ids } => {
                serde_json::to_value(self.remove_leftover_worktrees(&ticket_ids)?)?
            }
        })
    }
}
