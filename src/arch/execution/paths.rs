use std::path::PathBuf;

pub const STATE_FILE: &str = "/etc/instant/install_state.toml";
pub const CONFIG_FILE: &str = "/etc/instant/install_config.toml";
pub const DRY_RUN_FLAG: &str = "/etc/instant/installdryrun";
pub const CHROOT_MOUNT: &str = "/mnt";

pub fn chroot_path(path: &str) -> PathBuf {
    PathBuf::from(CHROOT_MOUNT).join(path.trim_start_matches('/'))
}
