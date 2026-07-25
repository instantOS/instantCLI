//! SQLite-backed notification store
//!
//! Replaces the legacy `instantnotifyctl.sh` with proper parameterized
//! queries (no string interpolation = no SQL injection).

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::common::paths;

/// A single notification record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique auto-increment ID.
    pub id: i64,
    /// Local date and time when the notification was received.
    pub timestamp: String,
    /// Application that sent the notification.
    pub app_name: String,
    /// Notification summary/title.
    pub title: String,
    /// Notification body.
    pub body: String,
    /// Whether the notification has been read.
    pub read: bool,
    /// Actions advertised by the originating application.
    pub actions: Vec<NotificationAction>,
    /// Whether the notification server still considers this notification live.
    pub active: bool,
    /// Action most recently invoked through the notification server.
    pub invoked_action: Option<String>,
}

/// A live action advertised on a desktop notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

/// Notification content captured from a D-Bus `Notify` call.
pub struct CapturedNotification<'a> {
    pub timestamp: &'a str,
    pub app_name: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub actions: &'a [NotificationAction],
}

const DEFAULT_HISTORY_LIMIT: usize = 1000;
const SCHEMA_VERSION: i64 = 1;

/// SQLite database for storing notification history.
pub struct NotifyDb {
    conn: Connection,
}

impl NotifyDb {
    /// Open the notification database, creating it if it doesn't exist.
    pub fn open() -> Result<Self> {
        let path = paths::instant_data_dir()?.join("notifications.db");
        Self::open_at(&path)
    }

    /// Open the database at a specific path (useful for testing).
    pub fn open_at(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("creating notification db directory at {}", parent.display())
            })?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening notification database at {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;

        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting notification database at {}", path.display()))?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Create the notifications table. Pre-release schemas are intentionally
    /// discarded: notification history has not shipped with compatibility
    /// guarantees yet.
    fn init_schema(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            self.conn.execute_batch(
                "DROP TABLE IF EXISTS notifications;
                 DROP TABLE IF EXISTS notification_settings;",
            )?;
        }
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notifications (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT    NOT NULL,
                app_name    TEXT    NOT NULL,
                title       TEXT    NOT NULL,
                body        TEXT    NOT NULL,
                read        INTEGER NOT NULL DEFAULT 0,
                sender      TEXT,
                external_id INTEGER,
                active      INTEGER NOT NULL DEFAULT 1,
                actions_json TEXT   NOT NULL DEFAULT '[]',
                invoked_action TEXT
            );
            CREATE TABLE IF NOT EXISTS notification_settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        self.conn.execute(
            "INSERT OR IGNORE INTO notification_settings (key, value) VALUES ('history_limit', ?1)",
            params![DEFAULT_HISTORY_LIMIT.to_string()],
        )?;
        self.conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS notifications_active_external_id
                 ON notifications(external_id)
                 WHERE external_id IS NOT NULL AND active = 1;
             CREATE TRIGGER IF NOT EXISTS notifications_trim_after_insert
                 AFTER INSERT ON notifications
                 BEGIN
                     DELETE FROM notifications
                     WHERE id NOT IN (
                         SELECT id FROM notifications ORDER BY id DESC
                         LIMIT CAST((SELECT value FROM notification_settings
                                     WHERE key = 'history_limit') AS INTEGER)
                     );
                 END;
             PRAGMA user_version = 1;",
        )?;
        Ok(())
    }

    /// Insert a new notification and return its ID.
    #[cfg(test)]
    pub fn add(&self, timestamp: &str, app_name: &str, title: &str, body: &str) -> Result<i64> {
        self.add_captured(
            &CapturedNotification {
                timestamp,
                app_name,
                title,
                body,
                actions: &[],
            },
            None,
            None,
        )
    }

    /// Insert a captured notification with optional D-Bus identity metadata.
    pub fn add_captured(
        &self,
        notification: &CapturedNotification<'_>,
        sender: Option<&str>,
        external_id: Option<u32>,
    ) -> Result<i64> {
        let actions_json = serde_json::to_string(notification.actions)?;
        self.conn.execute(
            "INSERT INTO notifications
                (timestamp, app_name, title, body, read, actions_json, sender, external_id)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7)",
            params![
                notification.timestamp,
                notification.app_name,
                notification.title,
                notification.body,
                actions_json,
                sender,
                external_id
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Replace a previously captured notification using its D-Bus identity.
    /// Returns the updated local row ID when a matching notification exists.
    pub fn replace_captured(
        &self,
        notification: &CapturedNotification<'_>,
        sender: &str,
        external_id: u32,
    ) -> Result<Option<i64>> {
        let actions_json = serde_json::to_string(notification.actions)?;
        let id = self
            .conn
            .query_row(
                "UPDATE notifications
                 SET timestamp = ?1, app_name = ?2, title = ?3, body = ?4,
                     read = 0, actions_json = ?5, invoked_action = NULL
                 WHERE sender = ?6 AND external_id = ?7 AND active = 1
                 RETURNING id",
                params![
                    notification.timestamp,
                    notification.app_name,
                    notification.title,
                    notification.body,
                    actions_json,
                    sender,
                    external_id
                ],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    /// Mark the live notification with this server ID as closed.
    pub fn mark_closed(&self, external_id: u32) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE notifications SET active = 0 WHERE external_id = ?1 AND active = 1",
            params![external_id],
        )? != 0)
    }

    /// Record an action emitted by the notification server.
    pub fn record_action(&self, external_id: u32, action_key: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE notifications SET invoked_action = ?1
             WHERE external_id = ?2 AND active = 1",
            params![action_key, external_id],
        )? != 0)
    }

    /// Expire all live-state claims, for example after the capture daemon
    /// restarts and can no longer prove which older popups remain open.
    pub fn mark_all_inactive(&self) -> Result<usize> {
        self.conn
            .execute("UPDATE notifications SET active = 0 WHERE active = 1", [])
            .map_err(Into::into)
    }

    /// Expire one local row after its D-Bus `Notify` request fails.
    pub fn mark_inactive(&self, id: i64) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE notifications SET active = 0 WHERE id = ?1",
            params![id],
        )? != 0)
    }

    /// Attach the notification daemon's ID to a newly inserted row.
    pub fn assign_external_id(&self, id: i64, sender: &str, external_id: u32) -> Result<bool> {
        // Notification servers may reuse IDs after a notification closes. Keep
        // old history rows, but ensure replacements target only the newest one.
        self.conn.execute(
            "UPDATE notifications SET active = 0
             WHERE external_id = ?1 AND id != ?2",
            params![external_id, id],
        )?;
        let changed = self.conn.execute(
            "UPDATE notifications
             SET sender = ?1, external_id = ?2, active = 1 WHERE id = ?3",
            params![sender, external_id, id],
        )?;
        Ok(changed != 0)
    }

    /// List all notifications, newest first.
    pub fn list(&self) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read, actions_json,
                    active, invoked_action
             FROM notifications ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], notification_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Fetch one notification by local ID.
    pub fn get(&self, id: i64) -> Result<Option<Notification>> {
        self.conn
            .query_row(
                "SELECT id, timestamp, app_name, title, body, read, actions_json,
                        active, invoked_action
                 FROM notifications WHERE id = ?1",
                params![id],
                notification_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Notifications whose title or body contains `keyword`, newest first.
    ///
    /// Non-destructive counterpart of [`Self::delete_by_keyword`]; use it to
    /// show the user what would be removed before confirming.
    pub fn find_by_keyword(&self, keyword: &str) -> Result<Vec<Notification>> {
        let pattern = format!("%{keyword}%");
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read, actions_json,
                    active, invoked_action
             FROM notifications WHERE title LIKE ?1 OR body LIKE ?1
             ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![pattern], notification_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// All notifications from `app_name`, newest first.
    ///
    /// Non-destructive counterpart of [`Self::delete_by_app`].
    pub fn find_by_app(&self, app_name: &str) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read, actions_json,
                    active, invoked_action
             FROM notifications WHERE app_name = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![app_name], notification_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// All notifications that have been read, newest first.
    ///
    /// Non-destructive counterpart of [`Self::delete_read`].
    pub fn find_read(&self) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read, actions_json,
                    active, invoked_action
             FROM notifications WHERE read = 1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], notification_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Get the total count of notifications.
    pub fn count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM notifications", [], |row| row.get(0))
            .map_err(Into::into)
    }

    /// Get the count of unread notifications.
    pub fn unread_count(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM notifications WHERE read = 0",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Mark a notification as read by ID.
    pub fn mark_read(&self, id: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE notifications SET read = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(changed != 0)
    }

    /// Mark a notification as unread by ID.
    pub fn mark_unread(&self, id: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE notifications SET read = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(changed != 0)
    }

    /// Mark all notifications as read.
    pub fn mark_all_read(&self) -> Result<()> {
        self.conn.execute("UPDATE notifications SET read = 1", [])?;
        Ok(())
    }

    /// Delete a notification by ID.
    pub fn delete(&self, id: i64) -> Result<bool> {
        let changed = self
            .conn
            .execute("DELETE FROM notifications WHERE id = ?1", params![id])?;
        Ok(changed != 0)
    }

    /// Delete all notifications from a specific application.
    pub fn delete_by_app(&self, app_name: &str) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM notifications WHERE app_name = ?1",
            params![app_name],
        )?;
        Ok(count)
    }

    /// Delete notifications containing a keyword in title or body.
    pub fn delete_by_keyword(&self, keyword: &str) -> Result<usize> {
        let pattern = format!("%{keyword}%");
        let count = self.conn.execute(
            "DELETE FROM notifications WHERE title LIKE ?1 OR body LIKE ?1",
            params![pattern],
        )?;
        Ok(count)
    }

    /// Delete all read notifications.
    pub fn delete_read(&self) -> Result<usize> {
        let count = self
            .conn
            .execute("DELETE FROM notifications WHERE read = 1", [])?;
        Ok(count)
    }

    /// Delete all notifications.
    pub fn delete_all(&self) -> Result<usize> {
        let count = self.conn.execute("DELETE FROM notifications", [])?;
        Ok(count)
    }

    /// Trim old notifications, keeping only the most recent `max_count`.
    pub fn trim_to(&self, max_count: usize) -> Result<usize> {
        let total = self.count()?;
        if total <= max_count as i64 {
            return Ok(0);
        }
        let delete_count = total - max_count as i64;
        self.conn.execute(
            "DELETE FROM notifications WHERE id NOT IN (
                SELECT id FROM notifications ORDER BY id DESC LIMIT ?1
            )",
            params![max_count as i64],
        )?;
        Ok(delete_count as usize)
    }

    /// Return the configured maximum number of notifications retained.
    pub fn history_limit(&self) -> Result<usize> {
        let value: String = self.conn.query_row(
            "SELECT value FROM notification_settings WHERE key = 'history_limit'",
            [],
            |row| row.get(0),
        )?;
        value
            .parse()
            .context("invalid history_limit in notification database")
    }

    /// Persist and immediately enforce the history limit.
    pub fn set_history_limit(&self, max_count: usize) -> Result<usize> {
        anyhow::ensure!(max_count > 0, "history limit must be greater than zero");
        self.conn.execute(
            "UPDATE notification_settings SET value = ?1 WHERE key = 'history_limit'",
            params![max_count.to_string()],
        )?;
        self.trim_to(max_count)
    }

    /// List all distinct application names that have notifications.
    pub fn list_apps(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT app_name FROM notifications ORDER BY app_name")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn parse_actions(json: String, column: usize) -> rusqlite::Result<Vec<NotificationAction>> {
    serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

/// Map a full notification row (id, timestamp, app_name, title, body, read,
/// actions_json, active, invoked_action) into a [`Notification`]. Shared by
/// every read query so the column order stays in sync.
fn notification_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notification> {
    Ok(Notification {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        app_name: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        read: row.get::<_, i64>(5)? != 0,
        actions: parse_actions(row.get::<_, String>(6)?, 6)?,
        active: row.get::<_, i64>(7)? != 0,
        invoked_action: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (NamedTempFile, NotifyDb) {
        let tmp = NamedTempFile::new().unwrap();
        let db = NotifyDb::open_at(tmp.path()).unwrap();
        (tmp, db)
    }

    fn captured<'a>(
        timestamp: &'a str,
        app_name: &'a str,
        title: &'a str,
        body: &'a str,
        actions: &'a [NotificationAction],
    ) -> CapturedNotification<'a> {
        CapturedNotification {
            timestamp,
            app_name,
            title,
            body,
            actions,
        }
    }

    #[test]
    fn test_add_and_list() {
        let (_tmp, db) = test_db();
        db.add("12:00", "Discord", "Hello", "World").unwrap();
        db.add("12:01", "Spotify", "Now Playing", "Song - Artist")
            .unwrap();

        let list = db.list().unwrap();
        assert_eq!(list.len(), 2);
        // Newest first
        assert_eq!(list[0].app_name, "Spotify");
        assert_eq!(list[1].app_name, "Discord");
    }

    #[test]
    fn test_mark_read_unread() {
        let (_tmp, db) = test_db();
        let id = db.add("12:00", "App", "Title", "Body").unwrap();

        assert!(!db.list().unwrap()[0].read);
        db.mark_read(id).unwrap();
        assert!(db.list().unwrap()[0].read);
        db.mark_unread(id).unwrap();
        assert!(!db.list().unwrap()[0].read);
    }

    #[test]
    fn test_delete() {
        let (_tmp, db) = test_db();
        let id = db.add("12:00", "App", "Title", "Body").unwrap();
        assert_eq!(db.count().unwrap(), 1);
        db.delete(id).unwrap();
        assert_eq!(db.count().unwrap(), 0);
    }

    #[test]
    fn test_delete_by_app() {
        let (_tmp, db) = test_db();
        db.add("12:00", "Discord", "Msg1", "Body1").unwrap();
        db.add("12:01", "Discord", "Msg2", "Body2").unwrap();
        db.add("12:02", "Spotify", "Play", "Song").unwrap();

        assert_eq!(db.count().unwrap(), 3);
        let deleted = db.delete_by_app("Discord").unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn test_delete_by_keyword() {
        let (_tmp, db) = test_db();
        db.add("12:00", "App", "Hello World", "Body").unwrap();
        db.add("12:01", "App", "Title", "Hello there").unwrap();
        db.add("12:02", "App", "Other", "Different").unwrap();

        let deleted = db.delete_by_keyword("Hello").unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn test_find_by_keyword_is_non_destructive() {
        let (_tmp, db) = test_db();
        db.add("12:00", "App", "Hello World", "Body").unwrap();
        db.add("12:01", "App", "Title", "Hello there").unwrap();
        db.add("12:02", "App", "Other", "Different").unwrap();

        let matches = db.find_by_keyword("Hello").unwrap();
        assert_eq!(matches.len(), 2);
        // Newest first
        assert_eq!(matches[0].title, "Title");
        assert_eq!(matches[1].title, "Hello World");
        // Nothing was deleted
        assert_eq!(db.count().unwrap(), 3);
    }

    #[test]
    fn test_find_by_app_is_non_destructive() {
        let (_tmp, db) = test_db();
        db.add("12:00", "Discord", "Msg1", "Body1").unwrap();
        db.add("12:01", "Discord", "Msg2", "Body2").unwrap();
        db.add("12:02", "Spotify", "Play", "Song").unwrap();

        let matches = db.find_by_app("Discord").unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(db.count().unwrap(), 3);
    }

    #[test]
    fn test_find_read_is_non_destructive() {
        let (_tmp, db) = test_db();
        let id1 = db.add("12:00", "App", "T1", "B1").unwrap();
        db.add("12:01", "App", "T2", "B2").unwrap();
        db.mark_read(id1).unwrap();

        let matches = db.find_read().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "T1");
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn test_delete_read() {
        let (_tmp, db) = test_db();
        let id1 = db.add("12:00", "App", "T1", "B1").unwrap();
        db.add("12:01", "App", "T2", "B2").unwrap();
        db.mark_read(id1).unwrap();

        let deleted = db.delete_read().unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn test_trim_to() {
        let (_tmp, db) = test_db();
        for i in 0..10 {
            db.add(&format!("12:0{i}"), "App", &format!("T{i}"), "B")
                .unwrap();
        }
        assert_eq!(db.count().unwrap(), 10);
        let deleted = db.trim_to(5).unwrap();
        assert_eq!(deleted, 5);
        assert_eq!(db.count().unwrap(), 5);
    }

    #[test]
    fn test_unread_count() {
        let (_tmp, db) = test_db();
        let id1 = db.add("12:00", "App", "T1", "B1").unwrap();
        db.add("12:01", "App", "T2", "B2").unwrap();
        assert_eq!(db.unread_count().unwrap(), 2);
        db.mark_read(id1).unwrap();
        assert_eq!(db.unread_count().unwrap(), 1);
    }

    #[test]
    fn test_list_apps() {
        let (_tmp, db) = test_db();
        db.add("12:00", "Discord", "M", "B").unwrap();
        db.add("12:01", "Spotify", "M", "B").unwrap();
        db.add("12:02", "Discord", "M2", "B").unwrap();

        let apps = db.list_apps().unwrap();
        assert_eq!(apps, vec!["Discord", "Spotify"]);
    }

    #[test]
    fn test_mark_all_read() {
        let (_tmp, db) = test_db();
        db.add("12:00", "App", "T1", "B1").unwrap();
        db.add("12:01", "App", "T2", "B2").unwrap();
        assert_eq!(db.unread_count().unwrap(), 2);
        db.mark_all_read().unwrap();
        assert_eq!(db.unread_count().unwrap(), 0);
    }

    #[test]
    fn history_limit_is_persisted_and_enforced_on_insert() {
        let (tmp, db) = test_db();
        assert_eq!(db.history_limit().unwrap(), DEFAULT_HISTORY_LIMIT);
        db.set_history_limit(2).unwrap();
        for i in 0..4 {
            db.add("12:00", "App", &format!("T{i}"), "Body").unwrap();
        }
        assert_eq!(db.count().unwrap(), 2);
        drop(db);

        let reopened = NotifyDb::open_at(tmp.path()).unwrap();
        assert_eq!(reopened.history_limit().unwrap(), 2);
        assert_eq!(reopened.count().unwrap(), 2);
    }

    #[test]
    fn replacement_updates_existing_notification() {
        let (_tmp, db) = test_db();
        let id = db
            .add_captured(
                &captured("12:00", "Downloader", "10%", "Starting", &[]),
                None,
                None,
            )
            .unwrap();
        assert!(db.assign_external_id(id, ":1.42", 7).unwrap());

        let replaced = db
            .replace_captured(
                &captured("12:01", "Downloader", "80%", "Nearly done", &[]),
                ":1.42",
                7,
            )
            .unwrap();
        assert_eq!(replaced, Some(id));
        assert_eq!(db.count().unwrap(), 1);
        let notification = db.get(id).unwrap().unwrap();
        assert_eq!(notification.title, "80%");
        assert_eq!(notification.body, "Nearly done");
    }

    #[test]
    fn reused_external_id_targets_newest_notification() {
        let (_tmp, db) = test_db();
        let old_id = db.add("12:00", "App", "Old", "Body").unwrap();
        db.assign_external_id(old_id, ":1.42", 7).unwrap();
        let new_id = db.add("12:01", "App", "New", "Body").unwrap();
        db.assign_external_id(new_id, ":1.42", 7).unwrap();

        let replaced = db
            .replace_captured(&captured("12:02", "App", "Newest", "Body", &[]), ":1.42", 7)
            .unwrap();
        assert_eq!(replaced, Some(new_id));
        assert_eq!(db.get(old_id).unwrap().unwrap().title, "Old");
        assert_eq!(db.get(new_id).unwrap().unwrap().title, "Newest");
    }

    #[test]
    fn missing_ids_report_no_change() {
        let (_tmp, db) = test_db();
        assert!(!db.mark_read(99).unwrap());
        assert!(!db.mark_unread(99).unwrap());
        assert!(!db.delete(99).unwrap());
    }

    #[test]
    fn resets_pre_release_notification_schema() {
        let tmp = NamedTempFile::new().unwrap();
        {
            let connection = Connection::open(tmp.path()).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE notifications (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        timestamp TEXT NOT NULL,
                        app_name TEXT NOT NULL,
                        title TEXT NOT NULL,
                        body TEXT NOT NULL,
                        read INTEGER NOT NULL DEFAULT 0
                    );
                    INSERT INTO notifications
                        (timestamp, app_name, title, body, read)
                    VALUES ('12:00', 'App', 'Existing', 'Body', 0);",
                )
                .unwrap();
        }

        let db = NotifyDb::open_at(tmp.path()).unwrap();
        assert_eq!(db.count().unwrap(), 0);
        assert_eq!(db.history_limit().unwrap(), DEFAULT_HISTORY_LIMIT);
    }

    #[test]
    fn stores_actions_and_tracks_live_state() {
        let (_tmp, db) = test_db();
        let actions = vec![NotificationAction {
            key: "default".into(),
            label: "Pair".into(),
        }];
        let id = db
            .add_captured(
                &captured("12:00", "Bluetooth", "Pair?", "Device", &actions),
                None,
                None,
            )
            .unwrap();
        db.assign_external_id(id, ":1.42", 9).unwrap();
        assert!(db.record_action(9, "default").unwrap());
        let notification = db.get(id).unwrap().unwrap();
        assert_eq!(notification.actions, actions);
        assert!(notification.active);
        assert_eq!(notification.invoked_action.as_deref(), Some("default"));
        assert!(db.mark_closed(9).unwrap());
        assert!(!db.get(id).unwrap().unwrap().active);
    }

    #[test]
    fn daemon_restart_expires_unverified_live_rows() {
        let (_tmp, db) = test_db();
        db.add("12:00", "App", "Title", "Body").unwrap();
        db.add("12:01", "App", "Title", "Body").unwrap();
        assert_eq!(db.mark_all_inactive().unwrap(), 2);
        assert!(
            db.list()
                .unwrap()
                .iter()
                .all(|notification| !notification.active)
        );
    }

    #[test]
    fn malformed_action_data_is_reported() {
        let (_tmp, db) = test_db();
        let id = db.add("12:00", "App", "Title", "Body").unwrap();
        db.conn
            .execute(
                "UPDATE notifications SET actions_json = 'not json' WHERE id = ?1",
                params![id],
            )
            .unwrap();
        assert!(db.get(id).is_err());
    }
}
