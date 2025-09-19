# Progress Display Improvement Plan

# TODO

this entire plan is overcomplicated. finish_with_message leaves the message and
one of the style characters. Setting the style to nothing before
finish_with_message might be enough. No new structs needed, I can create a
helper finish function which does this. 

## Overview
Fix frozen spinner issues and enhance progress display with proper cleanup, multi-progress support, and improved user experience.

## Current State Analysis

### Existing Implementation
- **Library**: `indicatif` crate for progress reporting
- **Current usage**: Simple spinner with custom styling
- **Location**: `src/common/progress.rs`
- **Issue**: Spinner freezes/leaves artifacts after completion

### Current Implementation Details
```rust
// src/common/progress.rs
pub fn create_spinner(message: String) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap()
            .tick_chars("⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚"),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}
```

### Problem Analysis
The spinner leaves behind artifacts because:
1. **Improper cleanup**: `ProgressBar` not properly finished/dropped
2. **Missing finish handlers**: No explicit completion handling
3. **Drawing conflicts**: Terminal state not restored after completion
4. **Steady tick continuation**: Background thread continues after operation completes

## Root Cause Research

### indicatif Library Behavior
Based on research of `indicatif` documentation:
- **ProgressBar.finish()**: Completes and leaves final message
- **ProgressBar.finish_and_clear()**: Completes and clears display
- **ProgressBar.abandon()**: Leaves current state but marks as finished
- **Automatic cleanup**: Happens on `Drop`, but timing issues can occur

### Common Anti-patterns
1. **Early returns**: Forgetting to finish progress bars
2. **Panic handling**: Progress bars not cleaned up during panics
3. **Multi-threading**: Progress bars created/accessed across threads
4. **Terminal interference**: Other output corrupting progress display

## Proposed Solution Architecture

### 1. Enhanced Progress Manager
```rust
pub struct ProgressManager {
    multi_progress: MultiProgress,
    active_bars: Vec<ProgressBar>,
    cleanup_on_drop: bool,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            multi_progress: MultiProgress::new(),
            active_bars: Vec::new(),
            cleanup_on_drop: true,
        }
    }

    pub fn create_spinner(&mut self, message: String) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        pb.set_style(Self::spinner_style());
        pb.set_message(message);
        pb.enable_steady_tick(Duration::from_millis(100));

        // Add to multi-progress for better management
        self.multi_progress.add(pb.clone());
        self.active_bars.push(pb.clone());

        pb
    }

    pub fn create_progress_bar(&mut self, length: u64, message: String) -> ProgressBar {
        let pb = ProgressBar::new(length);
        pb.set_style(Self::progress_bar_style());
        pb.set_message(message);

        self.multi_progress.add(pb.clone());
        self.active_bars.push(pb.clone());

        pb
    }

    pub fn finish_all(&mut self, result: ProgressResult) {
        for pb in &self.active_bars {
            match result {
                ProgressResult::Success => {
                    pb.finish_with_message("✓ Done");
                }
                ProgressResult::Error => {
                    pb.abandon_with_message("✗ Failed");
                }
                ProgressResult::Cancelled => {
                    pb.finish_and_clear();
                }
            }
        }
        self.active_bars.clear();
    }

    fn spinner_style() -> ProgressStyle {
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg:.dim} {elapsed:.dim}")
            .unwrap()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
    }

    fn progress_bar_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .progress_chars("##-")
            .unwrap()
    }
}

pub enum ProgressResult {
    Success,
    Error,
    Cancelled,
}

impl Drop for ProgressManager {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            self.finish_all(ProgressResult::Cancelled);
        }
    }
}
```

### 2. Scope-based Progress Management
```rust
pub struct ScopedProgress<'a> {
    manager: &'a mut ProgressManager,
    progress_bar: ProgressBar,
}

impl<'a> ScopedProgress<'a> {
    pub fn new(manager: &'a mut ProgressManager, message: String) -> Self {
        let pb = manager.create_spinner(message);
        Self { manager, progress_bar: pb }
    }

    pub fn success(mut self, message: impl Into<String>) {
        self.progress_bar.finish_with_message(message);
    }

    pub fn error(mut self, message: impl Into<String>) {
        self.progress_bar.abandon_with_message(message);
    }

    pub fn update_message(&mut self, message: impl Into<String>) {
        self.progress_bar.set_message(message.into());
    }
}

impl<'a> Drop for ScopedProgress<'a> {
    fn drop(&mut self) {
        if !self.progress_bar.is_finished() {
            self.progress_bar.abandon_with_message("Interrupted");
        }
    }
}
```

### 3. Async Progress Support
```rust
#[derive(Clone)]
pub struct AsyncProgress {
    progress_bar: ProgressBar,
}

impl AsyncProgress {
    pub fn new(length: u64, message: String) -> Self {
        let pb = ProgressBar::new(length);
        pb.set_style(ProgressManager::progress_bar_style());
        pb.set_message(message);
        Self { progress_bar: pb }
    }

    pub async fn increment(&self, delta: u64) {
        self.progress_bar.inc(delta);
        // Small delay to prevent overwhelming the display
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    pub fn set_position(&self, pos: u64) {
        self.progress_bar.set_position(pos);
    }

    pub fn finish(self, message: impl Into<String>) {
        self.progress_bar.finish_with_message(message);
    }
}
```

### 4. Integration with Existing Code

#### Usage Patterns
```rust
// Before:
let spinner = create_spinner("Processing...".to_string());
do_work();
// Spinner left hanging...

// After:
let mut manager = ProgressManager::new();
{
    let _progress = ScopedProgress::new(&mut manager, "Processing...".to_string());
    do_work();
    // Automatically cleaned up with success message
}

// For async operations:
async fn process_files(files: Vec<PathBuf>) -> Result<()> {
    let progress = AsyncProgress::new(files.len() as u64, "Processing files".to_string());

    for (index, file) in files.into_iter().enumerate() {
        process_file(&file).await?;
        progress.increment(1).await;
    }

    progress.finish("All files processed");
    Ok(())
}
```

## Implementation Plan

### Phase 1: Fix Immediate Issues (Week 1)
1. **Replace current spinner function**
   - Implement `ProgressManager` with proper cleanup
   - Update all existing usage sites
   - Add comprehensive testing

2. **Add scope-based management**
   - Implement `ScopedProgress` wrapper
   - Convert existing code to use scoped approach
   - Verify proper cleanup in all scenarios

### Phase 2: Enhanced Features (Week 2)
1. **Multi-progress support**
   - Implement `MultiProgress` integration
   - Add support for concurrent operations
   - Create progress grouping functionality

2. **Async progress support**
   - Implement `AsyncProgress` for async operations
   - Add progress streaming for long operations
   - Integrate with existing async codebase

### Phase 3: User Experience (Week 3)
1. **Improved visual design**
   - Enhanced templates and styling
   - Color-coded progress states
   - Better terminal compatibility

2. **Progress persistence**
   - Resume interrupted operations
   - Progress checkpointing
   - Estimated completion improvements

## Technical Considerations

### Dependencies
```toml
[dependencies]
indicatif = { version = "0.17", features = ["improved_unicode"] }
console = "0.15"
tokio = { version = "1.0", features = ["full"] }
```

### Error Handling
- **Panics**: Ensure progress bars cleanup during panics
- **Early returns**: Use scope-based cleanup
- **Terminal issues**: Graceful degradation on unsupported terminals

### Performance
- **Overhead**: Minimize progress update frequency
- **Memory**: Clean up completed progress bars
- **Concurrency**: Thread-safe progress updates

## Testing Strategy

### Unit Tests
- Progress creation and cleanup
- Multi-progress coordination
- Error scenario handling

### Integration Tests
- Long-running operations
- Concurrent progress updates
- Terminal interaction scenarios

### Manual Testing
- Visual verification of progress display
- Terminal compatibility testing
- Performance under load

## Success Metrics

- **Bug elimination**: Zero frozen spinners
- **User experience**: Clear progress indication
- **Performance**: <2ms overhead per update
- **Reliability**: Proper cleanup in all scenarios

## Future Enhancements

- **Remote progress**: WebSocket-based progress streaming
- **GUI integration**: Progress in desktop notifications
- **Advanced metrics**: Performance analytics
- **Custom themes**: User-configurable progress styles
