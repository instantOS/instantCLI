use anyhow::Result;

use super::super::utils::print_system_info;

pub(super) fn handle_info_command() -> Result<()> {
    let info = crate::arch::engine::SystemInfo::detect();
    print_system_info(&info);
    Ok(())
}
