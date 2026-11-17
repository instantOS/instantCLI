mod actions;
mod mime_info;
mod mime_sets;

pub use actions::{
    manage_default_apps, set_default_archive_manager, set_default_audio_player,
    set_default_browser, set_default_email, set_default_file_manager, set_default_image_viewer,
    set_default_pdf_viewer, set_default_text_editor, set_default_video_player,
};

pub(crate) use mime_sets::{
    ARCHIVE_MIME_TYPES, AUDIO_MIME_TYPES, IMAGE_MIME_TYPES, VIDEO_MIME_TYPES,
};
