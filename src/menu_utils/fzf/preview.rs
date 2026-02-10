//! Preview system for FZF items

use anyhow::Result;
use std::collections::HashMap;

use super::types::{FzfPreview, FzfSelectable};

pub struct PreviewUtils;

/// Content type for mixed previews
#[derive(Clone)]
pub(crate) enum MixedPreviewContent {
    Text(String),
    Command(String),
}

pub(crate) enum PreviewStrategy {
    /// No previews
    None,
    /// Text previews embedded in input (base64)
    Text(HashMap<String, String>),
    /// Single command executed by FZF with key substitution
    Command(String),
    /// Each item has its own command preview (stored as shell commands to execute)
    CommandPerItem(HashMap<String, String>),
    /// Mixed text and command previews - each item knows its type
    Mixed(HashMap<String, MixedPreviewContent>),
}

impl PreviewUtils {
    /// Analyze preview types and determine optimal strategy
    pub fn analyze_preview_strategy<T: FzfSelectable>(items: &[T]) -> Result<PreviewStrategy> {
        if items.is_empty() {
            return Ok(PreviewStrategy::None);
        }

        let mut first_command: Option<String> = None;
        let mut text_map: HashMap<String, String> = HashMap::new();
        let mut mixed_map: HashMap<String, MixedPreviewContent> = HashMap::new();
        let mut has_text = false;
        let mut has_command = false;
        let mut all_same_command = true;

        // First pass: collect info and populate mixed_map
        for item in items {
            let key = item.fzf_key();

            match item.fzf_preview() {
                FzfPreview::Text(text) => {
                    has_text = true;
                    mixed_map.insert(key.clone(), MixedPreviewContent::Text(text));
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
                    mixed_map.insert(key.clone(), MixedPreviewContent::Command(cmd));
                }
                FzfPreview::None => {
                    // No preview for this item
                }
            }
        }

        // Determine strategy and populate text_map only when needed
        if !has_text && !has_command {
            Ok(PreviewStrategy::None)
        } else if has_command && !has_text && all_same_command {
            // All items use the same command - optimal case!
            // We can use a single --preview command with {} substitution
            Ok(PreviewStrategy::Command(first_command.unwrap()))
        } else if has_command && !has_text && !all_same_command {
            // Each item has a different command - populate text_map with commands
            for (key, content) in &mixed_map {
                if let MixedPreviewContent::Command(cmd) = content {
                    text_map.insert(key.clone(), cmd.clone());
                }
            }
            Ok(PreviewStrategy::CommandPerItem(text_map))
        } else if !has_command && has_text {
            // All text previews - populate text_map with texts
            for (key, content) in &mixed_map {
                if let MixedPreviewContent::Text(text) = content {
                    text_map.insert(key.clone(), text.clone());
                }
            }
            Ok(PreviewStrategy::Text(text_map))
        } else {
            // Mixed text and command previews - track type per item
            // Don't populate text_map since it's not used in Mixed strategy
            Ok(PreviewStrategy::Mixed(mixed_map))
        }
    }
}
