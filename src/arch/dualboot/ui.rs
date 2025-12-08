use super::format_size;
use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper};
use anyhow::{Result, anyhow};

#[derive(Clone)]
struct AllocationOption {
    percentage: u8,
    linux_bytes: u64,
}

impl FzfSelectable for AllocationOption {
    fn fzf_display_text(&self) -> String {
        format!(
            "{:>3}% for Linux ({})",
            self.percentage,
            format_size(self.linux_bytes)
        )
    }
}

/// Show a slider-like interface for allocating disk space
pub fn show_allocation_slider(disk_size: u64, existing_os_min: u64, linux_min: u64) -> Result<u64> {
    // Validate inputs
    if existing_os_min + linux_min > disk_size {
        return Err(anyhow!("Disk too small for both OSs"));
    }

    let mut options = Vec::new();

    // Generate percentage options from 10% to 90%
    for p in (10..=90).step_by(5) {
        let linux_size = (disk_size as f64 * p as f64 / 100.0) as u64;
        let remaining = disk_size.saturating_sub(linux_size);

        // Filter out invalid options
        if linux_size >= linux_min && remaining >= existing_os_min {
            options.push(AllocationOption {
                percentage: p,
                linux_bytes: linux_size,
            });
        }
    }

    if options.is_empty() {
        return Err(anyhow!("No valid allocation options available"));
    }

    // Default to ~50% or closest valid option
    let default_idx = options
        .iter()
        .position(|o| o.percentage >= 50)
        .unwrap_or(options.len() / 2);

    match FzfWrapper::builder()
        .prompt("Allocate disk space for Linux")
        .header("Select how much space to allocate for the new Linux installation.\nThe remaining space will be kept for the existing OS.")
        .initial_index(default_idx)
        .select(options)?
    {
        FzfResult::Selected(option) => Ok(option.linux_bytes),
        FzfResult::Cancelled => Err(anyhow!("Allocation cancelled")),
        FzfResult::Error(e) => Err(anyhow!("FZF error: {}", e)),
        _ => Err(anyhow!("Unexpected selection result")),
    }
}
