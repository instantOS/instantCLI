use super::DoctorCommands;
use super::fix::{fix_all_checks, fix_batch_checks, fix_interactive, fix_single_check};
use super::run::{list_available_checks, run_all_checks_cmd, run_single_check};
use anyhow::{Result, bail};

pub async fn handle_doctor_command(
    command: Option<DoctorCommands>,
    max_concurrency: usize,
) -> Result<()> {
    match command {
        None => run_all_checks_cmd(max_concurrency).await,
        Some(DoctorCommands::List) => list_available_checks().await,
        Some(DoctorCommands::Run { name }) => run_single_check(&name).await,
        Some(DoctorCommands::Fix {
            name,
            all,
            choose,
            batch_ids,
        }) => {
            if let Some(ids) = batch_ids {
                fix_batch_checks(ids).await
            } else if choose {
                fix_interactive(max_concurrency).await
            } else if all {
                fix_all_checks(max_concurrency).await
            } else if let Some(check_name) = name {
                fix_single_check(&check_name).await
            } else {
                bail!("Either --all, --choose, or a check name must be provided")
            }
        }
    }
}
