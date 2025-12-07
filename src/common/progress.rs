use indicatif::{ProgressBar, ProgressStyle};

pub fn create_spinner(message: String) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap()
            .tick_chars("⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚"),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

/// Finish a spinner and print a success message with a checkmark
/// This clears the spinner line entirely and prints a clean message
pub fn finish_spinner_with_success(pb: ProgressBar, message: impl Into<String>) {
    pb.finish_and_clear();
    println!("✓ {}", message.into());
}
