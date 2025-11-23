use anyhow::Result;

pub struct TimezoneProvider;

#[async_trait::async_trait]
impl crate::arch::engine::AsyncDataProvider for TimezoneProvider {
    async fn provide(&self, context: &crate::arch::engine::InstallContext) -> Result<()> {
        // Simulate filesystem scan for timezones
        // In reality: walkdir /usr/share/zoneinfo
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        {
            let mut data = context.data.lock().unwrap();
            data.insert(
                "timezones".to_string(),
                "Europe/Berlin\nEurope/London\nAmerica/New_York".to_string(),
            );
        }
        Ok(())
    }
}
