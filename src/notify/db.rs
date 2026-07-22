//! SQLite-backed notification store
//!
//! Replaces the legacy `instantnotifyctl.sh` with proper parameterized
//! queries (no string interpolation = no SQL injection).

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::common::paths;

/// A single notification record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique auto-increment ID.
    pub id: i64,
    /// Time the notification was received (HH:MM format, matching legacy).
    pub timestamp: String,
    /// Application that sent the notification.
    pub app_name: String,
    /// Notification summary/title.
    pub title: String,
    /// Notification body.
    pub body: String,
    /// Whether the notification has been read.
    pub read: bool,
}

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
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating notification db directory at {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("opening notification database at {}", path.display()))?;

        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Create the notifications table if it doesn't exist.
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notifications (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   TEXT    NOT NULL,
                app_name    TEXT    NOT NULL,
                title       TEXT    NOT NULL,
                body        TEXT    NOT NULL,
                read        INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(())
    }

    /// Insert a new notification and return its ID.
    pub fn add(&self, timestamp: &str, app_name: &str, title: &str, body: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notifications (timestamp, app_name, title, body, read) VALUES (?1, ?2, ?3, ?4, 0)",
            params![timestamp, app_name, title, body],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List all notifications, newest first.
    pub fn list(&self) -> Result<Vec<Notification>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read FROM notifications ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Notification {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                app_name: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                read: row.get::<_, i64>(5)? != 0,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// List notifications for a specific page (legacy pagination support).
    pub fn list_page(&self, page: usize, page_size: usize) -> Result<Vec<Notification>> {
        let offset = (page * page_size) as i64;
        let limit = page_size as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, app_name, title, body, read FROM notifications ORDER BY id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(Notification {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                app_name: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                read: row.get::<_, i64>(5)? != 0,
            })
        })?;
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
            .query_row("SELECT COUNT(*) FROM notifications WHERE read = 0", [], |row| {
                row.get(0)
            })
            .map_err(Into::into)
    }

    /// Mark a notification as read by ID.
    pub fn mark_read(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET read = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Mark a notification as unread by ID.
    pub fn mark_unread(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET read = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Mark all notifications as read.
    pub fn mark_all_read(&self) -> Result<()> {
        self.conn.execute("UPDATE notifications SET read = 1", [])?;
        Ok(())
    }

    /// Delete a notification by ID.
    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM notifications WHERE id = ?1",
            params![id],
        )?;
        Ok(())
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
        let count = self.conn.execute("DELETE FROM notifications WHERE read = 1", [])?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_db() -> (NamedTempFile, NotifyDb) {
        let tmp = NamedTempFile::new().unwrap();
        let db = NotifyDb::open_at(tmp.path()).unwrap();
        (tmp, db)
    }

    #[test]
    fn test_add_and_list() {
        let (_tmp, db) = test_db();
        db.add("12:00", "Discord", "Hello", "World").unwrap();
        db.add("12:01", "Spotify", "Now Playing", "Song - Artist").unwrap();

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
            db.add(&format!("12:0{i}"), "App", &format!("T{i}"), "B").unwrap();
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
}
