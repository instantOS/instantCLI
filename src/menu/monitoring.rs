use crate::common::compositor::CompositorType;
use crate::scratchpad::{config::ScratchpadConfig, visibility::is_scratchpad_visible};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

/// Result of visibility monitoring
#[derive(Debug)]
pub enum MonitoringResult {
    /// Scratchpad became invisible
    Invisible,
    /// Monitoring was stopped externally
    Stopped,
}

/// Configuration for visibility monitoring
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// How often to check visibility (in milliseconds)
    pub check_interval_ms: u64,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            check_interval_ms: 100, // Check every 100ms
        }
    }
}

/// Visibility monitoring system for scratchpad
pub struct VisibilityMonitor {
    config: MonitoringConfig,
}

impl VisibilityMonitor {
    /// Create a new visibility monitor with default configuration
    pub fn new() -> Self {
        Self {
            config: MonitoringConfig::default(),
        }
    }

    /// Create a new visibility monitor with custom configuration
    pub fn with_config(config: MonitoringConfig) -> Self {
        Self { config }
    }

    /// Start monitoring scratchpad visibility in a background thread
    ///
    /// Returns a handle to join the monitoring thread and channels for communication:
    /// - `result_rx`: Receives MonitoringResult when monitoring stops
    /// - `stop_tx`: Send to this channel to stop monitoring externally
    pub fn start_monitoring(
        &self,
        compositor: CompositorType,
        scratchpad_config: ScratchpadConfig,
    ) -> (
        thread::JoinHandle<()>,
        mpsc::Receiver<MonitoringResult>,
        mpsc::Sender<()>,
    ) {
        let (result_tx, result_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();

        let check_interval = Duration::from_millis(self.config.check_interval_ms);

        let handle = thread::spawn(move || {
            loop {
                // Check if we should stop monitoring
                if stop_rx.try_recv().is_ok() {
                    let _ = result_tx.send(MonitoringResult::Stopped);
                    break;
                }

                // Check scratchpad visibility
                match is_scratchpad_visible(&compositor, &scratchpad_config) {
                    Ok(false) => {
                        // Scratchpad became invisible
                        let _ = result_tx.send(MonitoringResult::Invisible);
                        break;
                    }
                    Ok(true) => {
                        // Still visible, continue monitoring
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to check scratchpad visibility during monitoring: {e}"
                        );
                        // Continue monitoring despite error
                    }
                }

                thread::sleep(check_interval);
            }
        });

        (handle, result_rx, stop_tx)
    }

    /// Convenience method to start monitoring with atomic flag control
    ///
    /// This version uses an atomic boolean for control instead of channels,
    /// which can be more convenient in some scenarios.
    pub fn start_monitoring_with_flag(
        &self,
        compositor: CompositorType,
        scratchpad_config: ScratchpadConfig,
        monitoring_active: Arc<AtomicBool>,
    ) -> (thread::JoinHandle<()>, mpsc::Receiver<MonitoringResult>) {
        let (result_tx, result_rx) = mpsc::channel();

        let check_interval = Duration::from_millis(self.config.check_interval_ms);

        let handle = thread::spawn(move || {
            while monitoring_active.load(Ordering::SeqCst) {
                // Check scratchpad visibility
                match is_scratchpad_visible(&compositor, &scratchpad_config) {
                    Ok(false) => {
                        // Scratchpad became invisible
                        let _ = result_tx.send(MonitoringResult::Invisible);
                        break;
                    }
                    Ok(true) => {
                        // Still visible, continue monitoring
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to check scratchpad visibility during monitoring: {e}"
                        );
                        // Continue monitoring despite error
                    }
                }

                thread::sleep(check_interval);
            }

            // If we exit due to flag being false, send stopped result
            if !monitoring_active.load(Ordering::SeqCst) {
                let _ = result_tx.send(MonitoringResult::Stopped);
            }
        });

        (handle, result_rx)
    }
}

impl Default for VisibilityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_monitoring_config_default() {
        let config = MonitoringConfig::default();
        assert_eq!(config.check_interval_ms, 100);
    }

    #[test]
    fn test_visibility_monitor_creation() {
        let monitor = VisibilityMonitor::new();
        assert_eq!(monitor.config.check_interval_ms, 100);

        let custom_config = MonitoringConfig {
            check_interval_ms: 50,
        };
        let custom_monitor = VisibilityMonitor::with_config(custom_config);
        assert_eq!(custom_monitor.config.check_interval_ms, 50);
    }

    #[test]
    fn test_monitoring_can_be_stopped() {
        let monitor = VisibilityMonitor::new();
        let compositor = CompositorType::Other("test".to_string());
        let scratchpad_config = ScratchpadConfig::default();

        let (handle, result_rx, stop_tx) = monitor.start_monitoring(compositor, scratchpad_config);

        // Stop monitoring immediately
        let _ = stop_tx.send(());

        // Wait for result
        let start = Instant::now();
        if let Ok(result) = result_rx.recv_timeout(Duration::from_secs(1)) {
            match result {
                MonitoringResult::Stopped => {
                    // This is expected
                }
                MonitoringResult::Invisible => {
                    // This could happen if the test compositor reports invisible
                }
            }
        }

        // Should complete quickly
        assert!(start.elapsed() < Duration::from_secs(1));

        let _ = handle.join();
    }
}
