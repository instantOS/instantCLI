use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::menu_utils::{FzfSelectable, default_fzf_key};

pub use super::bindings::Binding;

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
    /// Stable selection key separate from the rendered label
    #[serde(default)]
    pub key: Option<String>,
    /// Text that appears in the fzf selection list
    pub display_text: String,
    /// Preview content shown in the preview window
    pub preview: FzfPreview,
    /// Optional metadata for the item
    pub metadata: Option<HashMap<String, String>>,
}

/// Re-export types from menu wrapper for use in protocol
pub use crate::menu_utils::{ConfirmResult, FilePickerScope, FzfPreview};

impl FzfSelectable for SerializableMenuItem {
    fn fzf_display_text(&self) -> String {
        self.display_text.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.key
            .clone()
            .unwrap_or_else(|| default_fzf_key(&self.display_text))
    }
}

/// Slider configuration transmitted between client and server
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SliderRequest {
    /// Minimum slider value
    pub min: i64,
    /// Maximum slider value
    pub max: i64,
    /// Optional initial value
    pub value: Option<i64>,
    /// Small step increment
    pub step: Option<i64>,
    /// Large step increment
    pub big_step: Option<i64>,
    /// Optional label to display above the slider
    pub label: Option<String>,
    /// Command argv to execute on change (value appended)
    pub command: Vec<String>,
}

/// Configuration shared by buffered and streaming choices.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChoiceOptions {
    /// Text shown before the menu query.
    pub prompt: String,
    /// Whether the user may submit more than one item.
    pub allow_multiple: bool,
    /// Namespace used to rank and remember selections.
    pub frecency_cache: Option<String>,
    /// Alternative actions that can accept the current selection.
    pub bindings: Vec<Binding>,
}

impl ChoiceOptions {
    /// Create single-select choice options without frecency or bindings.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            allow_multiple: false,
            frecency_cache: None,
            bindings: Vec::new(),
        }
    }

    /// Configure whether the user may submit more than one item.
    pub fn multi_select(mut self, allow_multiple: bool) -> Self {
        self.allow_multiple = allow_multiple;
        self
    }

    /// Set the optional frecency namespace.
    pub fn with_frecency_cache(mut self, frecency_cache: Option<String>) -> Self {
        self.frecency_cache = frecency_cache;
        self
    }

    /// Set the actions available alongside ordinary submission.
    pub fn with_bindings(mut self, bindings: Vec<Binding>) -> Self {
        self.bindings = bindings;
        self
    }
}

/// Input kind: visible text or hidden password. The split is structural so
/// that a prefilled password cannot be expressed: only [`InputKind::Text`]
/// carries `initial_text`.
///
/// Note: `prompt`/`placeholder`/`initial_text` are passed to `instantmenu` as
/// argv and are visible in `ps`; never put secrets there. `Password` carries
/// no prefill precisely for this reason.
///
/// Wire shape is flat (`#[serde(flatten)]` on [`InputOptions::kind`]):
/// `{"prompt":"q","kind":"text","initial_text":"hi"}` vs
/// `{"prompt":"p","kind":"password"}`. A `password` payload carrying
/// `initial_text` is rejected on deserialize (fail-closed), not silently
/// dropped.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputKind {
    /// Visible text, optionally pre-filled.
    Text {
        /// Text pre-filled into the input field.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_text: Option<String>,
    },
    /// Hidden input; never pre-filled.
    Password,
}

impl<'de> Deserialize<'de> for InputKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Helper mirrors the flat shape. No `deny_unknown_fields`: when
        // flattened into `InputOptions`, sibling fields (`prompt`,
        // `placeholder`) share the same map and must be ignored here.
        #[derive(Deserialize)]
        struct Helper {
            kind: String,
            #[serde(default)]
            initial_text: Option<String>,
        }

        let helper = Helper::deserialize(deserializer)?;
        match helper.kind.as_str() {
            "text" => Ok(InputKind::Text {
                initial_text: helper.initial_text.filter(|s| !s.is_empty()),
            }),
            "password"
                if helper
                    .initial_text
                    .as_deref()
                    .is_some_and(|s| !s.is_empty()) =>
            {
                Err(serde::de::Error::custom(
                    "password inputs must not carry `initial_text`",
                ))
            }
            "password" => Ok(InputKind::Password),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["text", "password"],
            )),
        }
    }
}

impl InputKind {
    /// Visible text without prefill.
    pub fn text() -> Self {
        Self::Text { initial_text: None }
    }

    /// Visible text pre-filled with the given value. Empty values normalize
    /// to no prefill so `Some("")` never reaches the wire or argv.
    pub fn text_with_initial(initial_text: impl Into<String>) -> Self {
        let text = initial_text.into();
        if text.is_empty() {
            Self::text()
        } else {
            Self::Text {
                initial_text: Some(text),
            }
        }
    }
}

/// Deserialize `Option<String>`, normalizing empty strings to `None` so
/// `""` from the wire matches the constructor behavior (`with_placeholder("")`
/// stays off the wire).
fn empty_string_as_none<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

/// Configuration shared by text and password prompts.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InputOptions {
    /// Text shown before the input query.
    pub prompt: String,
    /// Which input variant to show: visible text or hidden password.
    /// Flattened so the wire is `{"prompt","kind","initial_text?","placeholder?"}`.
    #[serde(flatten)]
    pub kind: InputKind,
    /// Faded text shown while the input is empty.
    #[serde(
        default,
        deserialize_with = "empty_string_as_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub placeholder: Option<String>,
}

impl InputOptions {
    /// Create a plain text input prompt.
    pub fn text(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            kind: InputKind::text(),
            placeholder: None,
        }
    }

    /// Create a text prompt pre-filled with the given value.
    pub fn text_with_initial(prompt: impl Into<String>, initial_text: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            kind: InputKind::text_with_initial(initial_text),
            placeholder: None,
        }
    }

    /// Create a hidden password prompt.
    pub fn password(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            kind: InputKind::Password,
            placeholder: None,
        }
    }

    /// Whether the input must be hidden.
    pub fn is_secret(&self) -> bool {
        matches!(self.kind, InputKind::Password)
    }

    /// Set the faded hint shown while the input is empty. Empty values
    /// normalize to no placeholder so `--placeholder ""` stays off the wire.
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        let text = placeholder.into();
        self.placeholder = if text.is_empty() { None } else { Some(text) };
        self
    }
}

/// Menu request types sent from client to server
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MenuRequest {
    /// Show a selection menu with an already available item corpus.
    Choice {
        options: ChoiceOptions,
        items: Vec<SerializableMenuItem>,
    },
    /// Show confirmation dialog
    Confirm { message: String },
    /// Start a streaming selection menu. First frame on a connection;
    /// server opens the menu immediately, then receives
    /// `ChoiceChunk` frames and a final `ChoiceEnd`.
    ChoiceBegin { options: ChoiceOptions },
    /// Batch of items for an in-progress streaming choice.
    /// Same `request_id` as the opening `ChoiceBegin`.
    ChoiceChunk { items: Vec<SerializableMenuItem> },
    /// End of stream — server keeps the menu open for selection.
    ChoiceEnd,
    /// Show chord navigator using provided chord definitions
    Chord { chords: Vec<String> },
    /// Show text or password input dialog
    Input { options: InputOptions },
    /// Launch file picker dialog
    FilePicker {
        start: Option<String>,
        scope: FilePickerScope,
        allow_multiple: bool,
    },
    /// Show slider interface
    Slide(SliderRequest),
    /// Show message dialog with OK button
    Message { title: String, message: String },
    /// Show an ephemeral toast notification popup
    Toast { message: String, duration: f64 },
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
    /// A submitted choice, including the optional action that accepted it.
    ChoiceResult {
        action: Option<String>,
        items: Vec<SerializableMenuItem>,
    },
    /// Request protocol does not match the running server.
    ProtocolMismatch { received: String, expected: String },
    /// Streaming choice renderer process has started, its initial input has
    /// been flushed, and the server may receive item chunks.
    ChoiceReady,
    /// Confirmation dialog result
    ConfirmResult(ConfirmResult),
    /// Chord selection result
    ChordResult(String),
    /// Text or password input result
    InputResult(String),
    /// Server status information
    StatusResult(MenuStatus),
    /// Server stop acknowledgment
    StopResult,
    /// Error occurred
    Error(String),
    /// Operation was cancelled
    Cancelled,
    /// File picker result paths
    FilePickerResult(Vec<PathBuf>),
    /// Show operation completed successfully
    ShowResult,
    /// Slider result value
    SlideResult(i64),
    /// Message dialog acknowledged
    MessageResult,
    /// Toast notification completed
    ToastResult,
}

/// Message envelope for requests
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MenuMessage {
    /// Unique request identifier
    pub request_id: String,
    /// Wire protocol spoken by the sender.
    #[serde(default = "legacy_protocol_version")]
    pub protocol_version: String,
    /// The actual request payload
    pub payload: MenuRequest,
    /// Timestamp when request was sent
    pub timestamp: SystemTime,
}

impl MenuMessage {
    pub fn new(request_id: String, payload: MenuRequest) -> Self {
        Self {
            request_id,
            protocol_version: PROTOCOL_VERSION.to_string(),
            payload,
            timestamp: SystemTime::now(),
        }
    }
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
pub struct MenuStatus {
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
pub const PROTOCOL_VERSION: &str = "8.0";

fn legacy_protocol_version() -> String {
    "1.0".to_string()
}

/// Maximum number of decoded choice items waiting between a producer and
/// menu backend. Keeping this bounded propagates backpressure to stdin and
/// remote clients instead of buffering an arbitrarily large stream in RAM.
pub const STREAM_ITEM_BUFFER_CAPACITY: usize = 256;

/// Default socket path
pub fn default_socket_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{runtime_dir}/insmenu.sock")
    } else {
        // Fallback to the platform's temp dir if XDG_RUNTIME_DIR is not set.
        // `std::env::temp_dir()` honors $TMPDIR, which on Termux points to
        // $PREFIX/tmp instead of /tmp.
        std::env::temp_dir()
            .join("insmenu.sock")
            .to_string_lossy()
            .into_owned()
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

impl SerializableMenuItem {
    pub fn plain(display_text: impl Into<String>) -> Self {
        Self {
            key: None,
            display_text: display_text.into(),
            preview: FzfPreview::None,
            metadata: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::preview::FzfPreview;

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
        let message = MenuMessage::new(
            "test_123".to_string(),
            MenuRequest::Input {
                options: InputOptions::text("Enter value:"),
            },
        );

        let json = serde_json::to_string(&message).unwrap();
        let deserialized: MenuMessage = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.request_id, "test_123");
        assert_eq!(deserialized.protocol_version, PROTOCOL_VERSION);
        assert!(
            matches!(deserialized.payload, MenuRequest::Input { options } if options.prompt == "Enter value:" && !options.is_secret())
        );
    }

    #[test]
    fn test_input_options_password_round_trip() {
        let request = MenuRequest::Input {
            options: InputOptions::password("Enter password:"),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: MenuRequest = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, MenuRequest::Input { options } if options.prompt == "Enter password:" && options.is_secret())
        );
    }

    #[test]
    fn test_input_options_placeholder_and_initial_text_round_trip() {
        let request = MenuRequest::Input {
            options: InputOptions::text_with_initial("Edit value:", "current value")
                .with_placeholder("leave empty to clear"),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: MenuRequest = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            MenuRequest::Input { options }
                if options.placeholder.as_deref() == Some("leave empty to clear")
                    && matches!(
                        options.kind,
                        InputKind::Text { initial_text: ref t } if t.as_deref() == Some("current value")
                    )
        ));
    }

    /// A password payload carrying `initial_text` is rejected on deserialize
    /// (fail-closed), not silently dropped.
    #[test]
    fn test_input_options_prefill_cannot_leak_into_password() {
        let json = r#"{"prompt":"p","kind":"password","initial_text":"leaked"}"#;
        let err = serde_json::from_str::<InputOptions>(json).unwrap_err();
        assert!(err.to_string().contains("must not carry"), "{err}");

        // empty prefill normalizes to no prefill, including for passwords
        let empty: InputOptions =
            serde_json::from_str(r#"{"prompt":"p","kind":"password","initial_text":""}"#).unwrap();
        assert!(matches!(empty.kind, InputKind::Password));
        let empty_text: InputOptions =
            serde_json::from_str(r#"{"prompt":"q","kind":"text","initial_text":""}"#).unwrap();
        assert!(matches!(
            empty_text.kind,
            InputKind::Text { initial_text: None }
        ));

        // password payloads carry no prefill field at all
        let password_json =
            serde_json::to_string(&InputOptions::password("Enter password:")).unwrap();
        assert_eq!(
            password_json,
            r#"{"prompt":"Enter password:","kind":"password"}"#
        );
    }

    #[test]
    fn empty_initial_text_and_placeholder_normalize_to_none() {
        assert!(matches!(
            InputOptions::text_with_initial("q", "").kind,
            InputKind::Text { initial_text: None }
        ));
        assert!(matches!(
            InputKind::text_with_initial(""),
            InputKind::Text { initial_text: None }
        ));
        assert_eq!(
            InputOptions::text("q").with_placeholder("").placeholder,
            None
        );
        assert_eq!(
            serde_json::to_string(&InputOptions::text_with_initial("q", "")).unwrap(),
            r#"{"prompt":"q","kind":"text"}"#
        );
        // Empty placeholder on the wire normalizes like the constructors, so a
        // forwarded frame stays minimal instead of carrying `""`.
        let wire_empty: InputOptions =
            serde_json::from_str(r#"{"prompt":"q","kind":"text","placeholder":""}"#).unwrap();
        assert_eq!(wire_empty.placeholder, None);
        assert_eq!(
            serde_json::to_string(&wire_empty).unwrap(),
            r#"{"prompt":"q","kind":"text"}"#
        );
    }

    /// `kind` is required: missing, legacy `secret`, and nested `kind`
    /// shapes all fail closed instead of silently downgrading to visible text.
    #[test]
    fn test_input_options_kind_is_required() {
        for json in [
            r#"{"prompt":"Enter value:"}"#,
            r#"{"prompt":"x","secret":true}"#,
            r#"{"prompt":"x","secret":false}"#,
            r#"{"prompt":"q","kind":{"kind":"text"}}"#,
        ] {
            assert!(
                serde_json::from_str::<InputOptions>(json).is_err(),
                "should reject: {json}"
            );
        }

        // unset optionals are omitted so payloads stay minimal
        let plain = serde_json::to_string(&InputOptions::text("q")).unwrap();
        assert_eq!(plain, r#"{"prompt":"q","kind":"text"}"#);

        let prefilled = serde_json::to_string(
            &InputOptions::text_with_initial("q", "hi").with_placeholder("ph"),
        )
        .unwrap();
        assert_eq!(
            prefilled,
            r#"{"prompt":"q","kind":"text","initial_text":"hi","placeholder":"ph"}"#
        );
    }

    #[test]
    fn missing_request_protocol_is_identified_as_v1() {
        let message = MenuMessage::new("legacy".to_string(), MenuRequest::Status);
        let mut value = serde_json::to_value(message).unwrap();
        value.as_object_mut().unwrap().remove("protocol_version");

        let decoded: MenuMessage = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.protocol_version, "1.0");
    }

    #[test]
    fn test_serializable_menu_item_creation() {
        let item = SerializableMenuItem {
            key: None,
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
                key: None,
                display_text: "Option 1".to_string(),
                preview: FzfPreview::Text("First option".to_string()),
                metadata: None,
            },
            SerializableMenuItem {
                key: None,
                display_text: "Option 2".to_string(),
                preview: FzfPreview::Command("echo 'Second option'".to_string()),
                metadata: None,
            },
        ];

        let request = MenuRequest::Choice {
            options: ChoiceOptions::new("Select an option:")
                .with_frecency_cache(Some("applications".to_string())),
            items,
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: MenuRequest = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, MenuRequest::Choice { options, items }
                if options.prompt == "Select an option:"
                    && !options.allow_multiple
                    && options.frecency_cache.as_deref() == Some("applications")
                    && options.bindings.is_empty()
                    && items.len() == 2)
        );
    }

    #[test]
    fn test_choice_response_serialization() {
        let items = vec![SerializableMenuItem {
            key: None,
            display_text: "Selected Item".to_string(),
            preview: FzfPreview::None,
            metadata: None,
        }];

        let response = MenuResponse::ChoiceResult {
            action: Some("ctrl-e".to_string()),
            items,
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: MenuResponse = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, MenuResponse::ChoiceResult { action: Some(action), items }
                if action == "ctrl-e" && items.len() == 1)
        );
    }

    #[test]
    fn test_menu_item_with_metadata() {
        use std::collections::HashMap;

        let mut metadata = HashMap::new();
        metadata.insert("file".to_string(), "/path/to/file".to_string());
        metadata.insert("type".to_string(), "config".to_string());

        let item = SerializableMenuItem {
            key: None,
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

    #[test]
    fn test_menu_item_prefers_explicit_key() {
        let item = SerializableMenuItem {
            key: Some("pass:add".to_string()),
            display_text: "\u{1b}[32mAdd\u{1b}[0m".to_string(),
            preview: FzfPreview::None,
            metadata: None,
        };

        assert_eq!(item.fzf_key(), "pass:add");
    }

    #[test]
    fn test_menu_item_fallback_key_strips_ansi() {
        let item = SerializableMenuItem {
            key: None,
            display_text: "\u{1b}[32mAdd\u{1b}[0m".to_string(),
            preview: FzfPreview::None,
            metadata: None,
        };

        assert_eq!(item.fzf_key(), "Add");
    }

    #[test]
    fn test_slider_request_serialization() {
        let request = SliderRequest {
            min: 0,
            max: 100,
            value: Some(42),
            step: Some(1),
            big_step: Some(10),
            label: Some("Volume".to_string()),
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                "wpctl set-volume @DEFAULT_AUDIO_SINK@ \"${1}%\"".to_string(),
                "_".to_string(),
            ],
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: SliderRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.min, 0);
        assert_eq!(deserialized.max, 100);
        assert_eq!(deserialized.value, Some(42));
        assert_eq!(deserialized.label.as_deref(), Some("Volume"));
        assert_eq!(deserialized.command.len(), 4);
    }

    #[test]
    fn test_streaming_choice_frames_round_trip() {
        let request_id = "req_stream_1".to_string();
        let frames = [
            MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceBegin {
                    options: ChoiceOptions::new("Pick:")
                        .with_frecency_cache(Some("stream".to_string())),
                },
            ),
            MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceChunk {
                    items: vec![SerializableMenuItem::plain("alpha")],
                },
            ),
            MenuMessage::new(request_id.clone(), MenuRequest::ChoiceEnd),
        ];

        // Frames must survive NDJSON framing (one JSON object per line).
        let wire: String = frames
            .iter()
            .map(|frame| serde_json::to_string(frame).unwrap() + "\n")
            .collect();
        let parsed: Vec<MenuMessage> = wire
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(parsed.len(), 3);
        assert!(
            matches!(&parsed[0].payload, MenuRequest::ChoiceBegin { options }
                if options.prompt == "Pick:"
                    && options.frecency_cache.as_deref() == Some("stream"))
        );
        assert!(
            matches!(&parsed[1].payload, MenuRequest::ChoiceChunk { items } if items.len() == 1 && items[0].display_text == "alpha")
        );
        assert!(matches!(parsed[2].payload, MenuRequest::ChoiceEnd));
        assert!(parsed.iter().all(|m| m.request_id == request_id));
        assert!(
            parsed
                .iter()
                .all(|m| m.protocol_version == PROTOCOL_VERSION)
        );
    }
}
