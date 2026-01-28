use std::process::Command;

use anyhow::Result;

use crate::preview::PreviewContext;
use crate::preview::helpers::{PreviewBuilderExt, indent_lines};
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_disk_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(disk) = ctx.key() else {
        return Ok(String::new());
    };

    if which::which("lsblk").is_err() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::HardDrive, "Disk Overview")
            .text("lsblk not found on this system.")
            .build_string());
    }

    let warning = hex_to_ansi_fg(colors::YELLOW);
    let ok = hex_to_ansi_fg(colors::GREEN);
    let reset = "\x1b[0m";

    let mount_lines = disk_mount_status(disk);

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::HardDrive, "Disk Overview")
        .subtext("Selecting a disk will erase all data on it.")
        .blank()
        .line(colors::YELLOW, Some(NerdFont::Warning), "Mount Status");

    if mount_lines.is_empty() {
        builder = builder.raw(&format!("{ok}  No mounted partitions detected{reset}"));
    } else {
        builder = builder.raw(&format!("{warning}  Mounted partitions detected{reset}"));
        for line in &mount_lines {
            builder = builder.raw(&format!("    {line}"));
        }
        builder = builder.raw(&format!("{warning}  Unmount before proceeding.{reset}"));
    }

    let device_lines = lsblk_lines(&["-d", "-l", "-n", "-o", "NAME,SIZE,MODEL,TYPE", disk]);
    builder = builder
        .blank()
        .line(colors::TEAL, None, "Device")
        .raw_lines(&indent_lines(&device_lines, "  "))
        .blank()
        .line(colors::TEAL, None, "Partitions")
        .raw_lines(&indent_lines(
            &lsblk_lines(&["-l", "-n", "-o", "NAME,SIZE,FSTYPE,MOUNTPOINT", disk]),
            "  ",
        ));

    Ok(builder.build_string())
}

pub(crate) fn render_partition_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(part) = ctx.key() else {
        return Ok(String::new());
    };

    if which::which("lsblk").is_err() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Partition, "Partition Details")
            .text("lsblk not found on this system.")
            .build_string());
    }

    let overview = lsblk_lines(&["-l", "-n", "-o", "NAME,SIZE,FSTYPE,MOUNTPOINT", part]);
    let identifiers = lsblk_lines(&["-l", "-n", "-o", "NAME,UUID,PARTUUID", part]);

    let builder = PreviewBuilder::new()
        .header(NerdFont::Partition, "Partition Details")
        .subtext("Verify the filesystem and mount point before selecting.")
        .blank()
        .line(colors::TEAL, None, "Overview")
        .raw_lines(&indent_lines(&overview, "  "))
        .blank()
        .line(colors::TEAL, None, "Identifiers")
        .raw_lines(&indent_lines(&identifiers, "  "))
        .build_string();

    Ok(builder)
}

fn disk_mount_status(disk: &str) -> Vec<String> {
    let lines = lsblk_lines(&["-l", "-n", "-o", "NAME,MOUNTPOINT", disk]);
    let mut mounted = Vec::new();

    for line in lines {
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or("");
        let mount = parts.collect::<Vec<_>>().join(" ");
        if !mount.is_empty() {
            mounted.push(format!("{name} -> {mount}"));
        }
    }

    mounted
}

fn lsblk_lines(args: &[&str]) -> Vec<String> {
    let output = Command::new("lsblk").args(args).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.trim_end().to_string())
        .collect()
}
