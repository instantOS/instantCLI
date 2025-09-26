use super::protocol::*;
use crate::fzf_wrapper::FzfWrapper;
use anyhow::Result;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// Handles processing of different menu request types
pub struct RequestProcessor {
    running: Arc<AtomicBool>,
    requests_processed: Arc<AtomicU64>,
}

impl RequestProcessor {
    /// Create a new request processor
    pub fn new(running: Arc<AtomicBool>, requests_processed: Arc<AtomicU64>) -> Self {
        Self {
            running,
            requests_processed,
        }
    }

    /// Process a menu request without any monitoring or scratchpad management
    pub fn process_internal(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Increment request counter
        self.requests_processed.fetch_add(1, Ordering::SeqCst);

        match request {
            MenuRequest::Confirm { message } => self.handle_confirm_request(message),
            MenuRequest::Choice {
                prompt,
                items,
                multi,
            } => self.handle_choice_request(prompt, items, multi),
            MenuRequest::Input { prompt } => self.handle_input_request(prompt),
            MenuRequest::Status => Ok(self.get_status_info()),
            MenuRequest::Stop => self.handle_stop_request(),
            MenuRequest::Show => Ok(MenuResponse::ShowResult),
        }
    }

    /// Handle confirm dialog request
    fn handle_confirm_request(&self, message: String) -> Result<MenuResponse> {
        match FzfWrapper::confirm(&message) {
            Ok(result) => Ok(MenuResponse::ConfirmResult(result.into())),
            Err(e) => Ok(MenuResponse::Error(format!(
                "Failed to show confirm dialog: {e}"
            ))),
        }
    }

    /// Handle choice selection request
    fn handle_choice_request(
        &self,
        prompt: String,
        items: Vec<SerializableMenuItem>,
        multi: bool,
    ) -> Result<MenuResponse> {
        if items.is_empty() {
            return Ok(MenuResponse::Error("No items to choose from".to_string()));
        }

        match FzfWrapper::builder()
            .prompt(prompt)
            .multi_select(multi)
            .select(items)?
        {
            crate::fzf_wrapper::FzfResult::Selected(item) => {
                Ok(MenuResponse::ChoiceResult(vec![item]))
            }
            crate::fzf_wrapper::FzfResult::MultiSelected(items) => {
                Ok(MenuResponse::ChoiceResult(items))
            }
            crate::fzf_wrapper::FzfResult::Cancelled => Ok(MenuResponse::Cancelled),
            crate::fzf_wrapper::FzfResult::Error(e) => {
                Ok(MenuResponse::Error(format!("Selection error: {e}")))
            }
        }
    }

    /// Handle text input request
    fn handle_input_request(&self, prompt: String) -> Result<MenuResponse> {
        match FzfWrapper::input(&prompt) {
            Ok(input) => Ok(MenuResponse::InputResult(input)),
            Err(e) => Ok(MenuResponse::Error(format!(
                "Failed to show input dialog: {e}"
            ))),
        }
    }

    /// Handle server stop request
    fn handle_stop_request(&self) -> Result<MenuResponse> {
        // Signal the server to stop
        self.running.store(false, Ordering::SeqCst);
        Ok(MenuResponse::StopResult)
    }

    /// Get server status information
    fn get_status_info(&self) -> MenuResponse {
        // For the processor, we don't have access to all server info,
        // so we return a simplified status
        let status = if self.running.load(Ordering::SeqCst) {
            ServerStatus::Ready
        } else {
            ServerStatus::ShuttingDown
        };

        let status_info = StatusInfo {
            status,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION.to_string(),
            uptime_seconds: 0,              // Not available at processor level
            socket_path: "N/A".to_string(), // Not available at processor level
            requests_processed: self.requests_processed.load(Ordering::SeqCst),
            start_time: "N/A".to_string(), // Not available at processor level
            compositor: "N/A".to_string(), // Not available at processor level
        };

        MenuResponse::StatusResult(status_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_processor_creation() {
        let running = Arc::new(AtomicBool::new(true));
        let requests_processed = Arc::new(AtomicU64::new(0));
        let processor = RequestProcessor::new(running, requests_processed);

        // Basic smoke test
        let request = MenuRequest::Status;
        let response = processor.process_internal(request);
        assert!(response.is_ok());
    }

    #[test]
    fn test_stop_request() {
        let running = Arc::new(AtomicBool::new(true));
        let requests_processed = Arc::new(AtomicU64::new(0));
        let processor = RequestProcessor::new(running.clone(), requests_processed);

        assert!(running.load(Ordering::SeqCst));

        let request = MenuRequest::Stop;
        let response = processor.process_internal(request);

        assert!(response.is_ok());
        assert!(!running.load(Ordering::SeqCst));
    }

    #[test]
    fn test_request_counter() {
        let running = Arc::new(AtomicBool::new(true));
        let requests_processed = Arc::new(AtomicU64::new(0));
        let processor = RequestProcessor::new(running, requests_processed.clone());

        assert_eq!(requests_processed.load(Ordering::SeqCst), 0);

        let request = MenuRequest::Status;
        let _ = processor.process_internal(request);

        assert_eq!(requests_processed.load(Ordering::SeqCst), 1);
    }
}
