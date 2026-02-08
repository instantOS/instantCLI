use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};

/// Game save tag management with base64 encoding
///
/// This module handles the encoding and decoding of game names for use as restic tags.
/// Game names are base64-encoded to ensure they're safe for use as restic tags, avoiding
/// issues with special characters like commas, spaces, and other problematic characters.
///
/// ## Migration Strategy
///
/// This system was migrated from plain text tags to base64-encoded tags to fix issues
/// with game names containing commas and other special characters that break restic's
/// tag filtering. The migration strategy is:
///
/// 1. **Old snapshots**: Use plain text game names as tags (still exist in repository)
/// 2. **New snapshots**: Use base64-encoded game names as tags (created going forward)
/// 3. **Filtering**: Only new snapshots are found by the current filtering logic
/// 4. **Developer tools**: Debug commands help visualize the transition
///
/// Old snapshots are effectively "orphaned" but remain in the repository for safety.
/// Since this is a development project, this breaking change is acceptable.
///
/// ## Usage Example
///
/// ```rust
/// use crate::game::restic::tags;
///
/// // Create tags for a game (handles encoding automatically)
/// let tags = tags::create_game_tags("Game: With, Special Characters!");
/// // Returns: ["instantgame", "R2FtZTogV2l0aCwgU3BlY2lhbCBDaGFyYWN0ZXJzIQ=="]
///
/// // Extract game name from snapshot tags
/// let game_name = tags::extract_game_name_from_tags(&tags).unwrap();
/// // Returns: "Game: With, Special Characters!"
/// ```
///
/// The primary tag used for all game-related snapshots
pub const INSTANT_GAME_TAG: &str = "instantgame";
/// Tag used for dependency snapshots
pub const INSTANT_GAME_DEPENDENCY_TAG: &str = "instantgame-dep";

/// Encode a game name for use as a restic tag
///
/// Game names may contain characters that are problematic for restic tags (like commas),
/// so we base64 encode them to ensure they're safe to use as tags.
///
/// # Arguments
/// * `game_name` - The original game name
///
/// # Returns
/// Base64-encoded game name safe for use as a restic tag
pub fn encode_game_name_for_tag(game_name: &str) -> String {
    general_purpose::STANDARD.encode(game_name.as_bytes())
}

/// Decode a base64-encoded game name tag back to the original name
///
/// # Arguments  
/// * `encoded_tag` - The base64-encoded tag
///
/// # Returns
/// The original game name, or an error if decoding fails
pub fn decode_game_name_from_tag(encoded_tag: &str) -> Result<String> {
    let decoded_bytes = general_purpose::STANDARD
        .decode(encoded_tag)
        .context("Failed to decode base64 tag")?;

    String::from_utf8(decoded_bytes).context("Decoded tag contains invalid UTF-8")
}

/// Create the complete set of tags for a game snapshot
///
/// # Arguments
/// * `game_name` - The original game name
///
/// # Returns
/// Vector of tags: [INSTANT_GAME_TAG, base64_encoded_game_name]
pub fn create_game_tags(game_name: &str) -> Vec<String> {
    vec![
        INSTANT_GAME_TAG.to_string(),
        encode_game_name_for_tag(game_name),
    ]
}

/// Encode a dependency ID for use as a restic tag fragment
pub fn encode_dependency_id_for_tag(dependency_id: &str) -> String {
    general_purpose::STANDARD.encode(dependency_id.as_bytes())
}

/// Create tag set for dependency snapshot (game + dependency ID)
pub fn create_dependency_tags(game_name: &str, dependency_id: &str) -> Vec<String> {
    vec![
        INSTANT_GAME_DEPENDENCY_TAG.to_string(),
        format!("game:{}", encode_game_name_for_tag(game_name)),
        format!("dep:{}", encode_dependency_id_for_tag(dependency_id)),
    ]
}

/// Extract game name from snapshot tags
///
/// Looks for the encoded game name tag in a snapshot's tags and decodes it.
///
/// # Arguments
/// * `tags` - The snapshot's tags
///
/// # Returns
/// The decoded game name if found, or None if not found/decodable
pub fn extract_game_name_from_tags(tags: &[String]) -> Option<String> {
    // Find the tag that's not the primary instant game tag
    tags.iter()
        .find(|tag| *tag != INSTANT_GAME_TAG)
        .and_then(|encoded_tag| decode_game_name_from_tag(encoded_tag).ok())
}

/// Debug utility: Pretty print all snapshots with decoded game names
///
/// This is useful for developers to understand what's in their restic repository
/// after the base64 encoding migration.
///
/// # Arguments
/// * `snapshots` - List of snapshots to analyze
///
/// # Returns
/// A formatted string showing snapshot IDs, times, and decoded game names
pub fn debug_snapshot_tags(snapshots: &[crate::restic::wrapper::Snapshot]) -> String {
    let mut output = String::new();
    output.push_str("Snapshot Debug Information:\n");
    output.push_str("==========================\n\n");

    for snapshot in snapshots {
        output.push_str(&format!(
            "Snapshot ID: {} ({})\n",
            &snapshot.id[..8],
            snapshot.time
        ));
        output.push_str(&format!("  Raw tags: {:?}\n", snapshot.tags));

        if let Some(game_name) = extract_game_name_from_tags(&snapshot.tags) {
            output.push_str(&format!("  Game: {game_name}\n"));
        } else {
            output.push_str("  Game: <unable to decode>\n");
        }

        // Check if this uses old format (plain text) or new format (base64)
        let uses_base64 = snapshot
            .tags
            .iter()
            .any(|tag| tag != INSTANT_GAME_TAG && decode_game_name_from_tag(tag).is_ok());

        let uses_plain_text = snapshot
            .tags
            .iter()
            .any(|tag| tag != INSTANT_GAME_TAG && decode_game_name_from_tag(tag).is_err());

        if uses_base64 {
            output.push_str("  Format: Base64 (new)\n");
        } else if uses_plain_text {
            output.push_str("  Format: Plain text (old)\n");
        } else {
            output.push_str("  Format: Unknown\n");
        }

        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_simple_name() {
        let original = "Animal Crossing";
        let encoded = encode_game_name_for_tag(original);
        let decoded = decode_game_name_from_tag(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_complex_name() {
        let original = "Professor Layton vs Phoenix Wright: Ace Attorney";
        let encoded = encode_game_name_for_tag(original);
        let decoded = decode_game_name_from_tag(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_with_commas() {
        let original = "Game, with, many, commas!";
        let encoded = encode_game_name_for_tag(original);
        let decoded = decode_game_name_from_tag(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_special_chars() {
        let original = "Game: With/Special\\Chars & Symbols!@#$%^&*()";
        let encoded = encode_game_name_for_tag(original);
        let decoded = decode_game_name_from_tag(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_create_game_tags() {
        let game_name = "Test Game";
        let tags = create_game_tags(game_name);

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], INSTANT_GAME_TAG);

        // Verify the second tag can be decoded back to original name
        let decoded = decode_game_name_from_tag(&tags[1]).unwrap();
        assert_eq!(decoded, game_name);
    }

    #[test]
    fn test_extract_game_name_from_tags() {
        let game_name = "Test Game With Spaces";
        let tags = create_game_tags(game_name);

        let extracted = extract_game_name_from_tags(&tags).unwrap();
        assert_eq!(extracted, game_name);
    }

    #[test]
    fn test_extract_game_name_from_invalid_tags() {
        let tags = vec!["instantgame".to_string(), "invalid_base64!".to_string()];
        let extracted = extract_game_name_from_tags(&tags);
        assert!(extracted.is_none());
    }

    #[test]
    fn test_debug_snapshot_tags() {
        use crate::restic::wrapper::Snapshot;

        let snapshots = vec![Snapshot {
            id: "abc123def456".to_string(),
            short_id: "abc123de".to_string(),
            time: "2025-01-01T12:00:00Z".to_string(),
            tags: create_game_tags("Test Game"),
            hostname: "test".to_string(),
            parent: None,
            tree: "tree123".to_string(),
            paths: vec!["/test".to_string()],
            username: "user".to_string(),
            uid: Some(1000),
            gid: Some(1000),
            excludes: None,
            program_version: Some("restic 0.17.0".to_string()),
            summary: None,
        }];

        let debug_output = debug_snapshot_tags(&snapshots);
        assert!(debug_output.contains("Test Game"));
        assert!(debug_output.contains("Base64 (new)"));
        assert!(debug_output.contains("abc123de"));
    }
}
