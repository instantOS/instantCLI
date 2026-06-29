use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::PackageManager;

/// Return installed packages that must be removed with the selected packages.
///
/// The selected packages themselves are excluded from the returned list.
pub fn removal_cascade(
    manager: PackageManager,
    packages: &[String],
    selected_package_info: Option<&str>,
) -> Result<Vec<String>> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    match manager {
        PackageManager::Pacman | PackageManager::Aur => {
            pacman_removal_cascade(packages, selected_package_info)
        }
        PackageManager::Apt => apt_removal_cascade(packages),
        _ => Ok(Vec::new()),
    }
}

fn pacman_removal_cascade(
    packages: &[String],
    selected_package_info: Option<&str>,
) -> Result<Vec<String>> {
    let fetched_package_info;
    let package_info = if let Some(package_info) = selected_package_info {
        package_info
    } else {
        let selected_output = Command::new("pacman")
            .arg("-Qi")
            .args(packages)
            .output()
            .context("Failed to inspect selected pacman packages")?;

        if !selected_output.status.success() {
            bail!("Failed to inspect selected pacman packages");
        }

        fetched_package_info = String::from_utf8_lossy(&selected_output.stdout).into_owned();
        &fetched_package_info
    };

    let selected_required_by = parse_pacman_required_by_map(package_info);
    if !has_selected_dependents(packages, &selected_required_by) {
        return Ok(Vec::new());
    }

    let all_output = Command::new("pacman")
        .arg("-Qi")
        .output()
        .context("Failed to inspect installed pacman packages")?;

    if !all_output.status.success() {
        bail!("Failed to inspect installed pacman packages");
    }

    let all_package_info = String::from_utf8_lossy(&all_output.stdout);
    Ok(dependent_closure(
        packages,
        &parse_pacman_required_by_map(&all_package_info),
    ))
}

fn has_selected_dependents(
    packages: &[String],
    required_by: &BTreeMap<String, Vec<String>>,
) -> bool {
    packages.iter().any(|package| {
        required_by
            .get(package)
            .is_some_and(|items| !items.is_empty())
    })
}

fn apt_removal_cascade(packages: &[String]) -> Result<Vec<String>> {
    let output = Command::new("apt-get")
        .args(["--simulate", "remove"])
        .args(packages)
        .env("LC_ALL", "C")
        .output()
        .context("Failed to simulate APT package removal")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = stderr
            .lines()
            .chain(stdout.lines())
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("APT could not resolve the requested removal");
        bail!("Failed to simulate APT package removal: {detail}");
    }

    Ok(apt_dependents_from_simulation(
        &String::from_utf8_lossy(&output.stdout),
        packages,
    ))
}

fn dependent_closure(
    packages: &[String],
    required_by: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut seen: BTreeSet<String> = packages.iter().cloned().collect();
    let mut queue = packages.to_vec();
    let mut dependents = BTreeSet::new();

    while let Some(package) = queue.pop() {
        if let Some(direct_dependents) = required_by.get(&package) {
            for dependent in direct_dependents {
                if seen.insert(dependent.clone()) {
                    dependents.insert(dependent.clone());
                    queue.push(dependent.clone());
                }
            }
        }
    }

    dependents.into_iter().collect()
}

fn parse_pacman_required_by_map(output: &str) -> BTreeMap<String, Vec<String>> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let name = parse_pacman_field(block, "Name")?;
            Some((name, parse_pacman_required_by(block)))
        })
        .collect()
}

fn parse_pacman_field(output: &str, field: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix(field)
            .and_then(|value| value.split_once(':'))
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn parse_pacman_required_by(output: &str) -> Vec<String> {
    output
        .lines()
        .find_map(|line| {
            line.strip_prefix("Required By")
                .and_then(|value| value.split_once(':'))
        })
        .map(|(_, packages)| {
            packages
                .split_whitespace()
                .filter(|package| *package != "None")
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_apt_simulated_removals(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("Remv "))
        .filter_map(|line| line.split_whitespace().next())
        .map(ToString::to_string)
        .collect()
}

fn apt_dependents_from_simulation(output: &str, selected_packages: &[String]) -> Vec<String> {
    let selected: BTreeSet<&str> = selected_packages.iter().map(String::as_str).collect();
    parse_apt_simulated_removals(output)
        .into_iter()
        .filter(|package| !selected.contains(package.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_transitive_pacman_dependents() {
        let output = "\
Name            : libfoo
Required By     : app-one  app-two

Name            : app-one
Required By     : shell-one

Name            : app-two
Required By     : None
";
        let selected = vec!["libfoo".to_string()];

        assert_eq!(
            dependent_closure(&selected, &parse_pacman_required_by_map(output)),
            vec![
                "app-one".to_string(),
                "app-two".to_string(),
                "shell-one".to_string()
            ]
        );
    }

    #[test]
    fn detects_when_selected_pacman_packages_have_no_dependents() {
        let output = "\
Name            : leaf-one
Required By     : None

Name            : leaf-two
Required By     : None
";
        let selected = ["leaf-one".to_string(), "leaf-two".to_string()];
        let required_by = parse_pacman_required_by_map(output);

        assert!(!has_selected_dependents(&selected, &required_by));
    }

    #[test]
    fn extracts_only_apt_dependents_from_simulation() {
        let output = "\
The following packages will be REMOVED:
  cascade-app cascade-base
0 upgraded, 0 newly installed, 2 to remove and 0 not upgraded.
Remv cascade-app [1]
Remv cascade-base [1]
";

        assert_eq!(
            apt_dependents_from_simulation(output, &["cascade-base".to_string()]),
            vec!["cascade-app".to_string()]
        );
    }
}
