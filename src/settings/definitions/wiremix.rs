//! Launch wiremix audio settings

use crate::common::requirements::WIREMIX_PACKAGE;
use crate::ui::prelude::*;

tui_command_setting!(
    LaunchWiremix,
    "audio.wiremix",
    "General audio settings",
    NerdFont::Settings,
    "Launch wiremix TUI to manage PipeWire routing and volumes.",
    "wiremix",
    WIREMIX_PACKAGE
);
