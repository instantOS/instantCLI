use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Represents a hash with its creation timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotfileHash {
    pub hash: String,
    pub created: DateTime<Utc>,
    pub path: String,
    pub unmodified: bool,
}

impl PartialEq for DotfileHash {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.path == other.path
    }
}

pub struct Database {
    conn: Connection,
}

const CURRENT_SCHEMA_VERSION: i32 = 1;

impl Database {
    pub fn new(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;

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
            (hash, path.to_str().ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?, unmodified),
        )?;
        Ok(())
    }

    pub fn hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM hashes WHERE hash = ? AND path = ?")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let mut result = stmt.query_map([hash, path_str], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    pub fn unmodified_hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM hashes WHERE hash = ? AND path = ? AND unmodified = 1")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let mut result = stmt.query_map([hash, path_str], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    fn row_to_dotfile_hash(row: &rusqlite::Row) -> Result<DotfileHash, rusqlite::Error> {
        let created_str: String = row.get(1)?;

        // Try to parse as SQLite datetime format first (YYYY-MM-DD HH:MM:SS)
        let created = if let Ok(dt) =
            chrono::NaiveDateTime::parse_from_str(&created_str, "%Y-%m-%d %H:%M:%S")
        {
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)
        } else if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&created_str) {
            // Fallback to RFC3339 format
            dt.with_timezone(&Utc)
        } else {
            return Err(rusqlite::Error::InvalidColumnType(
                1,
                "created".to_string(),
                rusqlite::types::Type::Text,
            ));
        };

        Ok(DotfileHash {
            hash: row.get(0)?,
            created,
            path: row.get(2)?,
            unmodified: row.get(3)?,
        })
    }

    pub fn get_unmodified_hashes(&self, path: &Path) -> Result<Vec<DotfileHash>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, created, path, unmodified FROM hashes WHERE path = ? AND unmodified = 1 ORDER BY created DESC")?;

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let hashes = stmt.query_map([path_str], Self::row_to_dotfile_hash)?;

        let mut result = Vec::new();
        for hash in hashes {
            result.push(hash?);
        }
        Ok(result)
    }

    /// Get the newest hash for a file, if any exists
    pub fn get_newest_hash(&self, path: &Path) -> Result<Option<DotfileHash>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, created, path, unmodified FROM hashes WHERE path = ? ORDER BY created DESC LIMIT 1")?;

        let result: Option<DotfileHash> = stmt
            .query_row(
                [path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?],
                Self::row_to_dotfile_hash,
            )
            .optional()?;

        Ok(result)
    }

    pub fn cleanup_hashes(&self, days: u32) -> Result<()> {
        // Keep all unmodified hashes, and for modified hashes:
        // 1. Keep the newest modified hash per file (for rollback capability)
        // 2. Remove modified hashes older than the configured number of days

        // First, remove modified hashes older than the configured number of days
        self.conn.execute(
            "DELETE FROM hashes WHERE unmodified = 0 AND created < datetime('now', ?1 || ' days')",
            [days.to_string()],
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
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn test_hash_exists() {
        let dir = tempdir().unwrap();
        let test_path = dir.path().join("test_file");
        std::fs::write(&test_path, "test content").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();

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
