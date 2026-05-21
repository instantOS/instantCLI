use std::time::Duration;

use anyhow::{Context, Result, anyhow};

use super::types::TimeRange;

pub fn parse_time_range(input: &str) -> Result<TimeRange> {
    let (start, end) = input
        .split_once('-')
        .ok_or_else(|| anyhow!("Missing `-` separator in timestamp range"))?;
    let start = parse_timestamp(start.trim())?;
    let end = parse_timestamp(end.trim())?;
    if end <= start {
        return Err(anyhow!("Timestamp range end must be greater than start"));
    }
    Ok(TimeRange { start, end })
}

pub fn parse_timestamp(value: &str) -> Result<Duration> {
    let (main, frac) = value
        .split_once('.')
        .ok_or_else(|| anyhow!("Timestamp must include fractional seconds"))?;
    let parts = main.split(':').collect::<Vec<_>>();

    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => {
            let minutes: u64 = minutes
                .parse()
                .with_context(|| format!("Invalid minute component in timestamp `{}`", value))?;
            let seconds: u64 = seconds
                .parse()
                .with_context(|| format!("Invalid second component in timestamp `{}`", value))?;
            (0, minutes, seconds)
        }
        [hours, minutes, seconds] => {
            let hours: u64 = hours
                .parse()
                .with_context(|| format!("Invalid hour component in timestamp `{}`", value))?;
            let minutes: u64 = minutes
                .parse()
                .with_context(|| format!("Invalid minute component in timestamp `{}`", value))?;
            let seconds: u64 = seconds
                .parse()
                .with_context(|| format!("Invalid second component in timestamp `{}`", value))?;
            (hours, minutes, seconds)
        }
        _ => {
            return Err(anyhow!(
                "Timestamp must be in HH:MM:SS.xxx or MM:SS.xxx format"
            ));
        }
    };
    let raw_millis: u64 = frac
        .parse()
        .with_context(|| format!("Invalid millisecond component in timestamp `{}`", value))?;

    let milliseconds = match frac.len() {
        0 => 0,
        1 => raw_millis * 100,
        2 => raw_millis * 10,
        3 => raw_millis,
        len => raw_millis / 10_u64.pow((len - 3) as u32),
    };

    if minutes >= 60 {
        return Err(anyhow!(
            "Minutes component must be less than 60 in `{}`",
            value
        ));
    }
    if seconds >= 60 {
        return Err(anyhow!(
            "Seconds component must be less than 60 in `{}`",
            value
        ));
    }

    let total_millis = hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds;
    Ok(Duration::from_millis(total_millis))
}
