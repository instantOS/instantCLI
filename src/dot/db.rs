use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

const CURRENT_SCHEMA_VERSION: i32 = 1;

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = super::config::db_path()?;
        let conn = Connection::open(db_path)?;

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", ())?;

        // Initialize or update schema
        Self::init_schema(&conn)?;

        Ok(Database { conn })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        // Create schema version table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL,
                updated TEXT NOT NULL,
                PRIMARY KEY (version)
            )",
            (),
        )?;

        // Get current schema version
        let version = match conn.query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // No schema version found, initialize with version 0
                conn.execute(
                    "INSERT INTO schema_version (version, updated) VALUES (0, datetime('now'))",
                    [],
                )?;
                0
            }
            Err(e) => return Err(e.into()),
        };

        // Run migrations if needed
        if version < CURRENT_SCHEMA_VERSION {
            Self::migrate_schema(conn, version)?;
        }

        Ok(())
    }

    fn migrate_schema(conn: &Connection, from_version: i32) -> Result<()> {
        match from_version {
            0 => {
                // Initial schema creation
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS hashes (
                        created TEXT NOT NULL,
                        hash TEXT NOT NULL,
                        path TEXT NOT NULL,
                        unmodified INTEGER NOT NULL,
                        PRIMARY KEY (hash, path)
                    )",
                    (),
                )?;

                // Update to version 1
                conn.execute(
                    "INSERT INTO schema_version (version, updated) VALUES (1, datetime('now'))",
                    [],
                )?;
            }
            // Future migrations can be added here
            _ => {}
        }
        Ok(())
    }

    pub fn add_hash(&self, hash: &str, path: &Path, unmodified: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO hashes (created, hash, path, unmodified) VALUES (datetime('now'), ?, ?, ?)",
            (hash, path.to_str().unwrap(), unmodified),
        )?;
        Ok(())
    }

    pub fn hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM hashes WHERE hash = ? AND path = ?")?;
        let mut result =
            stmt.query_map([hash, path.to_str().unwrap()], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    pub fn get_unmodified_hashes(&self, path: &Path) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM hashes WHERE path = ? AND unmodified = 1")?;
        let hashes = stmt.query_map([path.to_str().unwrap()], |row| row.get(0))?;
        let mut result = Vec::new();
        for hash in hashes {
            result.push(hash?);
        }
        Ok(result)
    }

    /// Get the newest hash timestamp for a file, if any exists
    // TODO: change this to return the newest hash once the hash struct mentioned in another TODO comment is implemented
    pub fn get_newest_hash_timestamp(&self, path: &Path) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT MAX(created) FROM hashes WHERE path = ?")?;
        let result: Option<String> = stmt.query_row([path.to_str().unwrap()], |row| row.get(0))?;
        Ok(result)
    }

    pub fn cleanup_hashes(&self) -> Result<()> {
        // Keep all unmodified hashes, and for modified hashes:
        // 1. Keep the newest modified hash per file (for rollback capability)
        // 2. Remove modified hashes older than 30 days

        // First, remove modified hashes older than 30 days
        self.conn.execute(
            "DELETE FROM hashes WHERE unmodified = 0 AND created < datetime('now', '-30 days')",
            (),
        )?;

        // Then, for each file, keep only the newest modified hash
        self.conn.execute(
            "DELETE FROM hashes WHERE unmodified = 0 AND rowid NOT IN (
                SELECT MAX(rowid)
                FROM hashes
                WHERE unmodified = 0
                GROUP BY path
            )",
            (),
        )?;

        Ok(())
    }

    pub fn get_hash_stats(&self) -> Result<(i32, i32, i32)> {
        let total: i32 = self
            .conn
            .query_row("SELECT COUNT(*) FROM hashes", [], |row| row.get(0))?;

        let valid: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM hashes WHERE unmodified = 1",
            [],
            |row| row.get(0),
        )?;

        let invalid: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM hashes WHERE unmodified = 0",
            [],
            |row| row.get(0),
        )?;

        Ok((total, valid, invalid))
    }

    pub fn cleanup_all_invalid_hashes(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM hashes WHERE unmodified = 0", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_hash_cleanup() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = Connection::open(db_path).unwrap();

        // Create test database
        conn.execute(
            "CREATE TABLE hashes (
                created TEXT NOT NULL,
                hash TEXT NOT NULL,
                path TEXT NOT NULL,
                unmodified INTEGER NOT NULL,
                PRIMARY KEY (hash, path)
            )",
            (),
        )
        .unwrap();

        let db = Database { conn };

        // Add test hashes
        let test_path = PathBuf::from("/home/user/test.txt");

        // Add unmodified hashes (should never be cleaned up)
        db.add_hash("valid1", &test_path, true).unwrap();
        db.add_hash("valid2", &test_path, true).unwrap();

        // Add invalid hashes with different timestamps
        db.conn
            .execute(
                "INSERT INTO hashes (created, hash, path, valid) VALUES
                (datetime('now', '-40 days'), 'old_invalid1', ?, 0),
                (datetime('now', '-35 days'), 'old_invalid2', ?, 0),
                (datetime('now', '-10 days'), 'recent_invalid1', ?, 0),
                (datetime('now', '-5 days'), 'recent_invalid2', ?, 0)",
                &[
                    test_path.to_str().unwrap(),
                    test_path.to_str().unwrap(),
                    test_path.to_str().unwrap(),
                    test_path.to_str().unwrap(),
                ],
            )
            .unwrap();

        // Test initial state
        let (total, valid, invalid) = db.get_hash_stats().unwrap();
        assert_eq!(total, 6);
        assert_eq!(valid, 2);
        assert_eq!(invalid, 4);

        // Run cleanup
        db.cleanup_hashes().unwrap();

        // Check results
        let (total_after, valid_after, invalid_after) = db.get_hash_stats().unwrap();
        assert_eq!(valid_after, 2); // Valid hashes should remain
        assert_eq!(invalid_after, 1); // Only newest invalid hash should remain
        assert_eq!(total_after, 3);

        // Verify the remaining invalid hash is the newest one
        let remaining_invalid: String = db
            .conn
            .query_row("SELECT hash FROM hashes WHERE unmodified = 0", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining_invalid, "recent_invalid2");
    }

    #[test]
    fn test_cleanup_all_invalid() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = Connection::open(db_path).unwrap();

        conn.execute(
            "CREATE TABLE hashes (
                created TEXT NOT NULL,
                hash TEXT NOT NULL,
                path TEXT NOT NULL,
                unmodified INTEGER NOT NULL,
                PRIMARY KEY (hash, path)
            )",
            (),
        )
        .unwrap();

        let db = Database { conn };

        let test_path = PathBuf::from("/home/user/test.txt");
        db.add_hash("valid1", &test_path, true).unwrap();
        db.add_hash("invalid1", &test_path, false).unwrap();
        db.add_hash("invalid2", &test_path, false).unwrap();

        let (total, valid, invalid) = db.get_hash_stats().unwrap();
        assert_eq!(total, 3);
        assert_eq!(valid, 1);
        assert_eq!(invalid, 2);

        db.cleanup_all_invalid_hashes().unwrap();

        let (total_after, valid_after, invalid_after) = db.get_hash_stats().unwrap();
        assert_eq!(total_after, 1);
        assert_eq!(valid_after, 1);
        assert_eq!(invalid_after, 0);
    }

    #[test]
    fn test_hash_exists() {
        let dir = tempdir().unwrap();
        let test_path = dir.path().join("test_file");
        std::fs::write(&test_path, "test content").unwrap();

        let db = Database::new().unwrap();

        // Initially hash should not exist
        assert!(!db.hash_exists("test_hash", &test_path).unwrap());

        // Add hash
        db.add_hash("test_hash", &test_path, true).unwrap();

        // Now hash should exist
        assert!(db.hash_exists("test_hash", &test_path).unwrap());

        // Different hash should not exist
        assert!(!db.hash_exists("different_hash", &test_path).unwrap());
    }
}
