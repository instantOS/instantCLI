#[derive(Debug, PartialEq)]
pub enum SyncAction {
    NoActionNeeded,
    CreateBackup(String),
    RestoreFromSnapshot(String),
    ResolveNewest,
    AcknowledgeSnapshot { id: String, content: String },
}

/// Outcome status for a single game's sync
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameSyncStatus {
    /// Local saves were pushed to the repository
    BackedUp,
    /// A snapshot was restored to this device
    Restored,
    BackedUpNewest(String),
    RestoredNewest(String),
    /// Local contents already match the synchronized baseline
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
