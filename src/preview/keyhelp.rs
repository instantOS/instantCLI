//! Per-keybind preview rendered on demand for `ins keyhelp`.
//!
//! The `KeybindRow::fzf_preview()` returns a shell command
//! (`ins preview --id keyhelp --key "$1"`). On highlight, fzf invokes this
//! child, which parses the JSON payload from `$1`, fetches the
//! `instantwmctl --json action --list` docs (if available), and prints the
//! sectioned preview text.

use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::keyhelp::{ActionDoc, KeybindPreviewPayload, fetch_action_docs};
use crate::preview::PreviewContext;

/// Render the preview for a single keybind row. The key is the JSON
/// [`KeybindPreviewPayload`] written by `KeybindRow::fzf_key()`; the rendered
/// output is what fzf shows in its preview pane for the highlighted row.
pub fn render_keyhelp_preview(ctx: &PreviewContext) -> Result<String> {
    let key = ctx
        .key()
        .with_context(|| "keyhelp preview requires a payload key")?;
    let payload: KeybindPreviewPayload = serde_json::from_str(key)
        .with_context(|| format!("failed to parse keyhelp payload '{key}'"))?;

    // One `instantwmctl` round-trip per highlight to look up action docs.
    // Cheap (the binary is local, the JSON is small) and avoids shipping the
    // docs map to every preview invocation. An empty map just skips the
    // docs section, which is fine.
    let docs = fetch_action_docs();
    Ok(render_with_docs(&payload, &docs))
}

fn render_with_docs(payload: &KeybindPreviewPayload, docs: &HashMap<String, ActionDoc>) -> String {
    payload.render_preview(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_key(key: &str) -> PreviewContext {
        PreviewContext {
            key: Some(key.to_string()),
            columns: Some(80),
            lines: Some(24),
        }
    }

    fn payload_json(modifiers: &str, key: &str, action: &str, mode: &str, origin: &str) -> String {
        serde_json::to_string(&KeybindPreviewPayload {
            modifiers: modifiers.to_string(),
            key: key.to_string(),
            action: action.to_string(),
            mode: mode.to_string(),
            origin: origin.to_string(),
        })
        .unwrap()
    }

    #[test]
    fn renders_global_user_binding() {
        let key = payload_json("Super", "Return", "spawn kitty", "global", "user");
        let text = render_keyhelp_preview(&ctx_with_key(&key)).unwrap();
        assert!(text.contains("Super + Return"));
        assert!(text.contains("spawn kitty"));
        assert!(text.contains("your config"));
        assert!(text.contains("Defined in ~/.config/instantwm/config.toml"));
        // No availability note for global bindings.
        assert!(!text.contains("Only available"));
    }

    #[test]
    fn renders_non_global_mode_availability() {
        let key = payload_json("", "Return", "spawn foo", "desktop", "compiled_default");
        let text = render_keyhelp_preview(&ctx_with_key(&key)).unwrap();
        assert!(text.contains("Only available when 'desktop' mode is active"));
        assert!(text.contains("built-in default"));
    }

    #[test]
    fn missing_key_is_an_error() {
        let ctx = PreviewContext {
            key: None,
            columns: Some(80),
            lines: Some(24),
        };
        assert!(render_keyhelp_preview(&ctx).is_err());
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(render_keyhelp_preview(&ctx_with_key("not json")).is_err());
    }
}
