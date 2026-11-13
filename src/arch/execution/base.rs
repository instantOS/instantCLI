use super::CommandRunner;
use crate::arch::engine::InstallPlan;
use anyhow::{Context, Result};

use super::package_source::PackageSource;

pub async fn install_base(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up mirrors...");
    setup_mirrors(plan, executor).await?;

    println!("Configuring pacman settings...");
    PackageSource::resolve(plan.linux_variant())
        .configure_bootstrap_pacman(plan.linux_variant(), executor.dry_run())
        .await?;

    println!("Installing base system with {}...", plan.kernel().label());
    bootstrap_base_system(plan, executor)?;

    Ok(())
}

/// Region list to write under the bundle line in an opportunistic install.
/// `None` when no region was selected or the bundle snapshot cannot resolve
/// it — the caller then keeps the shipped fallback (a warning is logged for
/// a selected-but-unresolvable region).
fn offline_region_list(region_name: Option<&String>) -> Option<String> {
    let name = region_name?;
    match crate::arch::mirrors::bundled_region_mirrorlist(name) {
        Ok(list) => Some(list),
        Err(e) => {
            println!("Warning: {e:#}; keeping the shipped fallback mirrorlist.");
            None
        }
    }
}

async fn setup_mirrors(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    // Artix has no archlinux.org mirror regions. The live ISO already ships
    // the right Artix mirrorlist, so keep it and never fetch an Arch one.
    if plan.linux_variant().is_artix() {
        println!(
            "Keeping the Artix mirrorlist; Arch mirror regions do not apply to {}.",
            plan.linux_variant().label()
        );
        return Ok(());
    }

    // Check if a region was selected (question may have been skipped if fetch failed)
    let region_name = plan.mirror_region().as_ref();
    let source = PackageSource::resolve(plan.linux_variant());

    match crate::arch::offline::arch_mirrorlist_action(crate::arch::offline::mode()) {
        crate::arch::offline::MirrorlistAction::Fetch => {}
        crate::arch::offline::MirrorlistAction::Keep => {
            if executor.dry_run() {
                match region_name {
                    Some(region) => println!(
                        "[DRY RUN] Offline mirrorlist: bundle first, selected region {region} below."
                    ),
                    None => println!("[DRY RUN] Keeping the bundled offline mirrorlist"),
                }
                return Ok(());
            }
            match offline_region_list(region_name) {
                Some(list) => {
                    let content = format!(
                        "{}\n{}\n",
                        crate::arch::offline::file_server_line(),
                        list.trim_end()
                    );
                    source.apply_mirrorlist(&content, false)?;
                    println!(
                        "Offline bundle detected: bundle first, selected region below (stripped from the target at finish)."
                    );
                }
                None => println!(
                    "Offline bundle detected: keeping the shipped mirrorlist (file:// first, network mirrors below)."
                ),
            }
            return Ok(());
        }
        crate::arch::offline::MirrorlistAction::Replace(content) => {
            if executor.dry_run() {
                println!("[DRY RUN] Writing file://-only mirrorlist (strict offline)");
                return Ok(());
            }
            source.apply_mirrorlist(&content, false)?;
            println!("Strict offline mode: mirrorlist restricted to the offline bundle.");
            return Ok(());
        }
    }

    if executor.dry_run() {
        match region_name {
            Some(region) => {
                println!("[DRY RUN] Fetching mirrorlist for region: {}", region);
            }
            None => {
                println!("[DRY RUN] Using fallback mirrorlist (region selection was skipped)");
            }
        }
        if source.is_isolated() {
            println!(
                "[DRY RUN] Writing the selected mirrorlist to the target and to a private pacman configuration (the host's mirrorlist is left untouched)"
            );
        } else {
            println!("[DRY RUN] Writing to /etc/pacman.d/mirrorlist");
        }
        return Ok(());
    }

    let mirrorlist = match region_name {
        Some(region) => {
            println!("Selected region: {}", region);
            mirrorlist_with_fallback(
                fetch_mirrorlist_for_region(region).await,
                fetch_fallback_mirrorlist(),
            )
            .await?
        }
        None => {
            // Fallback path: region selection was skipped (fetch failed)
            println!("Mirror region selection was skipped, using fallback mirrorlist...");
            fetch_fallback_mirrorlist().await?
        }
    };

    // Write to the run's own mirrorlist. On a live ISO that is the host's
    // file, which the bootstrap command copies into the target; on a running system it
    // is the installer's private copy, and the host is left alone.
    source.apply_mirrorlist(&mirrorlist, false)?;
    println!("Mirrors updated.");

    Ok(())
}

async fn fetch_mirrorlist_for_region(region_name: &str) -> Result<String> {
    let regions = crate::arch::mirrors::fetch_mirror_regions().await?;
    let region_code = regions
        .get(region_name)
        .context(format!("Could not find code for region: {}", region_name))?;

    println!("Fetching mirrors for code: {}", region_code);
    crate::arch::mirrors::fetch_mirrorlist(region_code).await
}

async fn fetch_fallback_mirrorlist() -> Result<String> {
    // Use empty region code to trigger fallback chain in fetch_mirrorlist
    crate::arch::mirrors::fetch_mirrorlist("").await
}

/// A saved region is a preference, even when its provider is unavailable.
async fn mirrorlist_with_fallback(
    selected: Result<String>,
    fallback: impl std::future::Future<Output = Result<String>>,
) -> Result<String> {
    match selected {
        Ok(mirrorlist) => Ok(mirrorlist),
        Err(error) => {
            println!(
                "Warning: Could not fetch selected mirrors: {error:#}; using fallback mirrors."
            );
            fallback
                .await
                .context("Failed to fetch fallback mirrorlist")
        }
    }
}

fn bootstrap_base_system(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let use_encryption = plan.storage().encryption().is_some();
    let use_plymouth = plan.initramfs().plymouth().is_some();

    let packages = plan.base_packages();

    if use_encryption {
        println!("Encryption enabled; required packages will be installed inside chroot.");
    }

    if use_plymouth {
        println!("Plymouth enabled; package will be installed after chroot.");
    }

    println!("Packages to install: {}", packages.join(" "));

    let packages_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();

    super::pacman::bootstrap_packages("/mnt", &packages_refs, executor, plan.linux_variant())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saved_region_failure_uses_fallback_and_propagates_total_failure() {
        let selected = || anyhow::anyhow!("region provider unavailable");
        assert_eq!(
            mirrorlist_with_fallback(Err(selected()), async { Ok("fallback mirrors".to_owned()) })
                .await
                .unwrap(),
            "fallback mirrors"
        );
        let error = mirrorlist_with_fallback(Err(selected()), async {
            anyhow::bail!("fallback unavailable")
        })
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains("fallback unavailable"));
    }

    #[tokio::test]
    async fn selected_mirrors_do_not_poll_fallback() {
        let mirrors = mirrorlist_with_fallback(Ok("selected mirrors".to_owned()), async {
            panic!("successful region lookup must not fetch fallback mirrors")
        })
        .await
        .unwrap();
        assert_eq!(mirrors, "selected mirrors");
    }
}
