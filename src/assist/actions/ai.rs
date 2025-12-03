use anyhow::Result;
use urlencoding::encode;

use crate::assist::utils;
use crate::common::display_server::DisplayServer;

enum AiProvider {
    Claude,
    ChatGpt,
    Gemini,
}

impl AiProvider {
    fn url(&self) -> &'static str {
        match self {
            AiProvider::Claude => "https://claude.ai/new?q",
            AiProvider::ChatGpt => "https://chat.openai.com/?prompt",
            AiProvider::Gemini => "https://gemini.google.com/?q",
        }
    }
}

fn ask_ai(provider: AiProvider) -> Result<()> {
    let display_server = DisplayServer::detect();
    let clipboard_content = utils::get_clipboard_content(&display_server)?;

    let encoded_content = encode(&clipboard_content);
    let url = format!("{}={}", provider.url(), encoded_content);

    utils::launch_detached("xdg-open", &[&url])?;

    Ok(())
}

pub fn ask_claude() -> Result<()> {
    ask_ai(AiProvider::Claude)
}

pub fn ask_chatgpt() -> Result<()> {
    ask_ai(AiProvider::ChatGpt)
}

pub fn ask_gemini() -> Result<()> {
    ask_ai(AiProvider::Gemini)
}
