//! Storage settings (additional)
//!
//! Disk management and partition editor.

use crate::common::requirements::{GNOME_DISKS_PACKAGE, GPARTED_PACKAGE};
use crate::settings::setting::Category;
use crate::ui::prelude::*;

gui_command_setting!(
    DiskManagement,
    "storage.disks",
    "Disk management",
    Category::Storage,
    NerdFont::HardDrive,
    "Launch GNOME Disks to manage drives and partitions.",
    "gnome-disks",
    GNOME_DISKS_PACKAGE
);

gui_command_setting!(
    PartitionEditor,
    "storage.gparted",
    "Partition editor",
    Category::Storage,
    NerdFont::Partition,
    "Launch GParted for advanced partition management (requires root).",
    "gparted",
    GPARTED_PACKAGE
);
