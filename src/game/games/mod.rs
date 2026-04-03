pub mod add;
mod add_discovery;
pub mod discover;
pub mod display;
pub mod manager;
pub mod prompts;
pub mod relocate;
pub mod remove;
pub mod selection;
pub mod validation;

pub use add::AddGameOptions;
pub use manager::GameManager;
pub use remove::remove_game;
