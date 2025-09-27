use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

use crate::fzf_wrapper::FzfSelectable;

/// Serializable menu item with rich preview support
///
/// This struct enables rich menu items with preview functionality that can be
/// serialized and transmitted between client and server. It implements the
/// FzfSelectable trait for seamless integration with the fzf wrapper.
///
/// # Examples
///
/// Basic item with text preview:
/// ```rust
/// use crate::menu::protocol::{SerializableMenuItem, FzfPreview};
///
/// let item = SerializableMenuItem {
///     display_text: "Edit Configuration".to_string(),
///     preview: FzfPreview::Text("Opens the configuration file in your editor".to_string()),
///     metadata: None,
/// };
/// ```
///
/// Item with command preview:
/// ```rust
/// use crate::menu::protocol::{SerializableMenuItem, FzfPreview};
/// use std::collections::HashMap;
///
/// let mut metadata = HashMap::new();
/// metadata.insert("file".to_string(), "/path/to/config".to_string());
///
/// let item = SerializableMenuItem {
///     display_text: "View Logs".to_string(),
///     preview: FzfPreview::Command("tail -n 50 /var/log/app.log".to_string()),
///     metadata: Some(metadata),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableMenuItem {
    /// Text that appears in the fzf selection list
    pub display_text: String,
    /// Preview content shown in the preview window
    pub preview: FzfPreview,
    /// Optional metadata for the item
    pub metadata: Option<HashMap<String, String>>,
}

/// Re-export FzfPreview from fzf_wrapper for use in protocol
pub use crate::fzf_wrapper::FzfPreview;

impl FzfSelectable for SerializableMenuItem {
    fn fzf_display_text(&self) -> String {
        self.display_text.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.display_text.clone()
    }
}

/// Menu request types sent from client to server
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MenuRequest {
    /// Show confirmation dialog
    Confirm { message: String },
    /// Show selection menu with rich item support
    Choice {
        prompt: String,
        items: Vec<SerializableMenuItem>,
        multi: bool,
    },
    /// Show text input dialog
    Input { prompt: String },
    /// Get server status information
    Status,
    /// Stop the server
    Stop,
    /// Show the scratchpad without any other action
    Show,
}

/// Menu response types sent from server to client
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MenuResponse {
    /// Confirmation dialog result
    ConfirmResult(ConfirmResult),
    /// Selection menu result(s) with rich item metadata
    ChoiceResult(Vec<SerializableMenuItem>),
    /// Text input result
    InputResult(String),
    /// Server status information
    StatusResult(StatusInfo),
    /// Server stop acknowledgment
    StopResult,
    /// Error occurred
    Error(String),
    /// Operation was cancelled
    Cancelled,
    /// Show operation completed successfully
    ShowResult,
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

/// Detailed server status information
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusInfo {
    /// Current server status
    pub status: ServerStatus,
    /// Server version information
    pub version: String,
    /// Protocol version
    pub protocol_version: String,
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Socket path
    pub socket_path: String,
    /// Number of processed requests
    pub requests_processed: u64,
    /// Server start time
    pub start_time: String,
    /// Window compositor type
    pub compositor: String,
}

/// Protocol version information
pub const PROTOCOL_VERSION: &str = "1.0";

/// Default socket path
pub fn default_socket_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{runtime_dir}/insmenu.sock")
    } else {
        // Fallback to /tmp if XDG_RUNTIME_DIR is not set
        "/tmp/insmenu.sock".to_string()
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

    format!("req_{timestamp}_{random}")
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
    use crate::fzf_wrapper::FzfPreview;

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

    #[test]
    fn test_serializable_menu_item_creation() {
        let item = SerializableMenuItem {
            display_text: "Test Item".to_string(),
            preview: FzfPreview::Text("Preview content".to_string()),
            metadata: None,
        };

        assert_eq!(item.fzf_display_text(), "Test Item");
        assert_eq!(item.fzf_key(), "Test Item");

        match item.fzf_preview() {
            FzfPreview::Text(text) => assert_eq!(text, "Preview content"),
            _ => panic!("Expected text preview"),
        }
    }

    #[test]
    fn test_rich_choice_request_serialization() {
        let items = vec![
            SerializableMenuItem {
                display_text: "Option 1".to_string(),
                preview: FzfPreview::Text("First option".to_string()),
                metadata: None,
            },
            SerializableMenuItem {
                display_text: "Option 2".to_string(),
                preview: FzfPreview::Command("echo 'Second option'".to_string()),
                metadata: None,
            },
        ];

        let request = MenuRequest::Choice {
            prompt: "Select an option:".to_string(),
            items,
            multi: false,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: MenuRequest = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, MenuRequest::Choice { prompt, items, multi: false }
                if prompt == "Select an option:" && items.len() == 2)
        );
    }

    #[test]
    fn test_choice_response_serialization() {
        let items = vec![SerializableMenuItem {
            display_text: "Selected Item".to_string(),
            preview: FzfPreview::None,
            metadata: None,
        }];

        let response = MenuResponse::ChoiceResult(items);

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: MenuResponse = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, MenuResponse::ChoiceResult(items) if items.len() == 1));
    }

    #[test]
    fn test_menu_item_with_metadata() {
        use std::collections::HashMap;

        let mut metadata = HashMap::new();
        metadata.insert("file".to_string(), "/path/to/file".to_string());
        metadata.insert("type".to_string(), "config".to_string());

        let item = SerializableMenuItem {
            display_text: "Config File".to_string(),
            preview: FzfPreview::Command("cat /path/to/file".to_string()),
            metadata: Some(metadata),
        };

        assert_eq!(item.fzf_display_text(), "Config File");

        match item.fzf_preview() {
            FzfPreview::Command(cmd) => assert_eq!(cmd, "cat /path/to/file"),
            _ => panic!("Expected command preview"),
        }

        assert!(item.metadata.is_some());
        let metadata = item.metadata.unwrap();
        assert_eq!(metadata.get("file"), Some(&"/path/to/file".to_string()));
        assert_eq!(metadata.get("type"), Some(&"config".to_string()));
    }
}
