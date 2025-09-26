# Game Setup Command Implementation

## Completed Features

The `instant game setup` command has been successfully implemented with the following functionality:

### Command Description
```bash
instant game setup
```

This command helps users set up games that have been added to the shared configuration but don't have installation paths configured on the current device.

### How It Works

1. **Detection**: The command identifies games that exist in the global `games.toml` configuration but are missing from the device-specific `installations.toml` file.

2. **Path Collection**: For each uninstalled game, it:
   - Retrieves all snapshots from the restic repository for that game
   - Extracts all unique save paths from different devices/snapshots
   - Groups paths by frequency and usage statistics

3. **Interactive Selection**: Users can choose from:
   - Existing paths found in snapshots (with usage statistics)
   - A custom path option for manual entry

4. **Path Information**: For each existing path, the interface shows:
   - Usage frequency (number of snapshots)
   - Device count and names
   - Timeline information (first/last seen)

5. **Installation Setup**: Creates the installation entry in `installations.toml` and optionally creates the directory if it doesn't exist.

### Key Features

- **Multi-Device Awareness**: Leverages snapshot data from different devices to suggest appropriate paths
- **Cross-Device Path Normalization**: Automatically converts `/home/<username>` paths to `~` notation for cross-device compatibility, so paths like `/home/alice/.config/game/saves` and `/home/bob/.config/game/saves` are both recognized as the same logical path `~/.config/game/saves`
- **Statistical Insights**: Shows usage patterns to help users make informed decisions
- **Path Validation**: Offers to create directories that don't exist
- **Graceful Handling**: Continues setup for remaining games even if one fails
- **Rich UI**: Provides detailed previews and formatted information

### Implementation Details

- **File**: `src/game/setup.rs`
- **CLI Integration**: Added to `GameCommands::Setup` in `src/game/cli.rs`
- **Command Handler**: Integrated in `src/game/commands.rs`
- **Core Functions**:
   - `setup_uninstalled_games()` - Main entry point
   - `find_uninstalled_games()` - Game detection logic
   - `setup_single_game()` - Individual game setup
   - `extract_unique_paths_from_snapshots()` - Path extraction and analysis with cross-device normalization
   - `normalize_path_for_cross_device()` - Converts `/home/<user>` paths to `~` notation for device independence
   - `choose_installation_path()` - Interactive path selection
   - Rich data structures for path information and user interaction

### Usage Scenarios

1. **Multi-Device Setup**: When setting up InstantCLI on a new device where games are already configured in the shared repository
2. **Path Migration**: When a game's save location changes and you want to update the local configuration
3. **New User Onboarding**: When joining a shared game save repository

### Architecture Benefits

- **Decentralized**: Each device maintains its own installation paths while sharing game definitions
- **Data-Driven**: Uses actual backup history to suggest the most appropriate paths
- **User-Friendly**: Interactive interface with rich preview information
- **Robust**: Handles edge cases like missing snapshots or empty paths gracefully

The implementation fully addresses the original plan requirements and provides a comprehensive solution for cross-device game save path management.

