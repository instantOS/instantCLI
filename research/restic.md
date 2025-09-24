# Restic CLI Research

## Overview

Restic is a modern backup program that can store data in many different storage backends. It supports encryption, deduplication, and has excellent scripting capabilities.

## Key Exit Codes

- **0**: Command was successful
- **1**: Command failed (general error)
- **2**: Go runtime error
- **3**: `backup` command could not read some source data
- **10**: Repository does not exist (since restic 0.17.0)
- **11**: Failed to lock repository (since restic 0.17.0)
- **12**: Wrong password (since restic 0.17.1)
- **130**: Restic was interrupted using SIGINT or SIGSTOP

## JSON Output

Most restic commands support `--json` flag for machine-readable output. Two formats are used:

1. **Single JSON document**: Commands output one complete JSON object
2. **JSON lines**: Stream of new-line separated JSON messages (common for long-running operations)

## Key Commands for Scripting

### Check if repository exists
```bash
restic -r /path/to/repo cat config
# Returns exit code 10 if repository doesn't exist (since 0.17.0)
```

### Initialize repository
```bash
restic -r /path/to/repo init
```

### Backup with progress
```bash
restic -r /path/to/repo backup /path/to/data --json
```

### List snapshots
```bash
restic -r /path/to/repo snapshots --json
```

### Restore from snapshot
```bash
restic -r /path/to/repo restore snapshot-id --target /path/to/restore --json
```

## Important JSON Message Types

### Backup Status
```json
{
  "message_type": "status",
  "seconds_elapsed": 60,
  "percent_done": 0.5,
  "total_files": 1000,
  "files_done": 500,
  "total_bytes": 1000000,
  "bytes_done": 500000,
  "error_count": 0
}
```

### Backup Summary
```json
{
  "message_type": "summary",
  "files_new": 10,
  "files_changed": 5,
  "files_unmodified": 985,
  "data_added": 50000,
  "snapshot_id": "abc123"
}
```

### Error Messages
```json
{
  "message_type": "error",
  "error": {
    "message": "Error description",
    "during": "backup",
    "item": "/path/to/file"
  }
}
```

## Common Patterns for CLI Wrappers

### Repository Initialization
1. Check if repository exists using `restic cat config`
2. If exit code 10, initialize with `restic init`
3. Handle exit codes appropriately

### Backup Operations
1. Use `--json` flag for progress tracking
2. Parse JSON lines stream for real-time updates
3. Check final exit code for success/failure

### Restore Operations
1. List snapshots first to get available IDs
2. Use `restore` command with target path
3. Monitor progress with JSON output

## Security Considerations

- Password should be passed via environment variable `RESTIC_PASSWORD` or command line
- Repository URLs should be validated
- Consider using `--json` for better error handling
- Always check exit codes, not just output

## Performance Considerations

- Use `--json` for efficient parsing
- Consider caching repository metadata
- Handle long-running operations with progress tracking
- Use appropriate timeout values