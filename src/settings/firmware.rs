use crate::common::requirements::{InstallTest, RequiredPackage};

pub const GNOME_FIRMWARE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "GNOME Firmware manager",
    arch_package_name: Some("gnome-firmware"),
    ubuntu_package_name: Some("gnome-firmware"),
    tests: &[InstallTest::WhichSucceeds("gnome-firmware")],
};
