//! Preview system for FZF items

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::types::FzfPreview;
use super::types::FzfSelectable;

// UNUSED: Consider removing - not used anywhere in the codebase
#[derive(Serialize, Deserialize)]
struct PreviewData {
    key: String,
    preview_type: String,
    preview_content: String,
}

pub struct PreviewUtils;

pub(crate) enum PreviewStrategy {
    /// No previews
    None,
    /// Text previews embedded in input (base64)
    Text(HashMap<String, String>),
    /// Single command executed by FZF with key substitution
    Command(String),
    /// Mixed - each item has different preview (fallback to text encoding)
    Mixed(HashMap<String, String>),
}

impl PreviewUtils {
    /// Analyze preview types and determine optimal strategy
    pub fn analyze_preview_strategy<T: FzfSelectable>(items: &[T]) -> Result<PreviewStrategy> {
        if items.is_empty() {
            return Ok(PreviewStrategy::None);
        }

        let mut first_command: Option<String> = None;
        let mut text_map = HashMap::new();
        let mut has_text = false;
        let mut has_command = false;
        let mut all_same_command = true;

        for item in items {
            let display = item.fzf_display_text();
            let key = item.fzf_key();

            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    has_text = true;
                    text_map.insert(display.clone(), text);
                }
                FzfPreview::Command(cmd) => {
                    has_command = true;
                    if let Some(ref first) = first_command {
                        if first != &cmd {
                            all_same_command = false;
                        }
                    } else {
                        first_command = Some(cmd.clone());
                    }
                    // For command previews, we'll pass the key (usually MIME type) to the command
                    // The command should use $1 to reference it
                    text_map.insert(display.clone(), key);
                }
                FzfPreview::None => {
                    // No preview for this item
                }
            }
        }

        // Determine strategy
        if !has_text && !has_command {
            Ok(PreviewStrategy::None)
        } else if has_command && !has_text && all_same_command {
            // All items use the same command - optimal case!
            // We can use a single --preview command with {} substitution
            Ok(PreviewStrategy::Command(first_command.unwrap()))
        } else if !has_command && has_text {
            // All text previews - use existing base64 approach
            Ok(PreviewStrategy::Text(text_map))
        } else {
            // Mixed or multiple different commands - fall back to text encoding
            // For commands, we store the key to pass to a wrapper script
            Ok(PreviewStrategy::Mixed(text_map))
        }
    }
}
