use anyhow::{Context, Result};

use super::{DotRepositoryPreviewPayload, PreviewContext};
use crate::common::git::{BranchSyncStatus, FileStatusCounts, RepoStatus};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

fn working_tree_summary(counts: &FileStatusCounts) -> String {
    let mut changes = Vec::new();

    if counts.modified > 0 {
        changes.push(format!("{} modified", counts.modified));
    }
    if counts.added > 0 {
        changes.push(format!("{} added", counts.added));
    }
    if counts.deleted > 0 {
        changes.push(format!("{} deleted", counts.deleted));
    }
    if counts.untracked > 0 {
        changes.push(format!("{} untracked", counts.untracked));
    }

    if changes.is_empty() {
        "Uncommitted changes".to_string()
    } else {
        format!("Uncommitted changes ({})", changes.join(", "))
    }
}

fn append_git_status(mut builder: PreviewBuilder, status: &RepoStatus) -> PreviewBuilder {
    builder = builder
        .blank()
        .line(colors::MAUVE, Some(NerdFont::GitBranch), "Git Status")
        .indented_line(
            colors::TEXT,
            Some(NerdFont::GitBranch),
            &format!("Branch: {}", status.branch),
        );

    builder = if status.working_dir_clean {
        builder.indented_line(
            colors::GREEN,
            Some(NerdFont::CheckCircle),
            "Working tree: Clean",
        )
    } else {
        builder.indented_line(
            colors::YELLOW,
            Some(NerdFont::Edit),
            &format!(
                "Working tree: {}",
                working_tree_summary(&status.file_counts)
            ),
        )
    };

    match &status.branch_sync {
        BranchSyncStatus::UpToDate => builder.indented_line(
            colors::GREEN,
            Some(NerdFont::CloudCheck),
            "Remote: Up to date",
        ),
        BranchSyncStatus::Ahead { commits } => builder.indented_line(
            colors::BLUE,
            Some(NerdFont::CloudUpload),
            &format!("Remote: {commits} commit(s) ahead"),
        ),
        BranchSyncStatus::Behind { commits } => builder.indented_line(
            colors::YELLOW,
            Some(NerdFont::CloudDownload),
            &format!("Remote: {commits} commit(s) behind"),
        ),
        BranchSyncStatus::Diverged { ahead, behind } => builder.indented_line(
            colors::RED,
            Some(NerdFont::GitMerge),
            &format!("Remote: Diverged ({ahead} ahead, {behind} behind)"),
        ),
        BranchSyncStatus::NoRemote => builder.indented_line(
            colors::SUBTEXT0,
            Some(NerdFont::Warning),
            "Remote: No upstream",
        ),
    }
}

pub(super) fn render_dot_repository_preview(ctx: &PreviewContext) -> Result<String> {
    let payload = ctx
        .key()
        .ok_or_else(|| anyhow::anyhow!("No repository preview payload provided"))?;
    let payload: DotRepositoryPreviewPayload =
        serde_json::from_str(payload).context("Failed to parse repository preview payload")?;
    let mut builder = PreviewBuilder::new()
        .title(colors::SKY, &payload.repo_name)
        .blank()
        .line(
            colors::TEXT,
            Some(NerdFont::Link),
            &format!("URL: {}", payload.url),
        );

    if let Some(branch) = &payload.configured_branch {
        builder = builder.line(
            colors::TEXT,
            Some(NerdFont::GitBranch),
            &format!("Configured branch: {branch}"),
        );
    }

    builder = builder
        .line(
            if payload.enabled {
                colors::GREEN
            } else {
                colors::RED
            },
            Some(if payload.enabled {
                NerdFont::ToggleOn
            } else {
                NerdFont::ToggleOff
            }),
            if payload.enabled {
                "Enabled"
            } else {
                "Disabled"
            },
        )
        .indented_line(
            colors::TEXT,
            Some(NerdFont::Folder),
            &format!("Local: {}", payload.repo_path.display()),
        );

    if payload.read_only {
        builder = builder.line(colors::YELLOW, Some(NerdFont::Lock), "Read-only");
    }

    builder = match crate::common::git::get_repo_status(&payload.repo_path) {
        Ok(status) => append_git_status(builder, &status),
        Err(err) => builder
            .blank()
            .line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "Git status unavailable",
            )
            .subtext(&err.to_string()),
    };

    Ok(builder.build_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_tree_summary_includes_each_change_kind() {
        let counts = FileStatusCounts {
            modified: 2,
            added: 1,
            deleted: 3,
            untracked: 4,
        };

        assert_eq!(
            working_tree_summary(&counts),
            "Uncommitted changes (2 modified, 1 added, 3 deleted, 4 untracked)"
        );
    }

    #[test]
    fn working_tree_summary_handles_empty_counts() {
        assert_eq!(
            working_tree_summary(&FileStatusCounts::default()),
            "Uncommitted changes"
        );
    }
}
