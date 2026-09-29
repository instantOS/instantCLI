//! Health checks for `ins doctor`.

use crate::doctor::{CheckStatus, DoctorCheck, PrivilegeLevel};

pub mod audio;
pub mod completions;
pub mod display;
pub mod locale;
pub mod nerdfont;
pub mod network;
pub mod security;
pub mod session;
pub mod storage;
pub mod system;
pub mod tools;

pub use audio::PipewireSessionManagerCheck;
pub use completions::{ShellCompletionCheck, ZshHealthCheck};
pub use display::{SwayDisplayCheck, SwaySetupCheck};
pub use locale::LocaleCheck;
pub use nerdfont::NerdFontCheck;
pub use network::{InstantRepoCheck, InternetCheck, PacmanMirrorCheck};
pub use security::{FaillockCheck, PolkitAgentCheck, SshAuthSockCheck};
pub use session::SessionEnvironmentCheck;
pub use storage::{
    PacmanCacheCheck, PacmanDbSyncCheck, PacmanStaleDownloadsCheck, SmartHealthCheck,
    SteamCompatdataOrphansCheck, TrashBinSizeCheck, YayCacheCheck,
};
pub use system::{ClockSynchronizationCheck, PendingUpdatesCheck, SwapCheck};
pub use tools::{BatCheck, FzfVersionCheck, GitConfigCheck};
