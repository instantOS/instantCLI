use anyhow::Result;

pub(super) fn handle_info_command() -> Result<()> {
    let info = crate::arch::engine::SystemInfo::detect();
    info.print_system_info();
    Ok(())
}
