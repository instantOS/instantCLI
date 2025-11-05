pub mod add;
pub mod apply;
pub mod reset;

// Re-export main functions for convenience
pub use add::add_dotfile;
pub use apply::apply_all;
pub use reset::reset_modified;
