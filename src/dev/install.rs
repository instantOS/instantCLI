use crate::common::git;
use crate::common::progress::{create_spinner, finish_spinner_with_success};
use crate::dev::fuzzy::select_package;
use crate::dev::package::Package;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use duct::cmd;
use std::path::PathBuf;

pub struct PackageRepo {
    pub path: PathBuf,
    pub url: String,
}

impl PackageRepo {
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find cache directory"))?
            .join("instant");

        std::fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;

        let repo_path = cache_dir.join("packages");
        let url = "https://github.com/instantOS/packages".to_string();

        Ok(PackageRepo {
            path: repo_path,
            url,
        })
    }

    pub fn ensure_updated(&self) -> Result<()> {
        if self.path.exists() {
            if git::has_local_changes(&self.path)? {
                self.handle_local_changes()?;
            }

            git::clean_and_pull(&self.path).context("Failed to pull latest changes")?;
        } else {
            // Clone repository
            git::clone_repo(&self.url, &self.path, Some("main"), Some(3))
                .context("Failed to clone repository")?;
        }

        Ok(())
    }

    fn handle_local_changes(&self) -> Result<()> {
        emit(
            Level::Warn,
            "dev.install.local_changes",
            &format!(
                "{} Local changes detected in package repository",
                char::from(NerdFont::Warning)
            ),
            None,
        );
        emit(
            Level::Info,
            "dev.install.stash",
            &format!("{} Stashing local changes...", char::from(NerdFont::Info)),
            None,
        );

        git::stash_local_changes(&self.path, "Auto-stash by instantCLI")?;

        Ok(())
    }
}

impl Package {
    pub fn build_and_install(&self, debug: bool) -> Result<()> {
        if debug {
            let message = format!(
                "{} Building package: {}",
                char::from(NerdFont::Bug),
                self.name
            );
            emit(Level::Debug, "dev.install.build.start", &message, None);
        }

        let build_message = format!(
            "{} Building and installing {}... (This may be interactive)",
            char::from(NerdFont::Info),
            self.name
        );
        emit(
            Level::Info,
            "dev.install.build.install",
            &build_message,
            None,
        );

        cmd!("makepkg", "-si")
            .dir(&self.path)
            .run()
            .context("Failed to build and install package")?;

        let success_message = format!(
            "{} Successfully installed {}",
            char::from(NerdFont::Check),
            self.name
        );
        emit(
            Level::Success,
            "dev.install.success",
            &success_message,
            None,
        );

        Ok(())
    }
}

/// Resolve a package by name using a case-insensitive exact match.
///
/// - Exactly one match → returns it.
/// - No match → error with a hint to use the interactive picker.
/// - Multiple matches (e.g. names differing only in case) → error listing the
///   ambiguous candidates so the user can disambiguate.
fn resolve_package_by_name<'a>(packages: &'a [Package], name: &str) -> Result<&'a Package> {
    let matches: Vec<&Package> = packages
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(name))
        .collect();

    match matches.len() {
        0 => Err(anyhow::anyhow!(
            "Package '{name}' not found. Run `ins dev install` without arguments to pick interactively."
        )),
        1 => Ok(matches[0]),
        _ => {
            let names: Vec<&str> = matches.iter().map(|p| p.name.as_str()).collect();
            Err(anyhow::anyhow!(
                "Multiple packages match '{name}': {}. Please specify the exact name.",
                names.join(", ")
            ))
        }
    }
}

pub async fn handle_install(debug: bool, package: Option<String>) -> Result<()> {
    if debug {
        let start_message = format!(
            "{} Starting package installation...",
            char::from(NerdFont::Bug)
        );
        emit(Level::Debug, "dev.install.start", &start_message, None);
    }

    let pb = create_spinner("Preparing package repository...".to_string());

    // Initialize and update repository. Suspend the spinner around the network
    // call so SSH/credential prompts can be answered on the user's terminal.
    let repo = PackageRepo::new()?;
    pb.suspend(|| repo.ensure_updated())?;

    finish_spinner_with_success(pb, "Package repository ready");

    if debug {
        let discover_message = format!("{} Discovering packages...", char::from(NerdFont::Bug));
        emit(
            Level::Debug,
            "dev.install.discover",
            &discover_message,
            None,
        );
    }

    // Discover available packages
    let packages = Package::discover_packages(&repo.path).context("Failed to discover packages")?;

    if packages.is_empty() {
        return Err(anyhow::anyhow!("No packages found in repository"));
    }

    if debug {
        let count_message = format!(
            "{} Found {} packages",
            char::from(NerdFont::Bug),
            packages.len()
        );
        emit(
            Level::Debug,
            "dev.install.packages.count",
            &count_message,
            None,
        );
        for pkg in &packages {
            let item_message = format!(
                "{}  - {} ({:?})",
                char::from(NerdFont::Bug),
                pkg.name,
                pkg.description
            );
            emit(
                Level::Debug,
                "dev.install.packages.item",
                &item_message,
                None,
            );
        }
    }

    // Select package: use the provided name directly, or fall back to the
    // interactive picker when no name was given.
    let selected_package = match &package {
        Some(name) => resolve_package_by_name(&packages, name)
            .cloned()
            .with_context(|| format!("Failed to resolve package '{name}'"))?,
        None => select_package(packages).context("Failed to select package")?,
    };

    if debug {
        let selected_message = format!(
            "{} Selected package: {}",
            char::from(NerdFont::Bug),
            selected_package.name
        );
        emit(
            Level::Debug,
            "dev.install.selected",
            &selected_message,
            None,
        );
    }

    // Build and install package
    selected_package.build_and_install(debug)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pkg(name: &str) -> Package {
        Package {
            name: name.to_string(),
            path: PathBuf::from(format!("/tmp/{name}")),
            description: None,
        }
    }

    #[test]
    fn resolves_single_exact_match() {
        let packages = vec![pkg("foo"), pkg("bar")];
        let resolved = resolve_package_by_name(&packages, "foo").unwrap();
        assert_eq!(resolved.name, "foo");
    }

    #[test]
    fn resolves_case_insensitive_match() {
        let packages = vec![pkg("FooBar")];
        let resolved = resolve_package_by_name(&packages, "foobar").unwrap();
        assert_eq!(resolved.name, "FooBar");
    }

    #[test]
    fn errors_when_no_match() {
        let packages = vec![pkg("foo"), pkg("bar")];
        let err = resolve_package_by_name(&packages, "baz").unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    #[test]
    fn errors_when_ambiguous() {
        let packages = vec![pkg("foo"), pkg("FOO")];
        let err = resolve_package_by_name(&packages, "foo").unwrap_err();
        assert!(
            err.to_string().contains("Multiple packages match"),
            "got: {err}"
        );
    }
}
