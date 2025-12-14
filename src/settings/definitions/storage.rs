//! Storage settings (additional)
//!
//! Disk management and partition editor.

use crate::settings::deps::{GNOME_DISKS, GPARTED};
use crate::ui::prelude::*;

gui_command_setting!(
    DiskManagement,
    "storage.disks",
    "Disk management",
    NerdFont::HardDrive,
    "Launch GNOME Disks to manage drives and partitions.",
    "gnome-disks",
    &GNOME_DISKS
);

gui_command_setting!(
    PartitionEditor,
    "storage.gparted",
    "Partition editor",
    NerdFont::Partition,
    "Launch GParted for advanced partition management (requires root).",
    "gparted",
    &GPARTED
);
