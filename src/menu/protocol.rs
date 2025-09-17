use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Menu request types sent from client to server
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MenuRequest {
    /// Show confirmation dialog
    Confirm { message: String },
    /// Show selection menu
    Choice {
        prompt: String,
        items: Vec<String>,
        multi: bool,
    },
    /// Show text input dialog
    Input { prompt: String },
}

/// Menu response types sent from server to client
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MenuResponse {
    /// Confirmation dialog result
    ConfirmResult(ConfirmResult),
    /// Selection menu result(s)
    ChoiceResult(Vec<String>),
    /// Text input result
    InputResult(String),
    /// Error occurred
    Error(String),
    /// Operation was cancelled
    Cancelled,
}

/// Confirmation dialog result
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConfirmResult {
    /// User confirmed
    Yes,
    /// User declined
    No,
    /// User cancelled
    Cancelled,
}

/// Message envelope for requests
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MenuMessage {
    /// Unique request identifier
    pub request_id: String,
    /// The actual request payload
    pub payload: MenuRequest,
    /// Timestamp when request was sent
    pub timestamp: SystemTime,
}

/// Message envelope for responses
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MenuResponseMessage {
    /// Corresponding request identifier
    pub request_id: String,
    /// The response payload
    pub payload: MenuResponse,
    /// Timestamp when response was sent
    pub timestamp: SystemTime,
}

/// Server status information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerStatus {
    /// Server is ready to accept requests
    Ready,
    /// Server is busy processing a request
    Busy,
    /// Server is shutting down
    ShuttingDown,
}

/// Protocol version information
pub const PROTOCOL_VERSION: &str = "1.0";

/// Default socket path
pub fn default_socket_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{}/instantmenu.sock", runtime_dir)
    } else {
        // Fallback to /tmp if XDG_RUNTIME_DIR is not set
        "/tmp/instantmenu.sock".to_string()
    }
}

/// Generate a unique request ID
pub fn generate_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let random: u32 = rand::random();

    format!("req_{}_{}", timestamp, random)
}

/// Convert FZF confirmation result to protocol result
impl From<crate::fzf_wrapper::ConfirmResult> for ConfirmResult {
    fn from(result: crate::fzf_wrapper::ConfirmResult) -> Self {
        match result {
            crate::fzf_wrapper::ConfirmResult::Yes => ConfirmResult::Yes,
            crate::fzf_wrapper::ConfirmResult::No => ConfirmResult::No,
            crate::fzf_wrapper::ConfirmResult::Cancelled => ConfirmResult::Cancelled,
        }
    }
}

/// Convert protocol confirmation result to exit code
impl From<ConfirmResult> for i32 {
    fn from(result: ConfirmResult) -> Self {
        match result {
            ConfirmResult::Yes => 0,       // Yes
            ConfirmResult::No => 1,        // No
            ConfirmResult::Cancelled => 2, // Cancelled
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = MenuRequest::Confirm {
            message: "Are you sure?".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: MenuRequest = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, MenuRequest::Confirm { message } if message == "Are you sure?")
        );
    }

    #[test]
    fn test_response_serialization() {
        let response = MenuResponse::ConfirmResult(ConfirmResult::Yes);

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: MenuResponse = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            MenuResponse::ConfirmResult(ConfirmResult::Yes)
        ));
    }

    #[test]
    fn test_message_envelope() {
        let message = MenuMessage {
            request_id: "test_123".to_string(),
            payload: MenuRequest::Input {
                prompt: "Enter value:".to_string(),
            },
            timestamp: SystemTime::now(),
        };

        let json = serde_json::to_string(&message).unwrap();
        let deserialized: MenuMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.request_id, "test_123");
        assert!(
            matches!(deserialized.payload, MenuRequest::Input { prompt } if prompt == "Enter value:")
        );
    }
}
