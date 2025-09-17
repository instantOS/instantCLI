# Plan for Menu System Feature Parity

## Problem Statement
The server version of the menu system currently lacks preview support and only accepts simple string arrays, while the normal version supports rich previews through the `FzfSelectable` trait. This creates feature inconsistency and code duplication.

## Current State Analysis
- **Normal Mode**: Supports `FzfSelectable` trait with rich previews (text/command/none)
- **Server Mode**: Limited to `Vec<String>` with no preview capability
- **Protocol**: Currently only serializes basic string data
- **Code Duplication**: Server recreates `SelectItem` structs without preview support

## Implementation Plan

### Phase 1: Extend Protocol to Support Rich Items
1. **Create Serializable Item Structure**
   - Define `SerializableMenuItem` struct with fields for display text, preview, and metadata
   - Implement serde serialization/deserialization
   - Support all preview types (text, command, none)

2. **Update MenuRequest Enum**
   - Modify `Choice` variant to use `Vec<SerializableMenuItem>` instead of `Vec<String>`
   - Do not main Maintain backward compatibility, breaking changes allowed

3. **Extend MenuResponse Enum**
   - Add support for returning rich item metadata in responses
   - Enable bi-directional rich data transfer

### Phase 2: Unify Item Handling Logic
4. **Leverage Existing FzfSelectable Trait**
   - Use existing `FzfSelectable` trait instead of creating new trait
   - Make server items implement `FzfSelectable` directly
   - Move common item processing logic to shared module
   - Eliminate code duplication between server and normal modes

5. **Refactor FzfWrapper**
   - Extract preview generation logic into shared utilities
   - Make preview system work with both local and server item types
   - Create unified item mapping system based on `FzfSelectable`

### Phase 3: Implement Server Preview Support
6. **Update Server Processing**
   - Modify server to deserialize rich item data
   - Implement preview generation on server side
   - Create temporary preview scripts for server mode

7. **Update Client Serialization**
   - Modify client to serialize items with preview data
   - Add preview data transmission over Unix domain socket
   - Handle preview command execution on server

9. **Documentation Updates**
   - Update protocol documentation
   - Add examples of rich item usage
   - Document migration path from string-based to rich items

### Phase 5: Optimization and Cleanup
10. **Performance Optimization**
    - Benchmark serialization overhead
    - Optimize preview script generation
    - Reduce memory usage for large item sets

11. **Code Cleanup**
    - Remove duplicated item handling code
    - Consolidate shared utilities
    - Update error handling for unified approach

## Technical Details

### New Data Structures
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableMenuItem {
    pub display_text: String,
    pub preview: FzfPreview,
    pub metadata: Option<HashMap<String, String>>,
}

// Leverage existing FzfPreview enum from fzf_wrapper.rs
// FzfPreview already has: Text(String), Command(String), None
```

### Backward Compatibility Strategy
- Breaking changes allowed per requirements
- Clean migration path without maintaining legacy string-based API
- Direct upgrade path to new rich item protocol

### Performance Considerations
- Lazy serialization of preview data
- Compression for large preview text
- Caching of preview command results


