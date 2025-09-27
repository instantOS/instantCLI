use crate::game::config::InstantGameConfig;
use crate::game::restic::{cache, tags};
use crate::game::utils::validation;
use crate::restic::wrapper::{ResticWrapper, Snapshot};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::time::SystemTime;

const REPO_MISSING_CONTEXT: &str = "Failed to load game configuration";

pub fn prune_snapshots(game_name: Option<String>, zero_changes: bool) -> Result<()> {
    if zero_changes {
        return prune_zero_change_snapshots(game_name);
    }

    prune_with_retention(game_name)
}

fn prune_with_retention(game_name: Option<String>) -> Result<()> {
    let game_config = InstantGameConfig::load().context(REPO_MISSING_CONTEXT)?;
    validation::check_restic_and_game_manager(&game_config)?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    let restic = ResticWrapper::new(repo_path.clone(), game_config.repo_password.clone());

    let games = resolve_games(&game_config, game_name);
    if games.is_empty() {
        println!("No games matched the prune request.");
        return Ok(());
    }

    let retention_policy = game_config.retention_policy.effective();
    let retention_rules = retention_policy.to_rules();
    let retention_rules_slice = retention_rules.as_slice();

    println!(
        "Retention policy (per game): keep-last {keep_last}, keep-daily {keep_daily}, keep-weekly {keep_weekly}, keep-monthly {keep_monthly}, keep-yearly {keep_yearly}.",
        keep_last = retention_policy.keep_last,
        keep_daily = retention_policy.keep_daily,
        keep_weekly = retention_policy.keep_weekly,
        keep_monthly = retention_policy.keep_monthly,
        keep_yearly = retention_policy.keep_yearly,
    );

    let mut processed_games = Vec::new();

    for game in games.iter() {
        let tags_for_game = tags::create_game_tags(game);
        let snapshots_json = restic
            .list_snapshots_filtered(Some(tags_for_game.clone()))
            .with_context(|| format!("Failed to list restic snapshots for '{game}'"))?;
        let snapshots: Vec<Snapshot> = serde_json::from_str(&snapshots_json)
            .with_context(|| format!("Failed to parse snapshot data for '{game}'"))?;

        if snapshots.is_empty() {
            println!("No snapshots found for '{game}', skipping retention pruning.");
            continue;
        }

        println!(
            "Applying retention policy to '{game}' ({} snapshots)...",
            snapshots.len()
        );

        restic
            .forget_with_policy(
                Some(tags_for_game),
                Some(vec!["host".to_string(), "tags".to_string()]),
                retention_rules_slice,
                true,
            )
            .with_context(|| format!("Failed to apply retention policy for '{game}'"))?;

        cache::invalidate_game_cache(game, &repo_path);
        processed_games.push(game.clone());
    }

    if processed_games.is_empty() {
        println!("No games required retention pruning.");
    } else {
        println!(
            "Retention pruning completed for {} game(s): {}.",
            processed_games.len(),
            processed_games.join(", ")
        );
    }

    Ok(())
}

fn prune_zero_change_snapshots(game_name: Option<String>) -> Result<()> {
    let game_config = InstantGameConfig::load().context(REPO_MISSING_CONTEXT)?;
    validation::check_restic_and_game_manager(&game_config)?;

    let repo_path = game_config.repo.as_path().to_string_lossy().to_string();
    let restic = ResticWrapper::new(repo_path.clone(), game_config.repo_password.clone());

    let games = resolve_games(&game_config, game_name);
    if games.is_empty() {
        println!("No games matched the prune request.");
        return Ok(());
    }

    let mut total_pruned = 0usize;
    let mut processed_games = HashSet::new();

    for game in games.iter() {
        let tags_for_game = tags::create_game_tags(game);
        let snapshots_json = restic
            .list_snapshots_filtered(Some(tags_for_game.clone()))
            .with_context(|| format!("Failed to list restic snapshots for '{game}'"))?;
        let mut snapshots: Vec<Snapshot> = serde_json::from_str(&snapshots_json)
            .with_context(|| format!("Failed to parse snapshot data for '{game}'"))?;

        if snapshots.is_empty() {
            println!("No snapshots found for '{game}', skipping zero-change prune.");
            continue;
        }

        snapshots.sort_by(|a, b| parse_snapshot_time(&b.time).cmp(&parse_snapshot_time(&a.time)));
        let zero_change: Vec<&Snapshot> = snapshots
            .iter()
            .filter(|snap| is_zero_change(snap))
            .collect();

        if zero_change.len() <= 1 {
            println!("No redundant zero-change snapshots for '{game}'.");
            continue;
        }

        let mut prune_targets = Vec::new();
        let mut removed_details = Vec::new();
        for snapshot in zero_change.into_iter().skip(1) {
            prune_targets.push(snapshot.id.clone());
            removed_details.push((snapshot.short_id.clone(), snapshot.time.clone()));
        }

        println!(
            "Pruning {} redundant zero-change snapshot(s) for '{}':",
            prune_targets.len(),
            game
        );
        for (short_id, time) in &removed_details {
            println!("  • {} ({})", short_id, time);
        }

        restic
            .forget_snapshots(&prune_targets, true)
            .with_context(|| format!("Failed to prune redundant snapshots for '{game}'"))?;

        cache::invalidate_game_cache(game, &repo_path);
        total_pruned += prune_targets.len();
        processed_games.insert(game.clone());
    }

    if total_pruned == 0 {
        println!("No redundant zero-change snapshots found.");
    } else {
        println!(
            "Removed {} redundant zero-change snapshot(s) across {} game(s).",
            total_pruned,
            processed_games.len()
        );
    }

    Ok(())
}

fn resolve_games(game_config: &InstantGameConfig, game_name: Option<String>) -> Vec<String> {
    match game_name {
        Some(name) => {
            let exists = game_config
                .games
                .iter()
                .any(|g| g.name.0.as_str() == name.as_str());
            if exists {
                vec![name]
            } else {
                println!("Game '{}' is not configured.", name);
                Vec::new()
            }
        }
        None => game_config.games.iter().map(|g| g.name.0.clone()).collect(),
    }
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
