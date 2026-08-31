#[derive(Debug, PartialEq)]
pub enum SyncAction {
    /// No action needed (already in sync within tolerance)
    NoActionNeeded,
    /// Create backup (local saves are newer)
    CreateBackup,
    /// Restore from snapshot (snapshot is newer)
    RestoreFromSnapshot(String),
    /// No local saves, restore from latest snapshot
    RestoreFromLatest(String),
    /// No snapshots, create initial backup
    CreateInitialBackup,
    /// Restore skipped due to matching checkpoint
    RestoreSkipped(String),
    /// Backup skipped due to matching checkpoint
    BackupSkipped(String),
    /// Skipped due to being within tolerance window
    WithinTolerance {
        direction: ToleranceDirection,
        delta_seconds: i64,
    },
    /// Error condition
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToleranceDirection {
    LocalNewer,
    SnapshotNewer,
}

/// Outcome status for a single game's sync
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameSyncStatus {
    /// Backup or restore completed
    Synced,
    /// No action taken (in sync, within tolerance, or checkpoint skip)
    Skipped,
    /// Sync failed with an error message
    Failed(String),
}

/// Per-game result recorded during a sync run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSyncOutcome {
    pub game: String,
    pub status: GameSyncStatus,
}
