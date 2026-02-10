mod convert;
mod file_selection;
mod operations;
mod project;
mod prompts;
mod types;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfSelectable, FzfWrapper, Header, MenuCursor, MenuItem};
use crate::ui::catppuccin::fzf_mocha_args;

use convert::run_new_project;
use operations::{run_preprocess, run_setup, run_slide, run_transcribe};
use project::run_project_menu;
use types::VideoMenuEntry;

pub async fn video_menu(_debug: bool) -> Result<()> {
    let mut cursor = MenuCursor::new();
    loop {
        let entry = match select_video_menu_entry(&mut cursor)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            VideoMenuEntry::NewProject => run_new_project().await?,
            VideoMenuEntry::Transcribe => run_transcribe().await?,
            VideoMenuEntry::Project => run_project_menu().await?,
            VideoMenuEntry::Slide => run_slide().await?,
            VideoMenuEntry::Preprocess => run_preprocess().await?,
            VideoMenuEntry::Setup => run_setup().await?,
            VideoMenuEntry::CloseMenu => return Ok(()),
        }
    }
}

fn select_video_menu_entry(cursor: &mut MenuCursor) -> Result<Option<VideoMenuEntry>> {
    let entries = vec![
        MenuItem::entry(VideoMenuEntry::NewProject),
        MenuItem::entry(VideoMenuEntry::Project),
        MenuItem::entry(VideoMenuEntry::Slide),
        MenuItem::separator("Advanced"),
        MenuItem::entry(VideoMenuEntry::Transcribe),
        MenuItem::entry(VideoMenuEntry::Preprocess),
        MenuItem::entry(VideoMenuEntry::Setup),
        MenuItem::line(),
        MenuItem::entry(VideoMenuEntry::CloseMenu),
    ];

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Video Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    let result = builder.select_menu(entries.clone())?;

    match result {
        FzfResult::Selected(entry) => {
            cursor.update_from_key(&entry.fzf_key());
            Ok(Some(entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}
