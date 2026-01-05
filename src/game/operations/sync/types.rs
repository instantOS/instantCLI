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
