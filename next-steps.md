# Dev Subcommand Implementation Plan

## Overview
Add a new `dev` subcommand to InstantCLI that provides development utilities, starting with a `clone` subcommand for interactive repository cloning from the instantOS GitHub organization.

## Implementation Plan

### Phase 1: Dependencies and Setup

1. **Add required dependencies to Cargo.toml**
   - `fzf-wrapped = "0.1.4"` - For fuzzy selection interface
   - `reqwest = { version = "0.11", features = ["json"] }` - For GitHub API calls
   - `serde = { version = "1.0", features = ["derive"] }` - Already present, needed for JSON deserialization
   - `tokio = { version = "1.0", features = ["full"] }` - Already present, needed for async HTTP requests

### Phase 2: GitHub API Integration

2. **Create GitHub API client module** (`src/dev/github.rs`)
   - Implement function to fetch instantOS organization repositories
   - Handle API rate limiting and errors
   - Parse repository list from GitHub API response

```rust
// src/dev/github.rs
pub async fn fetch_instantos_repos() -> Result<Vec<GitHubRepo>>;
#[derive(Debug, serde::Deserialize)]
pub struct GitHubRepo {
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub clone_url: String,
    pub default_branch: String,
}
```

### Phase 3: Interactive Selection

3. **Create fuzzy selection module** (`src/dev/fuzzy.rs`)
   - Implement repository selection using fzf-wrapped
   - Configure fzf with appropriate UI settings
   - Handle user cancellation gracefully
   - Display repository names with descriptions

```rust
// src/dev/fuzzy.rs
pub fn select_repository(repos: Vec<GitHubRepo>) -> Result<Option<GitHubRepo>>;
```

### Phase 4: Clone Implementation

4. **Create clone functionality** (`src/dev/clone.rs`)
   - Implement repository cloning with depth 1
   - Create `~/workspace` directory if it doesn't exist
   - Use existing git utilities from `src/dot/utils.rs`
   - Handle existing directories gracefully
   - Show progress indicators

```rust
// src/dev/clone.rs
pub fn clone_repository(repo: &GitHubRepo, target_dir: &Path) -> Result<()>;
```

### Phase 5: CLI Integration

5. **Create dev command module** (`src/dev/mod.rs`)
   - Define CLI command structure using clap
   - Orchestrate the clone workflow
   - Handle errors and user feedback
   - Support debug mode

```rust
// src/dev/mod.rs
#[derive(Subcommand, Debug)]
pub enum DevCommands {
    Clone,
}

pub async fn handle_dev_command(command: DevCommands, debug: bool) -> Result<()>;
```

6. **Update main CLI parser** (`src/main.rs`)
   - Add `Dev` variant to `Commands` enum
   - Add dev command handler in main function
   - Update module imports

### Phase 6: Error Handling and UX

7. **Implement comprehensive error handling**
   - Network errors (GitHub API)
   - Git operation failures
   - File system errors
   - User cancellation (fzf)
   - Permission issues

8. **Enhance user experience**
   - Progress spinners for long operations
   - Clear success/error messages
   - Verbose output in debug mode
   - Helpful error messages

## Technical Details

### Directory Structure
```
src/
├── dev/
│   ├── mod.rs          # Main dev module
│   ├── github.rs       # GitHub API integration
│   ├── fuzzy.rs        # Fuzzy selection interface
│   └── clone.rs        # Repository cloning logic
├── main.rs             # Updated to include dev command
└── ...
```

### Dependencies to Add
```toml
[dependencies]
# Existing dependencies...
fzf-wrapped = "0.1.4"
reqwest = { version = "0.11", features = ["json"] }
```

### API Endpoints
- GitHub Organization Repositories: `https://api.github.com/orgs/instantOS/repos`
- Rate limit: 60 requests/hour for unauthenticated, 5000/hour for authenticated

### FZF Configuration
- Border: Rounded
- Layout: Reverse
- Header: "Select instantOS repository to clone:"
- Height: 40% of terminal
