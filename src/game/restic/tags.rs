use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};

/// The primary tag used for all game-related snapshots
pub const INSTANT_GAME_TAG: &str = "instantgame";

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
    
    String::from_utf8(decoded_bytes)
        .context("Decoded tag contains invalid UTF-8")
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
}