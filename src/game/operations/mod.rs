pub mod desktop;
pub mod exec;
pub mod launch;
pub mod steam;
pub mod sync;

pub use desktop::{add_game_to_desktop, add_menu_to_desktop, remove_game_from_desktop};
pub use exec::exec_game_command;
pub use launch::launch_game;
pub use sync::sync_game_saves;
