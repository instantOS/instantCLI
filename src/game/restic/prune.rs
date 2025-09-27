use crate::game::config::InstantGameConfig;
use crate::game::restic::{cache, tags};
use crate::game::utils::validation;
use crate::restic::wrapper::{ResticWrapper, Snapshot};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

const REPO_MISSING_CONTEXT: &str = "Failed to load game configuration";

const RETENTION_POLICY_RULES: [(&str, &str); 5] = [
    ("keep-last", "30"),
    ("keep-daily", "90"),
    ("keep-weekly", "52"),
    ("keep-monthly", "36"),
    ("keep-yearly", "10"),
];

pub fn prune_snapshots(zero_changes: bool) -> Result<()> {
    if zero_changes {
        return prune_zero_change_snapshots();
    }

    prune_with_retention()
}

fn prune_with_retention() -> Result<()> {
    let game_config = InstantGameConfig::load().context(REPO_MISSING_CONTEXT)?;
    validation::check_restic_and_game_manager(&game_config)?;

    let restic = ResticWrapper::new(
        game_config.repo.as_path().to_string_lossy().to_string(),
        game_config.repo_password.clone(),
    );

    println!("Applying retention policy using restic forget...");

    restic
        .forget_with_policy(
            Some(vec![tags::INSTANT_GAME_TAG.to_string()]),
            Some(vec!["host".to_string(), "tags".to_string()]),
            &RETENTION_POLICY_RULES,
            true,
        )
        .context("Failed to apply retention policy with restic forget")?;

    cache::invalidate_snapshot_cache();

    println!(
        "Retention policy applied per game: keep last {0}, daily {1}, weekly {2}, monthly {3}, yearly {4} snapshots.",
        RETENTION_POLICY_RULES[0].1,
        RETENTION_POLICY_RULES[1].1,
        RETENTION_POLICY_RULES[2].1,
        RETENTION_POLICY_RULES[3].1,
        RETENTION_POLICY_RULES[4].1,
    );
    println!("Prune completed successfully.");

    Ok(())
}

fn prune_zero_change_snapshots() -> Result<()> {
    let game_config = InstantGameConfig::load().context(REPO_MISSING_CONTEXT)?;
    validation::check_restic_and_game_manager(&game_config)?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    let restic = ResticWrapper::new(repo_path.clone(), game_config.repo_password.clone());

    let snapshots_json = restic
        .list_snapshots_filtered(Some(vec![tags::INSTANT_GAME_TAG.to_string()]))
        .context("Failed to list restic snapshots")?;

    let mut snapshots: Vec<Snapshot> =
        serde_json::from_str(&snapshots_json).context("Failed to parse snapshot data")?;

    if snapshots.is_empty() {
        println!("No game snapshots found to prune.");
        return Ok(());
    }

    let mut grouped: HashMap<String, Vec<Snapshot>> = HashMap::new();
    for snapshot in snapshots.drain(..) {
        if let Some(game_name) = tags::extract_game_name_from_tags(&snapshot.tags) {
            grouped.entry(game_name).or_default().push(snapshot);
        }
    }

    let mut prune_targets = Vec::new();
    let mut affected_games = HashSet::new();
    let mut removed_details = Vec::new();

    for (game_name, mut game_snapshots) in grouped.into_iter() {
        game_snapshots
            .sort_by(|a, b| parse_snapshot_time(&b.time).cmp(&parse_snapshot_time(&a.time)));
        let zero_change: Vec<&Snapshot> = game_snapshots
            .iter()
            .filter(|snap| is_zero_change(snap))
            .collect();

        if zero_change.len() <= 1 {
            continue;
        }

        for snapshot in zero_change.into_iter().skip(1) {
            prune_targets.push(snapshot.id.clone());
            affected_games.insert(game_name.clone());
            removed_details.push((
                game_name.clone(),
                snapshot.short_id.clone(),
                snapshot.time.clone(),
            ));
        }
    }

    if prune_targets.is_empty() {
        println!("No redundant zero-change snapshots found.");
        return Ok(());
    }

    println!(
        "Pruning {} redundant zero-change snapshots across {} games...",
        prune_targets.len(),
        affected_games.len()
    );
    for (game, short_id, time) in &removed_details {
        println!("  • {} – {} ({})", game, short_id, time);
    }

    restic
        .forget_snapshots(&prune_targets, true)
        .context("Failed to prune redundant snapshots")?;

    for game in affected_games {
        cache::invalidate_game_cache(&game, &repo_path);
    }

    println!("Prune completed successfully.");
    Ok(())
}

fn is_zero_change(snapshot: &Snapshot) -> bool {
    match &snapshot.summary {
        Some(summary) => summary.files_new == 0 && summary.files_changed == 0,
        None => false,
    }
}

fn parse_snapshot_time(time: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(time)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH))
}
