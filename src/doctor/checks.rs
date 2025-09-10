use super::{CheckStatus, DoctorCheck};
use anyhow::Result;
use async_trait::async_trait;
use tokio::process::{Child, Command as TokioCommand};

pub struct InternetCheck;

#[async_trait]
impl DoctorCheck for InternetCheck {
    fn name(&self) -> &'static str {
        "Internet Connectivity"
    }

    async fn execute(&self) -> CheckStatus {
        let output = TokioCommand::new("ping")
            .arg("-c")
            .arg("1")
            .arg("-W")
            .arg("1")
            .arg("8.8.8.8")
            .output()
            .await;

        match output {
            Ok(output) if output.status.success() => {
                CheckStatus::Pass("Internet connection is available".to_string())
            }
            _ => CheckStatus::Fail("No internet connection detected".to_string()),
        }
    }

    fn fix_message(&self) -> Option<String> {
        Some("Run nmtui to configure your network interface.".to_string())
    }

    async fn fix(&self) -> Result<()> {
        let status = TokioCommand::new("nmtui").status().await?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("nmtui failed to run"))
        }
    }
}
