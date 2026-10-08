//! Store schema 6 retires repository coordinators. Each one's tickets and agents move under its
//! project coordinator, its undelivered input is re-sent or listed, and it is archived with its
//! conversation intact. Nothing is deleted, and a copy of the store from before is kept.
use crate::*;

const BACKUP: &str = "workspace.sqlite3.pre-flatten";

/// Runs once per store, in one transaction: a failure or crash leaves schema 5 for a full retry.
/// Earlier migrations reset `user_version` on every open, so `version` is the one read first.
pub(crate) fn migrate(home: &Path, db: &Connection, version: i64) -> Result<()> {
    if version >= 6 {
        db.execute_batch("PRAGMA user_version = 6;")?;
        return Ok(());
    }
    if version > 0 {
        back_up(home, db)?;
    }
    db.execute_batch("BEGIN IMMEDIATE")?;
    match retire_repository_coordinators(db) {
        Ok(()) => db.execute_batch("PRAGMA user_version = 6; COMMIT;")?,
        Err(error) => {
            db.execute_batch("ROLLBACK")?;
            return Err(error.context("Could not move repository teams to their coordinators"));
        }
    }
    Ok(())
}

/// A consistent copy beside the store, made once; a partial copy never takes its name.
fn back_up(home: &Path, db: &Connection) -> Result<()> {
    let backup = home.join(BACKUP);
    if backup.exists() {
        return Ok(());
    }
    let staged = home.join(format!("{BACKUP}.partial"));
    let _ = fs::remove_file(&staged);
    db.execute("VACUUM INTO ?1", [staged.to_string_lossy()])?;
    fs::rename(&staged, &backup)?;
    Ok(())
}

fn retire_repository_coordinators(db: &Connection) -> Result<()> {
    let present: i64 = db.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name='quiet'",
        [],
        |r| r.get(0),
    )?;
    if present == 0 {
        // Quiet messages ride along with the recipient's next turn without starting one.
        db.execute_batch("ALTER TABLE messages ADD COLUMN quiet INTEGER NOT NULL DEFAULT 0")?;
    }
    let sessions: Vec<Session> = db
        .prepare("SELECT data FROM sessions ORDER BY rowid")?
        .query_map([], |r| decode(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    for coordinator in sessions.iter().filter(|s| s.role == Role::TaskOrchestrator) {
        let migrated: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM activity WHERE session_id=?1 AND kind='team_migrated')",
            [&coordinator.id],
            |r| r.get(0),
        )?;
        if !migrated {
            retire(db, &sessions, coordinator)?;
        }
    }
    Ok(())
}

fn retire(db: &Connection, sessions: &[Session], retired: &Session) -> Result<()> {
    let main = sessions
        .iter()
        .find(|s| Some(&s.id) == retired.parent_id.as_ref())
        .or_else(|| {
            sessions.iter().find(|s| {
                s.project_id == retired.project_id
                    && s.role == Role::ProjectOrchestrator
                    && s.parent_id.is_none()
            })
        });
    let Some(main) = main else {
        // Only stores assembled by hand lack a project coordinator; with nothing to move,
        // archiving is all that is left to do.
        let owns: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM tickets WHERE coordinator_id=?1) OR EXISTS(SELECT 1 FROM sessions WHERE parent_id=?1)",
            [&retired.id],
            |r| r.get(0),
        )?;
        ensure!(
            !owns,
            "{} has no project coordinator to take over its work",
            retired.name
        );
        return archive(db, retired, "Retired; it had no work to move");
    };
    let repository = retired.repository_id.clone().unwrap_or_default();
    let tickets: Vec<Ticket> = db
        .prepare("SELECT data FROM tickets WHERE coordinator_id=?1 ORDER BY rowid")?
        .query_map([&retired.id], |r| decode(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    db.execute(
        "UPDATE tickets SET coordinator_id=?2,data=json_set(data,'$.coordinator_id',?2,'$.repository_id',?3) WHERE coordinator_id=?1",
        params![retired.id, main.id, repository],
    )?;
    let children: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.parent_id.as_ref() == Some(&retired.id))
        .collect();
    db.execute(
        "UPDATE sessions SET parent_id=?2,data=json_set(data,'$.parent_id',?2) WHERE parent_id=?1",
        params![retired.id, main.id],
    )?;

    // Delivered input belongs to a turn the stop interrupted; startup would hold it anyway.
    let inbound: Vec<Message> = db
        .prepare("SELECT * FROM messages WHERE recipient=?1 AND receipt IN ('queued','held','delivered') ORDER BY sequence")?
        .query_map([&retired.id], Host::message_row)?
        .collect::<rusqlite::Result<_>>()?;
    let mut resent = vec![];
    let mut listed = vec![];
    for message in &inbound {
        let from_child = children
            .iter()
            .any(|c| message.sender.as_ref() == Some(&c.id));
        if from_child {
            let id = format!("migrated:{}", message.id);
            db.execute(
                "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at) VALUES (?1,?2,?3,?4,?5,'queued',?6)",
                params![
                    id,
                    message.project_id,
                    message.sender,
                    main.id,
                    format!(
                        "Re-sent after {} was retired; original message {}:\n{}",
                        retired.name, message.id, message.body
                    ),
                    now()
                ],
            )?;
            resent.push(format!("- {id}, from {}", message.id));
        } else if message.sender.as_ref() != Some(&retired.id) {
            listed.push(format!("- {}: {}", message.id, excerpt(&message.body)));
        }
    }
    db.execute(
        "UPDATE messages SET receipt='cancelled' WHERE recipient=?1 AND receipt IN ('queued','held','delivered')",
        [&retired.id],
    )?;

    let timers: Vec<String> = db
        .prepare(
            "SELECT id,label FROM schedules WHERE session_id=?1 AND stopped IS NULL ORDER BY rowid",
        )?
        .query_map([&retired.id], |r| {
            Ok(format!(
                "- {} \"{}\"",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Host::stop_schedules_in(db, std::slice::from_ref(&retired.id), None, "migrated")?;

    let open: Vec<Attention> = db
        .prepare("SELECT data FROM attention WHERE session_id=?1 AND json_extract(data,'$.answer') IS NULL")?
        .query_map([&retired.id], |r| decode(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    for mut attention in open {
        attention.answer = Some("Denied: coordinator migrated".into());
        db.execute(
            "UPDATE attention SET data=?2 WHERE id=?1",
            params![attention.id, encode(&attention)?],
        )?;
        Host::event(
            db,
            &attention.project_id,
            Some(&retired.id),
            "attention_resolved",
            &attention.operation_id,
        )?;
    }

    archive(
        db,
        retired,
        &format!("Tickets and agents moved to {}", main.name),
    )?;

    let notice = notice(db, retired, &tickets, &resent, &listed, &timers)?;
    db.execute(
        "INSERT INTO messages(id,project_id,sender,recipient,body,receipt,created_at,quiet) VALUES (?1,?2,NULL,?3,?4,'queued',?5,1)",
        params![format!("migration:{}", retired.id), retired.project_id, main.id, notice, now()],
    )?;
    Ok(())
}

/// Archives the retired coordinator and records that it was migrated, which also keeps a
/// re-run from migrating it twice.
fn archive(db: &Connection, retired: &Session, detail: &str) -> Result<()> {
    let mut archived = retired.clone();
    archived.archived = true;
    if archived.status == Status::Working {
        archived.status = Status::Ready;
    }
    db.execute(
        "UPDATE sessions SET data=?2 WHERE id=?1",
        params![archived.id, encode(&archived)?],
    )?;
    Host::event(
        db,
        &retired.project_id,
        Some(&retired.id),
        "team_migrated",
        detail,
    )
}

fn notice(
    db: &Connection,
    retired: &Session,
    tickets: &[Ticket],
    resent: &[String],
    listed: &[String],
    timers: &[String],
) -> Result<String> {
    let mut lines = vec![format!(
        "Wiffletree retired the repository coordinator \"{}\" ({}). You now own its tickets and agents directly; its conversation stays readable in the archive.",
        retired.name, retired.id
    )];
    lines.push("\nTickets:".into());
    for ticket in tickets {
        let agents: Vec<String> = db
            .prepare("SELECT s.data FROM runtimes r JOIN sessions s ON s.id=r.session_id WHERE json_extract(r.data,'$.ticket_id')=?1 ORDER BY s.rowid")?
            .query_map([&ticket.id], |r| decode::<Session>(r, 0))?
            .map(|s| s.map(|s| format!("{} ({})", s.name, s.id)))
            .collect::<rusqlite::Result<_>>()?;
        lines.push(format!(
            "- \"{}\" ({}): {}, branch {}, worktree {}; agents: {}",
            ticket.title,
            ticket.id,
            ticket.state,
            ticket.branch,
            ticket.worktree,
            if agents.is_empty() {
                "none".into()
            } else {
                agents.join(", ")
            }
        ));
    }
    if tickets.is_empty() {
        lines.push("- none".into());
    }
    for (title, entries) in [
        ("Unread reports re-sent to you:", resent),
        (
            "Unread instructions cancelled and not re-sent; decide what still applies:",
            listed,
        ),
        ("Stopped timers:", timers),
    ] {
        if !entries.is_empty() {
            lines.push(format!("\n{title}"));
            lines.extend(entries.iter().cloned());
        }
    }
    let mut notice = lines.join("\n");
    notice.truncate(steps::floor_boundary(
        &notice,
        notice.len().min(MAX_TEXT_BYTES),
    ));
    Ok(notice)
}

fn excerpt(body: &str) -> String {
    let line: String = body.chars().take(200).collect();
    let line = line.replace('\n', " ");
    if body.chars().count() > 200 {
        format!("{line}…")
    } else {
        line
    }
}
