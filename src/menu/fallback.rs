use super::processing::RequestProcessor;
use super::protocol::{MenuRequest, MenuResponse};
use anyhow::{Context, Result};
use std::fs;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64},
};

pub fn run_worker(request_file: &str, response_file: &str) -> Result<i32> {
    let request_json = fs::read_to_string(request_file)
        .with_context(|| format!("Failed to read fallback request file: {request_file}"))?;

    let request: MenuRequest = serde_json::from_str(&request_json)
        .context("Failed to deserialize fallback menu request")?;

    let processor =
        RequestProcessor::new(Arc::new(AtomicBool::new(true)), Arc::new(AtomicU64::new(0)));

    let response: MenuResponse = processor.process_internal(request)?;

    let response_json =
        serde_json::to_string(&response).context("Failed to serialize fallback menu response")?;

    fs::write(response_file, response_json)
        .with_context(|| format!("Failed to write fallback response file: {response_file}"))?;

    Ok(0)
}
