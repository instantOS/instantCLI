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

    fn name(&self) -> &'static str {
        match self {
            AiProvider::Claude => "Claude",
            AiProvider::ChatGpt => "ChatGPT",
            AiProvider::Gemini => "Gemini",
        }
    }
}

fn ask_ai(provider: AiProvider) -> Result<()> {
    let display_server = DisplayServer::detect();

    let clipboard_content = match utils::get_clipboard_content(&display_server) {
        Ok(content) => content,
        Err(e) => {
            let error_msg = if e.to_string().contains("Clipboard is empty") {
                "Clipboard is empty"
            } else if e.to_string().contains("not valid UTF-8") {
                "Clipboard contains non-text data"
            } else {
                "Failed to get clipboard content"
            };

            utils::show_notification(&format!("Ask {}", provider.name()), error_msg)?;
            return Ok(());
        }
    };

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
