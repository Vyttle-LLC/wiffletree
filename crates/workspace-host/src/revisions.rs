//! Revisions follow durable writes, including writes inside transactions and recovery.
//! Connection-local triggers cover the single writer without binding a closed store's schema
//! to optional columns that older-store migrations add on open.
use crate::*;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS archive_revision (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL);
         INSERT OR IGNORE INTO archive_revision VALUES (1,1);
         CREATE TABLE IF NOT EXISTS archive_turns (session_id TEXT PRIMARY KEY REFERENCES sessions(id));
         CREATE TEMP VIEW IF NOT EXISTS revision_archive_sessions AS SELECT id FROM sessions WHERE {};
         CREATE TEMP VIEW IF NOT EXISTS revision_archive_tickets AS SELECT id FROM tickets WHERE {};",
        sessions_where(false), tickets_listed(false)
    ))?;
    // A row can enter or leave the archive indirectly: settling a ticket changes its archived
    // agents' membership, and restoring an agent takes its settled ticket out of the archive.
    for table in ["sessions", "tickets", "runtimes"] {
        for (event, timing, row) in [
            ("INSERT", "BEFORE", "NEW"),
            ("INSERT", "AFTER", "NEW"),
            ("DELETE", "BEFORE", "OLD"),
            ("DELETE", "AFTER", "OLD"),
            ("UPDATE", "BEFORE", "OLD"),
            ("UPDATE", "AFTER", "NEW"),
        ] {
            let key = if table == "runtimes" {
                "session_id"
            } else {
                "id"
            };
            let changed = match (event, timing) {
                ("UPDATE", _) => "OLD.data IS NOT NEW.data".into(),
                // An UPSERT runs BEFORE INSERT even when it takes a no-op UPDATE instead.
                ("INSERT", "BEFORE") => {
                    format!("NOT EXISTS (SELECT 1 FROM {table} WHERE {key}=NEW.{key})")
                }
                _ => "1".into(),
            };
            let membership_changed = match (table, event) {
                ("sessions", "UPDATE") => "coalesce(json_extract(OLD.data,'$.archived'),0) != coalesce(json_extract(NEW.data,'$.archived'),0)".into(),
                ("tickets", "UPDATE") => format!("({}) != ({})", open("OLD.data"), open("NEW.data")),
                ("runtimes", "UPDATE") => "json_extract(OLD.data,'$.ticket_id') IS NOT json_extract(NEW.data,'$.ticket_id')".into(),
                _ => "1".into(),
            };
            let affected = match table {
                "sessions" => format!(
                    "EXISTS (SELECT 1 FROM revision_archive_sessions WHERE id={row}.id)
                     OR (({membership_changed}) AND EXISTS (
                       SELECT 1 FROM runtimes r JOIN tickets t ON t.id=json_extract(r.data,'$.ticket_id')
                       WHERE r.session_id={row}.id AND NOT ({}) AND NOT EXISTS (
                         SELECT 1 FROM runtimes other JOIN sessions s ON s.id=other.session_id
                         WHERE json_extract(other.data,'$.ticket_id')=t.id AND s.id!={row}.id
                         AND coalesce(json_extract(s.data,'$.archived'),0)=0)))", open("t.data")),
                "tickets" => format!(
                    "EXISTS (SELECT 1 FROM revision_archive_tickets WHERE id={row}.id)
                     OR (({membership_changed}) AND ({}) AND EXISTS (
                       SELECT 1 FROM runtimes r JOIN sessions s ON s.id=r.session_id
                       WHERE json_extract(r.data,'$.ticket_id')={row}.id
                       AND coalesce(json_extract(s.data,'$.archived'),0)=1))",
                    if event == "UPDATE" { "1".into() } else { open(&format!("{row}.data")) }),
                "runtimes" => format!(
                    "EXISTS (SELECT 1 FROM revision_archive_sessions WHERE id={row}.session_id)
                     OR (({membership_changed}) AND EXISTS (
                       SELECT 1 FROM sessions s JOIN tickets t ON t.id=json_extract({row}.data,'$.ticket_id')
                       WHERE s.id={row}.session_id AND coalesce(json_extract(s.data,'$.archived'),0)=0
                       AND NOT ({}) AND NOT EXISTS (
                         SELECT 1 FROM runtimes other JOIN sessions a ON a.id=other.session_id
                         WHERE json_extract(other.data,'$.ticket_id')=t.id AND a.id!=s.id
                         AND coalesce(json_extract(a.data,'$.archived'),0)=0)))", open("t.data")),
                _ => unreachable!(),
            };
            db.execute_batch(&format!(
                "CREATE TEMP TRIGGER IF NOT EXISTS revision_{table}_{event}_{timing} {timing} {event} ON main.{table}
                 WHEN ({changed}) AND ({affected}) BEGIN
                   UPDATE archive_revision SET revision=revision+1;
                 END;"
            ))?;
        }
    }
    db.execute_batch(
        "CREATE TEMP TRIGGER IF NOT EXISTS revision_turn_started AFTER INSERT ON main.archive_turns
         WHEN EXISTS (SELECT 1 FROM revision_archive_sessions WHERE id=NEW.session_id) BEGIN
           UPDATE archive_revision SET revision=revision+1;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS revision_turn_ended BEFORE DELETE ON main.archive_turns
         WHEN EXISTS (SELECT 1 FROM revision_archive_sessions WHERE id=OLD.session_id) BEGIN
           UPDATE archive_revision SET revision=revision+1;
         END;",
    )?;
    // A restart ends the previous host's turns, even when their stored status was already done.
    db.execute("DELETE FROM archive_turns", [])?;
    messages(db)
}

fn open(data: &str) -> String {
    let states = TicketState::ALL
        .into_iter()
        .filter(|s| s.is_open())
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",");
    format!("json_extract({data},'$.state') IN ({states})")
}

fn messages(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS message_revisions (session_id TEXT PRIMARY KEY REFERENCES sessions(id), revision INTEGER NOT NULL);
         CREATE TEMP TRIGGER IF NOT EXISTS revision_message_insert AFTER INSERT ON main.messages BEGIN
           INSERT INTO message_revisions VALUES (NEW.recipient,1)
           ON CONFLICT(session_id) DO UPDATE SET revision=revision+1;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS revision_message_delete AFTER DELETE ON main.messages BEGIN
           INSERT INTO message_revisions VALUES (OLD.recipient,1)
           ON CONFLICT(session_id) DO UPDATE SET revision=revision+1;
         END;
         CREATE TEMP TRIGGER IF NOT EXISTS revision_message_update AFTER UPDATE ON main.messages
         WHEN (OLD.sequence,OLD.id,OLD.project_id,OLD.sender,OLD.recipient,OLD.body,OLD.receipt,OLD.created_at,OLD.attachments,OLD.reply_update)
           IS NOT (NEW.sequence,NEW.id,NEW.project_id,NEW.sender,NEW.recipient,NEW.body,NEW.receipt,NEW.created_at,NEW.attachments,NEW.reply_update) BEGIN
           INSERT INTO message_revisions VALUES (OLD.recipient,1)
           ON CONFLICT(session_id) DO UPDATE SET revision=revision+1;
           INSERT INTO message_revisions SELECT NEW.recipient,1 WHERE NEW.recipient!=OLD.recipient
           ON CONFLICT(session_id) DO UPDATE SET revision=revision+1;
         END;"
    )?;
    Ok(())
}

impl Host {
    pub(crate) fn archive_stamp(&self) -> Result<u64> {
        Ok(self.db.query_row(
            "SELECT revision FROM archive_revision WHERE singleton=1",
            [],
            |r| r.get(0),
        )?)
    }

    /// Only live snapshot sessions need a signal. An absent entry means unknown, including
    /// sessions from an older host and archived sessions restored from a client's cache.
    pub(crate) fn message_revisions(&self) -> Result<std::collections::BTreeMap<String, u64>> {
        Ok(self.db.prepare(&format!(
            "SELECT s.id,coalesce(m.revision,0) FROM sessions s LEFT JOIN message_revisions m ON m.session_id=s.id
             WHERE {} OR s.id IN (SELECT session_id FROM archive_turns)", sessions_where(true)
        ))?.query_map([], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn turn_started(&mut self, session: &str) -> Result<()> {
        self.db
            .execute("INSERT OR IGNORE INTO archive_turns VALUES (?1)", [session])?;
        self.in_turn.insert(session.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn old_hash(host: &Host) -> Result<u64> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for sql in [
            format!(
                "SELECT id,data FROM sessions WHERE {} ORDER BY rowid",
                sessions_where(false)
            ),
            format!(
                "SELECT id,data FROM tickets WHERE {} ORDER BY rowid",
                tickets_listed(false)
            ),
            format!(
                "SELECT session_id,data FROM runtimes WHERE session_id IN (SELECT id FROM sessions WHERE {}) ORDER BY rowid",
                sessions_where(false)
            ),
        ] {
            let mut statement = host.db.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let id = row.get_ref(0)?.as_str()?;
                (id, row.get_ref(1)?.as_str()?, host.in_turn.contains(id)).hash(&mut hasher);
            }
            0xff_u8.hash(&mut hasher);
        }
        Ok(hasher.finish())
    }
    fn step(host: &mut Host, label: &str, work: impl FnOnce(&mut Host) -> Result<()>) {
        let before = (old_hash(host).unwrap(), host.archive_stamp().unwrap());
        work(host).unwrap();
        let after = (old_hash(host).unwrap(), host.archive_stamp().unwrap());
        assert_eq!(before.0 != after.0, before.1 != after.1, "{label}");
        assert_eq!(host.archive().unwrap().stamp, after.1);
        assert_eq!(host.snapshot().unwrap().archive_stamp, after.1);
    }

    #[test]
    fn archive_revision_matches_the_old_hash_through_a_scripted_lifecycle() {
        let mut f = publish::tests::fixture();
        let host = &mut f.host;
        let id = f.implementer.id.clone();
        let ticket_id = f.ticket.clone();
        step(host, "live status", |h| h.set_status(&id, Status::Done));
        step(host, "turn starts live", |h| h.turn_started(&id));
        step(host, "archived agent of open ticket stays live", |h| {
            h.set_archived(&id, true).map(drop)
        });
        step(host, "live runtime", |h| {
            let mut r = h.session_runtime(&id)?;
            r.last_error = Some("changed".into());
            h.save_runtime(&r)
        });
        step(
            host,
            "ticket settles with archived agent still in turn",
            |h| {
                let mut t = h.ticket(&ticket_id)?;
                t.state = TicketState::Closed;
                h.save_ticket(&t)
            },
        );
        step(host, "archived runtime", |h| {
            let mut r = h.session_runtime(&id)?;
            r.last_error = Some("archived".into());
            h.save_runtime(&r)
        });
        step(host, "no-op archived runtime", |h| {
            h.save_runtime(&h.session_runtime(&id)?)
        });
        step(host, "no-op archived session", |h| {
            h.set_status(&id, Status::Done)
        });
        step(host, "archived turn ends", |h| h.turn_ended(&id));
        step(host, "repeated turn end", |h| h.turn_ended(&id));
        step(host, "restore moves the settled ticket live", |h| {
            h.set_archived(&id, false).map(drop)
        });
        step(host, "live runtime of restored agent", |h| {
            let mut r = h.session_runtime(&id)?;
            r.last_error = None;
            h.save_runtime(&r)
        });
        step(host, "settled ticket edited while restored", |h| {
            let mut t = h.ticket(&ticket_id)?;
            t.title.push_str(" edited");
            h.save_ticket(&t)
        });
        step(host, "archive returns the settled ticket", |h| {
            h.set_archived(&id, true).map(drop)
        });
        step(host, "settled ticket edited in archive", |h| {
            let mut t = h.ticket(&ticket_id)?;
            t.title.push_str(" archived");
            h.save_ticket(&t)
        });
        step(host, "archived turn starts", |h| h.turn_started(&id));
        step(host, "repeated turn start", |h| h.turn_started(&id));
        step(host, "transaction rolls back", |h| {
            let failed: Result<()> = h.atomically(|h| {
                h.set_status(&id, Status::Blocked)?;
                bail!("rollback")
            });
            assert!(failed.is_err());
            Ok(())
        });
        step(host, "runtime leaves a closed ticket", |h| {
            let mut r = h.session_runtime(&id)?;
            r.ticket_id = None;
            h.save_runtime(&r)
        });
        step(host, "runtime rejoins an open ticket", |h| {
            let mut t = h.ticket(&ticket_id)?;
            t.state = TicketState::Assigned;
            h.save_ticket(&t)?;
            let mut r = h.session_runtime(&id)?;
            r.ticket_id = Some(ticket_id.clone());
            h.save_runtime(&r)
        });
        step(host, "open ticket closes while agent archived", |h| {
            let mut t = h.ticket(&ticket_id)?;
            t.state = TicketState::Closed;
            h.save_ticket(&t)
        });
        let before = (old_hash(host).unwrap(), host.archive_stamp().unwrap());
        let home = host.home.clone();
        drop(f.host);
        let host = Host::open(home).unwrap();
        assert_ne!(
            old_hash(&host).unwrap(),
            before.0,
            "restart ends archived turn"
        );
        assert_ne!(host.archive_stamp().unwrap(), before.1);
        let revision = host.archive_stamp().unwrap();
        let home = host.home.clone();
        drop(host);
        assert_eq!(
            Host::open(home).unwrap().archive_stamp().unwrap(),
            revision,
            "idle restart"
        );
    }

    #[test]
    fn archive_membership_tracks_each_agents_runtime_and_the_other_restored_agents() {
        let mut f = publish::tests::fixture();
        let other = f
            .host
            .assign_ticket(&f.ticket, Role::Reviewer, Provider::Claude, "Review", None)
            .unwrap();
        let agents = [f.implementer.id.clone(), other.id];
        for state in [
            TicketState::Assigned,
            TicketState::Closed,
            TicketState::Accepted,
        ] {
            step(&mut f.host, "ticket state", |h| {
                let mut ticket = h.ticket(&f.ticket)?;
                ticket.state = state;
                h.save_ticket(&ticket)
            });
            for archived in [false, true, false] {
                for id in &agents {
                    step(&mut f.host, "agent membership", |h| {
                        let mut session = h.session(id)?;
                        session.archived = archived;
                        h.db.execute(
                            "UPDATE sessions SET data=?2 WHERE id=?1",
                            params![id, encode(&session)?],
                        )?;
                        Ok(())
                    });
                    for ticket in [None, Some(f.ticket.clone())] {
                        step(&mut f.host, "runtime membership", |h| {
                            let mut runtime = h.session_runtime(id)?;
                            runtime.ticket_id = ticket;
                            h.save_runtime(&runtime)
                        });
                        step(&mut f.host, "no-op runtime upsert", |h| {
                            h.save_runtime(&h.session_runtime(id)?)
                        });
                    }
                }
            }
        }
    }

    #[test]
    fn message_revisions_are_local_durable_and_follow_all_message_writes() {
        let home = tempfile::tempdir().unwrap();
        let mut host = Host::open_set_up(home.path()).unwrap();
        host.create_project("First").unwrap();
        host.create_project("Second").unwrap();
        let sessions = host.sessions().unwrap();
        let a = &sessions[0].id;
        let b = &sessions[1].id;
        let revisions = |host: &Host| host.snapshot().unwrap().message_revisions;
        let initial = revisions(&host);
        host.send("one", Some(b), a, "hello").unwrap_err(); // cross-project write is rejected
        assert_eq!(revisions(&host), initial);
        host.send("one", None, a, "hello").unwrap();
        let sent = revisions(&host);
        assert_ne!(sent[a], initial[a]);
        assert_eq!(sent[b], initial[b]);
        host.send("one", None, a, "hello").unwrap();
        assert_eq!(revisions(&host), sent, "idempotent send");
        host.advance_receipt("one", Receipt::Delivered).unwrap();
        let delivered = revisions(&host);
        assert_ne!(delivered[a], sent[a]);
        assert_eq!(delivered[b], sent[b]);
        host.advance_receipt("one", Receipt::Delivered).unwrap();
        host.rename_project(&sessions[0].project_id, "Renamed")
            .unwrap();
        assert_eq!(revisions(&host), delivered, "unrelated and no-op writes");
        host.db
            .execute("UPDATE messages SET body='edited' WHERE id='one'", [])
            .unwrap();
        let edited = revisions(&host);
        assert_ne!(edited[a], delivered[a]);
        host.db
            .execute("UPDATE messages SET body=body WHERE id='one'", [])
            .unwrap();
        assert_eq!(revisions(&host), edited);
        let failed: Result<()> = host.atomically(|h| {
            h.send("rollback", None, a, "no")?;
            bail!("rollback")
        });
        assert!(failed.is_err());
        assert_eq!(revisions(&host), edited);
        host.db
            .execute("DELETE FROM messages WHERE id='one'", [])
            .unwrap();
        let deleted = revisions(&host);
        assert_ne!(deleted[a], edited[a]);
        assert_eq!(deleted[b], initial[b]);
        host.append_output(&sessions[0], "run", "reply").unwrap();
        let reply = revisions(&host);
        assert_ne!(reply[a], deleted[a]);
        host.append_output(&sessions[0], "run", "reply").unwrap();
        assert_eq!(revisions(&host), reply, "same final reply");
        host.append_output(&sessions[0], "run", "updated reply")
            .unwrap();
        let final_reply = revisions(&host);
        assert_ne!(final_reply[a], reply[a]);
        assert_eq!(final_reply[b], initial[b]);
        let mut json = serde_json::to_value(host.snapshot().unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("message_revisions");
        assert!(
            serde_json::from_value::<Snapshot>(json)
                .unwrap()
                .message_revisions
                .is_empty()
        );
        drop(host);
        assert_eq!(revisions(&Host::open(home.path()).unwrap()), final_reply);
    }
}
