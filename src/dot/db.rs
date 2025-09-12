use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Represents the type of file a hash belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DotFileType {
    /// File in the dotfile repository (source)
    #[serde(rename = "true")]
    SourceFile,
    /// File in the home directory (target)
    #[serde(rename = "false")]
    TargetFile,
}

impl From<bool> for DotFileType {
    fn from(value: bool) -> Self {
        match value {
            true => DotFileType::SourceFile,
            false => DotFileType::TargetFile,
        }
    }
}

impl From<DotFileType> for bool {
    fn from(file_type: DotFileType) -> Self {
        match file_type {
            DotFileType::SourceFile => true,
            DotFileType::TargetFile => false,
        }
    }
}

/// Represents a hash with its creation timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHash {
    pub hash: String,
    pub created: DateTime<Utc>,
    pub path: String,
    #[serde(
        serialize_with = "serialize_file_type",
        deserialize_with = "deserialize_file_type"
    )]
    pub file_type: DotFileType,
}

fn serialize_file_type<S>(file_type: &DotFileType, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_bool((*file_type).into())
}

fn deserialize_file_type<'de, D>(deserializer: D) -> Result<DotFileType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let bool_val = bool::deserialize(deserializer)?;
    Ok(DotFileType::from(bool_val))
}

impl PartialEq for FileHash {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.path == other.path
    }
}

pub struct Database {
    conn: Connection,
}

const CURRENT_SCHEMA_VERSION: i32 = 2;

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
                    "CREATE TABLE IF NOT EXISTS file_hashes (
                        created TEXT NOT NULL,
                        hash TEXT NOT NULL,
                        path TEXT NOT NULL,
                        source_file INTEGER NOT NULL,
                        PRIMARY KEY (hash, path)
                    )",
                    (),
                )?;

                // Update to version 2
                conn.execute(
                    "INSERT INTO schema_version (version, updated) VALUES (2, datetime('now'))",
                    [],
                )?;
            }
            1 => {
                // Migration from version 1: drop old table and create new one
                conn.execute("DROP TABLE IF EXISTS hashes", ())?;
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS file_hashes (
                        created TEXT NOT NULL,
                        hash TEXT NOT NULL,
                        path TEXT NOT NULL,
                        source_file INTEGER NOT NULL,
                        PRIMARY KEY (hash, path)
                    )",
                    (),
                )?;

                // Update to version 2
                conn.execute(
                    "INSERT INTO schema_version (version, updated) VALUES (2, datetime('now'))",
                    [],
                )?;
            }
            // Future migrations can be added here
            _ => {}
        }
        Ok(())
    }

    pub fn add_hash(&self, hash: &str, path: &Path, file_type: DotFileType) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO file_hashes (created, hash, path, source_file) VALUES (datetime('now'), ?, ?, ?)",
            (hash, path.to_str().ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?, bool::from(file_type)),
        )?;
        Ok(())
    }

    pub fn hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM file_hashes WHERE hash = ? AND path = ?")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let mut result = stmt.query_map([hash, path_str], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    pub fn source_hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM file_hashes WHERE hash = ? AND path = ? AND source_file = 1")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let mut result = stmt.query_map([hash, path_str], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    pub fn target_hash_exists(&self, hash: &str, path: &Path) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM file_hashes WHERE hash = ? AND path = ? AND source_file = 0")?;
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?;
        let mut result = stmt.query_map([hash, path_str], |row| row.get::<_, i32>(0))?;
        Ok(result.next().is_some())
    }

    fn row_to_file_hash(row: &rusqlite::Row) -> Result<FileHash, rusqlite::Error> {
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

        Ok(FileHash {
            hash: row.get(0)?,
            created,
            path: row.get(2)?,
            file_type: DotFileType::from(row.get::<_, bool>(3)?),
        })
    }

    /// Get the newest hash for a file, if any exists
    pub fn get_newest_hash(&self, path: &Path) -> Result<Option<FileHash>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, created, path, source_file FROM file_hashes WHERE path = ? ORDER BY created DESC LIMIT 1")?;

        let result: Option<FileHash> = stmt
            .query_row(
                [path
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 path: {}", path.display()))?],
                Self::row_to_file_hash,
            )
            .optional()?;

        Ok(result)
    }

    pub fn cleanup_hashes(&self, days: u32) -> Result<()> {
        // Keep newest N hashes per target file (source_file = 0), but always keep all
        // source file hashes

        // Remove old target file hashes
        self.conn.execute(
            "DELETE FROM file_hashes WHERE source_file = 0 AND created < datetime('now', ?1 || ' days')",
            [days.to_string()],
        )?;

        // Keep newest hash per target file for rollback capability
        self.conn.execute(
            "DELETE FROM file_hashes WHERE source_file = 0 AND rowid NOT IN (
                SELECT MAX(rowid)
                FROM file_hashes
                WHERE source_file = 0
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

        // Add hash as source file
        db.add_hash("test_hash", &test_path, DotFileType::SourceFile)
            .unwrap();

        // Now hash should exist
        assert!(db.hash_exists("test_hash", &test_path).unwrap());

        // Different hash should not exist
        assert!(!db.hash_exists("different_hash", &test_path).unwrap());
    }

    #[test]
    fn test_source_hash_exists() {
        let dir = tempdir().unwrap();
        let test_path = dir.path().join("test_file");
        std::fs::write(&test_path, "test content").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();

        // Add hash as source file
        db.add_hash("test_hash", &test_path, DotFileType::SourceFile)
            .unwrap();

        // Source hash should exist
        assert!(db.source_hash_exists("test_hash", &test_path).unwrap());

        // Should not exist as target hash
        assert!(!db.target_hash_exists("test_hash", &test_path).unwrap());
    }

    #[test]
    fn test_target_hash_exists() {
        let dir = tempdir().unwrap();
        let test_path = dir.path().join("test_file");
        std::fs::write(&test_path, "test content").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();

        // Add hash as target file
        db.add_hash("test_hash", &test_path, DotFileType::TargetFile)
            .unwrap();

        // Target hash should exist
        assert!(db.target_hash_exists("test_hash", &test_path).unwrap());

        // Should not exist as source hash
        assert!(!db.source_hash_exists("test_hash", &test_path).unwrap());
    }
}
