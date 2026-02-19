pub mod desktop;
pub mod exec;
pub mod launch;
pub mod steam;
pub mod sync;

pub use exec::exec_game_command;
pub use launch::launch_game;
pub use sync::sync_game_saves;

use std::path::PathBuf;

/// Get the real path to the ins binary.
/// When running as an AppImage, `std::env::current_exe()` resolves to a
/// temporary FUSE mount (e.g. `/tmp/.mount_xxx/usr/bin/ins`) which becomes
/// invalid once the process exits. The `APPIMAGE` env var holds the actual
/// `.AppImage` file path that persists across launches.
pub(crate) fn resolve_ins_binary() -> PathBuf {
    if let Ok(appimage_path) = std::env::var("APPIMAGE") {
        return PathBuf::from(appimage_path);
    }
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ins"))
}
