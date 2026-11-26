//! Paired devices in the store (schema 8, with `message_uploads`): each device's key, name and
//! access level, and which devices that paired by QR code the Inbox still notes.
use crate::*;

/// Adds the device table and sets schema 8, after `uploads::migrate`.
pub(crate) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS devices (
             id TEXT PRIMARY KEY,
             public_key BLOB NOT NULL,
             name TEXT NOT NULL,
             level TEXT NOT NULL,
             paired_at INTEGER NOT NULL,
             last_connected_at INTEGER
         );
         CREATE TABLE IF NOT EXISTS paired_notices (
             device_id TEXT PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE
         );
         PRAGMA user_version = 8;",
    )?;
    Ok(())
}

/// A paired device as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    /// SEC1 uncompressed.
    pub public_key: Vec<u8>,
    pub name: String,
    pub level: AccessLevel,
    pub paired_at: i64,
    pub last_connected_at: Option<i64>,
}

fn level_name(level: AccessLevel) -> &'static str {
    match level {
        AccessLevel::Control => "control",
        AccessLevel::ViewOnly => "view_only",
    }
}

fn device(row: &rusqlite::Row) -> rusqlite::Result<Device> {
    let level: String = row.get(3)?;
    Ok(Device {
        id: row.get(0)?,
        public_key: row.get(1)?,
        name: row.get(2)?,
        level: match level.as_str() {
            "control" => AccessLevel::Control,
            _ => AccessLevel::ViewOnly,
        },
        paired_at: row.get(4)?,
        last_connected_at: row.get(5)?,
    })
}

const COLUMNS: &str = "id, public_key, name, level, paired_at, last_connected_at";

impl Host {
    /// Every paired device, the most recently paired first.
    pub fn devices(&self) -> Result<Vec<Device>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {COLUMNS} FROM devices ORDER BY paired_at DESC, id"
        ))?;
        let devices = statement.query_map([], device)?.collect::<Result<_, _>>()?;
        Ok(devices)
    }
    pub fn device(&self, id: &str) -> Result<Option<Device>> {
        Ok(self
            .db
            .query_row(
                &format!("SELECT {COLUMNS} FROM devices WHERE id = ?1"),
                [id],
                device,
            )
            .optional()?)
    }
    /// Stores a newly paired device with Control, replacing one paired before with that key.
    pub fn add_device(&self, public_key: &[u8], name: &str, at: i64) -> Result<Device> {
        let id = workspace_core::pairing::key_id(public_key);
        self.db.execute(
            "INSERT OR REPLACE INTO devices (id, public_key, name, level, paired_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, public_key, name, level_name(AccessLevel::Control), at],
        )?;
        self.device(&id)?.context("The device was just stored")
    }
    pub fn set_device_level(&self, id: &str, level: AccessLevel) -> Result<()> {
        let changed = self.db.execute(
            "UPDATE devices SET level = ?2 WHERE id = ?1",
            params![id, level_name(level)],
        )?;
        ensure!(changed == 1, "No paired device has that id");
        Ok(())
    }
    pub fn remove_device(&self, id: &str) -> Result<()> {
        let changed = self.db.execute("DELETE FROM devices WHERE id = ?1", [id])?;
        ensure!(changed == 1, "No paired device has that id");
        Ok(())
    }
    pub fn device_connected(&self, id: &str, at: i64) -> Result<()> {
        self.db.execute(
            "UPDATE devices SET last_connected_at = ?2 WHERE id = ?1",
            params![id, at],
        )?;
        Ok(())
    }
    /// Notes in the Inbox that the device paired, until dismissed or the device is removed.
    pub(crate) fn note_paired(&self, id: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO paired_notices (device_id) VALUES (?1)",
            [id],
        )?;
        Ok(())
    }
    pub(crate) fn dismiss_paired_notice(&self, id: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM paired_notices WHERE device_id = ?1", [id])?;
        Ok(())
    }
    /// The devices noted as paired, the earliest first.
    pub(crate) fn paired_notices(&self) -> Result<Vec<PairedNotice>> {
        let mut statement = self.db.prepare(
            "SELECT d.id, d.name, d.paired_at FROM paired_notices n
             JOIN devices d ON d.id = n.device_id ORDER BY d.paired_at, d.id",
        )?;
        let notices = statement.query_map([], |row| {
            Ok(PairedNotice {
                device_id: row.get(0)?,
                device_name: row.get(1)?,
                paired_at: row.get(2)?,
            })
        })?;
        Ok(notices.collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_is_stored_with_control_and_can_be_lowered_and_removed() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        let key = workspace_core::pairing::KeyPair::generate().unwrap();
        let stored = host.add_device(&key.public_key(), "MacBook", 10).unwrap();
        assert_eq!(stored.level, AccessLevel::Control);
        assert_eq!(stored.id, key.id());
        host.set_device_level(&stored.id, AccessLevel::ViewOnly)
            .unwrap();
        host.device_connected(&stored.id, 20).unwrap();
        let device = host.device(&stored.id).unwrap().unwrap();
        assert_eq!(
            (device.level, device.last_connected_at),
            (AccessLevel::ViewOnly, Some(20))
        );
        host.remove_device(&stored.id).unwrap();
        assert!(host.devices().unwrap().is_empty());
        assert!(host.remove_device(&stored.id).is_err());
    }

    #[test]
    fn a_paired_notice_lasts_until_dismissed_or_the_device_is_removed() {
        let home = tempfile::tempdir().unwrap();
        let host = Host::open(home.path()).unwrap();
        let [phone, other] = [(); 2].map(|_| {
            let key = workspace_core::pairing::KeyPair::generate().unwrap();
            host.add_device(&key.public_key(), "iPhone", 10).unwrap().id
        });
        host.note_paired(&phone).unwrap();
        host.note_paired(&other).unwrap();
        assert_eq!(host.paired_notices().unwrap().len(), 2);
        host.dismiss_paired_notice(&phone).unwrap();
        host.remove_device(&other).unwrap();
        assert!(host.paired_notices().unwrap().is_empty());
    }
}
