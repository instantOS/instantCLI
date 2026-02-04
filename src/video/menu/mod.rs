mod convert;
mod file_selection;
mod operations;
mod project;
mod prompts;
mod types;

use anyhow::Result;

use crate::menu_utils::{FzfResult, FzfWrapper, Header, MenuCursor};
use crate::ui::catppuccin::fzf_mocha_args;

use convert::run_convert_multi;
use operations::{run_append, run_preprocess, run_setup, run_slide, run_transcribe};
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
            VideoMenuEntry::Convert => run_convert_multi().await?,
            VideoMenuEntry::Transcribe => run_transcribe().await?,
            VideoMenuEntry::Project => run_project_menu().await?,
            VideoMenuEntry::Append => run_append().await?,
            VideoMenuEntry::Slide => run_slide().await?,
            VideoMenuEntry::Preprocess => run_preprocess().await?,
            VideoMenuEntry::Setup => run_setup().await?,
            VideoMenuEntry::CloseMenu => return Ok(()),
        }
    }
}

fn select_video_menu_entry(cursor: &mut MenuCursor) -> Result<Option<VideoMenuEntry>> {
    let entries = vec![
        VideoMenuEntry::Convert,
        VideoMenuEntry::Transcribe,
        VideoMenuEntry::Project,
        VideoMenuEntry::Append,
        VideoMenuEntry::Slide,
        VideoMenuEntry::Preprocess,
        VideoMenuEntry::Setup,
        VideoMenuEntry::CloseMenu,
    ];

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Video Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    let result = builder.select(entries.clone())?;

    match result {
        FzfResult::Selected(entry) => {
            cursor.update(&entry, &entries);
            Ok(Some(entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}
