pub mod desktop;
pub mod exec;
pub mod launch;
pub mod steam;
pub mod sync;

pub use exec::exec_game_command;
pub use launch::launch_game;
pub use sync::sync_game_saves;

pub(crate) fn resolve_ins_binary() -> std::path::PathBuf {
    crate::common::shell::resolve_current_binary()
}
