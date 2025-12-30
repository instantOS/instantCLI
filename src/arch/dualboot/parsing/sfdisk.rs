//! sfdisk JSON parsing for free space calculation

use crate::arch::dualboot::types::FreeRegion;
use anyhow::Context;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub struct SfdiskOutput {
    pub partitiontable: SfdiskPartitionTable,
}

#[derive(Debug, Deserialize)]
pub struct SfdiskPartitionTable {
    // label: String, // e.g. "gpt", "dos" - unused for now
    pub firstlba: Option<u64>,
    pub lastlba: Option<u64>,
    pub size: Option<u64>, // total sectors (may be present for MBR)
    pub sectorsize: u64,
    pub partitions: Option<Vec<SfdiskPartition>>,
}

#[derive(Debug, Deserialize)]
pub struct SfdiskPartition {
    // node: String, // e.g. "test.img1" - unused
    pub start: u64,
    pub size: u64,
    // type: String, // unused
}

/// Calculate free regions by finding gaps in the partition table
pub fn calculate_free_regions_from_json(
    json: &str,
    disk_size_bytes: Option<u64>,
) -> anyhow::Result<Vec<FreeRegion>> {
    let output: SfdiskOutput =
        serde_json::from_str(json).context("Failed to parse sfdisk JSON output")?;

    let pt = output.partitiontable;
    let sector_size = pt.sectorsize;
    let disk_size_sectors = disk_size_bytes.and_then(|b| {
        if sector_size > 0 {
            Some(b / sector_size)
        } else {
            None
        }
    });
    let mut partitions = pt.partitions.unwrap_or_default();

    let first_lba = pt
        .firstlba
        .or_else(|| partitions.iter().map(|p| p.start).min())
        .unwrap_or(0);

    let last_lba = pt
        .lastlba
        .or_else(|| pt.size.map(|s| s.saturating_sub(1)))
        .or_else(|| disk_size_sectors.map(|s| s.saturating_sub(1)))
        .or_else(|| {
            partitions
                .iter()
                .map(|p| p.start.saturating_add(p.size).saturating_sub(1))
                .max()
        })
        .map(|l| l.max(first_lba))
        .unwrap_or(first_lba);

    // Sort partitions by start sector to reliably find gaps
    partitions.sort_by_key(|p| p.start);

    let mut regions = Vec::new();
    let mut current_sector = first_lba;

    // Check gaps between partitions
    for partition in partitions {
        if partition.start > current_sector {
            let gap_sectors = partition.start - current_sector;
            // Only consider gaps large enough to be usable (e.g., > 1MB)
            // 1MB = 2048 sectors (at 512 bytes/sector)
            if gap_sectors > 2048 {
                regions.push(FreeRegion {
                    start: current_sector,
                    sectors: gap_sectors,
                    size_bytes: gap_sectors * sector_size,
                });
            }
        }
        current_sector = std::cmp::max(current_sector, partition.start + partition.size);
    }

    // Check gap at the end (between last partition and lastlba)
    if current_sector <= last_lba {
        let gap_sectors = (last_lba - current_sector) + 1; // lastlba is inclusive

        // Only consider gaps large enough (> 1MB)
        if gap_sectors > 2048 {
            regions.push(FreeRegion {
                start: current_sector,
                sectors: gap_sectors,
                size_bytes: gap_sectors * sector_size,
            });
        }
    }

    Ok(regions)
}

/// Get all contiguous free regions for a device
pub fn get_free_regions(
    device: &str,
    disk_size_bytes: Option<u64>,
) -> anyhow::Result<Vec<FreeRegion>> {
    // Run sfdisk -J <device> to get partition table in JSON
    let output = Command::new("sfdisk")
        .args(["-J", device])
        .output()
        .context("Failed to run sfdisk -J")?;

    if !output.status.success() {
        // If it returns error (e.g. no partition table, empty disk), we assume no dual boot targets.
        // Return empty regions rather than error
        return Ok(Vec::new());
    }

    let json_output = String::from_utf8_lossy(&output.stdout);
    calculate_free_regions_from_json(&json_output, disk_size_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_free_regions_simple_gap() {
        // 100 sectors total. Partition at 20-30.
        // Free: 0-19 (if firstlba=0), 31-100.
        // firstlba typically 34 or 2048 for GPT. Let's say 34.

        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "id": "A",
      "device": "test",
      "unit": "sectors",
      "firstlba": 34,
      "lastlba": 10000,
      "sectorsize": 512,
      "partitions": [
         {
            "node": "p1",
            "start": 5000,
            "size": 1000
         }
      ]
   }
}"#;
        // Gaps:
        // 1. 34 to 4999 (size 4966)
        // 2. 6000 to 10000 (size 4001)

        // Note: Our logic filters < 2048 sectors (1MB).
        // 4966 > 2048 -> Keep
        // 4001 > 2048 -> Keep

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        assert_eq!(regions.len(), 2);

        assert_eq!(regions[0].start, 34);
        assert_eq!(regions[0].sectors, 4966);

        assert_eq!(regions[1].start, 6000);
        assert_eq!(regions[1].sectors, 4001);
    }

    #[test]
    fn test_calculate_free_regions_contiguous_user_scenario() {
        // User scenario:
        // 4GB free at start? (Assume existing partitions start late)
        // 5GB Linux
        // 2GB EFI
        // 26GB free at end

        // Let's model roughly in sectors (512b)
        // 1GB = 2,097,152 sectors
        // 4GB ~= 8,388,608 sectors

        // Case:
        // P1 Start: 8,388,608 + 2048 (offset). Size: 10,000,000
        // P2 Start: P1_End + 1. Size: 4,000,000
        // Disk End: Very large

        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "firstlba": 2048,
      "lastlba": 100000000,
      "sectorsize": 512,
      "partitions": [
         {
            "start": 10000000,
            "size": 10000000
         },
         {
            "start": 20000000,
            "size": 4000000
         }
      ]
   }
}"#;
        // Gaps:
        // 1. 2048 to 9,999,999. Size ~ 10M sectors (~5GB)
        // 2. 24,000,000 to 100,000,000. Size ~ 76M sectors (~38GB)

        let regions = calculate_free_regions_from_json(json, None).unwrap();

        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].start, 2048);
        assert!(regions[0].size_bytes > 4 * 1024 * 1024 * 1024); // > 4GB

        assert_eq!(regions[1].start, 24000000);
        assert!(regions[1].size_bytes > 20 * 1024 * 1024 * 1024); // > 20GB
    }

    #[test]
    fn test_calculate_free_regions_no_partitions() {
        // Empty partition table (e.g. freshly initialized GPT but no partitions)
        let json = r#"{
   "partitiontable": {
      "label": "gpt",
      "firstlba": 2048,
      "lastlba": 100000,
      "sectorsize": 512,
      "partitions": []
   }
}"#;
        let regions = calculate_free_regions_from_json(json, None).unwrap();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 2048);
        assert_eq!(regions[0].sectors, 100000 - 2048 + 1);
    }

    #[test]
    fn test_calculate_free_regions_missing_lba_fields_dos() {
        // DOS/MBR outputs do not include firstlba/lastlba; ensure we still parse without crashing.
        let json = r#"{
    "partitiontable": {
        "label": "dos",
        "device": "test",
        "unit": "sectors",
        "sectorsize": 512,
        "partitions": [
            {
                "start": 2048,
                "size": 4096
            }
        ]
    }
}"#;

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        // With no reported disk end we cannot infer trailing free space; we just ensure parsing works.
        assert!(regions.is_empty());
    }

    #[test]
    fn test_calculate_free_regions_missing_lba_with_size() {
        // DOS/MBR with size field should infer disk end from size.
        let json = r#"{
    "partitiontable": {
        "label": "dos",
        "device": "test",
        "unit": "sectors",
        "size": 100000,
        "sectorsize": 512,
        "partitions": [
            {
                "start": 2048,
                "size": 4096
            }
        ]
    }
}"#;

        let regions = calculate_free_regions_from_json(json, None).unwrap();
        // Gap after the single partition to the end of disk.
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 2048 + 4096); // partition end + 1
        assert_eq!(regions[0].sectors, 100000 - (2048 + 4096));
    }
}
