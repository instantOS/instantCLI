use std::path::Path;

use anyhow::{Result, bail};

use crate::ui::prelude::Level;
use crate::video::planning::{TimelinePlan, align_plan_with_subtitles, plan_timeline};
use crate::video::render::logging::log_event;

pub(crate) fn build_timeline_plan(
    document: &crate::video::document::VideoDocument,
    cues: &[crate::video::support::transcript::TranscriptCue],
    markdown_path: &Path,
) -> Result<TimelinePlan> {
    log_event(
        Level::Info,
        "video.render.plan",
        "Planning timeline (selecting clips, overlays, cards)",
    );
    let mut plan = plan_timeline(document)?;

    if plan.items.is_empty() {
        bail!(
            "No renderable blocks found in {}. Ensure the markdown contains timestamp code spans or headings.",
            markdown_path.display()
        );
    }

    log_event(
        Level::Info,
        "video.render.plan.align",
        "Aligning planned segments with transcript timing",
    );
    align_plan_with_subtitles(&mut plan, cues)?;

    Ok(plan)
}
