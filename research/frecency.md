# Frecency Integration with the `fre` Library

## Overview

The `fre` library is a Rust crate that implements frecency (frequency + recency) scoring for items. It provides a sophisticated algorithm that balances how often an item is used (frequency) with how recently it was used (recency), making it perfect for application launchers.

## Key Concepts

### Frecency Algorithm
- **Frequency**: How often an item is accessed
- **Recency**: How recently an item was accessed  
- **Half-life**: Time period after which the frecency score is halved (default: 3 days)
- **Exponential decay**: Older accesses contribute less to the score over time

### Core Components

1. **FrecencyStore**: Main storage container for all items and their statistics
2. **ItemStats**: Individual item statistics (frecency score, access count, last access time)
3. **SortMethod**: Enum for different sorting approaches (Frecent, Frequent, Recent)

## Library Structure

### FrecencyStore
```rust
pub struct FrecencyStore {
    reference_time: f64,        // Base time for calculations
    half_life: f64,            // Time for score to halve (seconds)
    pub items: Vec<ItemStats>, // Collection of tracked items
}
```

### ItemStats
```rust
pub struct ItemStats {
    pub item: String,          // Item identifier (e.g., application name)
    half_life: f64,           // Half-life for this item
    reference_time: f64,      // Reference time for calculations
    last_accessed: f64,       // Last access time relative to reference
    frecency: f64,           // Current frecency score
    pub num_accesses: i32,   // Total number of accesses
}
```

## Integration Strategy for InstantCLI

### 1. Data Storage
- Store frecency data in `~/.cache/ins/frecency_store.json`
- Use JSON serialization for persistence
- Separate from the application cache for different refresh cycles

### 2. Usage Tracking
- Record each application launch with `store.add(app_name)`
- Automatically updates frecency score and access count
- Updates last access time

### 3. Sorting Integration
- Use `SortMethod::Frecent` for primary sorting
- Fall back to alphabetical for items with equal scores
- Maintain existing cache for fast startup

### 4. Background Updates
- Load frecency store on startup
- Update scores when applications are launched
- Save store periodically and on exit

## Code Examples

### Basic Usage
```rust
use fre::store::{FrecencyStore, read_store, write_store};
use std::path::PathBuf;

// Initialize or load existing store
let store_path = PathBuf::from("~/.cache/ins/frecency_store.json");
let mut store = read_store(&store_path).unwrap_or_default();

// Record application usage
store.add("firefox");
store.add("code");
store.add("firefox"); // firefox now has higher score

// Get sorted applications by frecency
let sorted_apps = store.sorted(SortMethod::Frecent);

// Save store
write_store(store, &store_path).unwrap();
```

### Integration with Existing Cache
```rust
// In launch/cache.rs
pub struct LaunchCache {
    cache_path: PathBuf,
    frecency_path: PathBuf,
    frecency_store: Option<FrecencyStore>,
}

impl LaunchCache {
    pub async fn get_applications_with_frecency(&mut self) -> Result<Vec<String>> {
        // Get base applications from PATH scan
        let mut apps = self.scan_path_directories()?;
        
        // Load frecency store
        let frecency_store = self.get_frecency_store()?;
        
        // Sort by frecency, then alphabetically
        apps.sort_by(|a, b| {
            let score_a = frecency_store.get_frecency_score(a);
            let score_b = frecency_store.get_frecency_score(b);
            
            score_b.partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
        
        Ok(apps)
    }
    
    pub fn record_launch(&mut self, app_name: &str) -> Result<()> {
        let frecency_store = self.get_frecency_store_mut()?;
        frecency_store.add(app_name);
        self.save_frecency_store()?;
        Ok(())
    }
}
```

### Frecency Score Calculation
```rust
// Get current frecency score for an item
let current_time = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs_f64();

let score = item_stats.get_frecency(current_time);
```

## Implementation Plan

### Phase 1: Basic Integration
1. Add `fre` dependency to Cargo.toml
2. Create frecency store management in cache.rs
3. Integrate frecency sorting with existing application list
4. Record launches in the execution function

### Phase 2: Advanced Features
1. Configurable half-life settings
2. Frecency store cleanup (remove unused items)
3. Import/export functionality
4. Statistics and debugging commands

### Phase 3: Optimization
1. Lazy loading of frecency store
2. Batch updates for better performance
3. Memory-efficient storage for large application lists

## Configuration Options

### Half-life Settings
- **Short** (1 hour): Heavily favors recent usage
- **Medium** (1 day): Balanced approach (recommended)
- **Long** (1 week): Emphasizes long-term patterns

### Store Management
- **Auto-cleanup**: Remove items not accessed in 30 days
- **Max items**: Limit store size to top 1000 applications
- **Backup**: Periodic backup of frecency data

## Benefits for InstantCLI

1. **Smart Ordering**: Most-used applications appear first
2. **Adaptive**: Learns user patterns over time
3. **Fast Startup**: Frecency calculation is O(1) per item
4. **Persistent**: Survives application restarts
5. **Configurable**: Tunable for different usage patterns

## Technical Notes

- Frecency scores are floating-point numbers
- Higher scores indicate more frequent/recent usage
- Scores naturally decay over time without usage
- The algorithm is designed to be stable and predictable
- JSON serialization handles persistence automatically
