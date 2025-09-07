use rusqlite::Connection;
use anyhow::Result;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
        let db_path = super::config::db_path()?;
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS hashes (
                created TEXT NOT NULL,
                hash TEXT NOT NULL,
                path TEXT NOT NULL,
                valid INTEGER NOT NULL,
                PRIMARY KEY (hash, path)
            )",
            (),
        )?;
        Ok(Database { conn })
    }

    pub fn add_hash(&self, hash: &str, path: &Path, valid: bool) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO hashes (created, hash, path, valid) VALUES (datetime('now'), ?, ?, ?)",
            (hash, path.to_str().unwrap(), valid),
        )?;
        Ok(())
    }

    pub fn get_valid_hashes(&self, path: &Path) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT hash FROM hashes WHERE path = ? AND valid = 1")?;
        let hashes = stmt.query_map([path.to_str().unwrap()], |row| row.get(0))?;
        let mut result = Vec::new();
        for hash in hashes {
            result.push(hash?);
        }
        Ok(result)
    }

    pub fn cleanup_hashes(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM hashes WHERE valid = 0 AND hash NOT IN (
                SELECT hash FROM hashes WHERE valid = 0 ORDER BY created DESC LIMIT 1
            )",
            (),
        )?;
        Ok(())
    }
}