//! Builder pattern for FZF dialogs

mod checklist;
mod dialogs;
mod padded;

use anyhow::{self, Result};
use serde::de::DeserializeOwned;

use crate::ui::catppuccin::format_icon_colored;
use crate::ui::nerd_font::NerdFont;

use super::types::*;
use super::wrapper::FzfWrapper;
use super::wrapper::FzfWrapperParts;

#[derive(Debug, Clone)]
pub struct FzfBuilder {
    multi_select: bool,
    prompt: Option<String>,
    header: Option<Header>,
    additional_args: Vec<String>,
    dialog_type: DialogType,
    initial_cursor: Option<InitialCursor>,
    initial_query: Option<String>,
    ghost_text: Option<String>,
    responsive_layout: bool,
    checklist_actions: Vec<ChecklistAction>,
}

#[derive(Debug, Clone)]
enum DialogType {
    Selection,
    Input,
    Password {
        confirm: bool,
    },
    Confirmation {
        yes_text: String,
        no_text: String,
    },
    Message {
        ok_text: String,
        title: Option<String>,
    },
    Checklist {
        confirm_text: String,
        allow_empty: bool,
    },
}

#[derive(Clone)]
struct ConfirmOption {
    label: String,
    color: &'static str,
    icon: NerdFont,
    result: ConfirmResult,
}

#[derive(Clone)]
struct ChecklistEntry {
    display: String,
    key: String,
    preview: FzfPreview,
}

impl ChecklistEntry {
    fn new(display: String, key: String, preview: FzfPreview) -> Self {
        Self {
            display,
            key,
            preview,
        }
    }
}

impl FzfSelectable for ChecklistEntry {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }
}

impl ConfirmOption {
    fn new(label: String, color: &'static str, icon: NerdFont, result: ConfirmResult) -> Self {
        Self {
            label,
            color,
            icon,
            result,
        }
    }
}

impl FzfSelectable for ConfirmOption {
    fn fzf_display_text(&self) -> String {
        let badge = format_icon_colored(self.icon, self.color);
        format!("{badge}{}", self.label)
    }

    fn fzf_key(&self) -> String {
        self.label.clone()
    }
}

impl FzfBuilder {
    pub fn new() -> Self {
        Self {
            multi_select: false,
            prompt: None,
            header: None,
            additional_args: Self::default_args(),
            dialog_type: DialogType::Selection,
            initial_cursor: None,
            initial_query: None,
            ghost_text: None,
            responsive_layout: false,
            checklist_actions: Vec::new(),
        }
    }

    pub(crate) fn into_wrapper_parts(self) -> FzfWrapperParts {
        FzfWrapperParts {
            multi_select: self.multi_select,
            prompt: self.prompt,
            header: self.header,
            additional_args: self.additional_args,
            initial_cursor: self.initial_cursor,
            responsive_layout: self.responsive_layout,
        }
    }

    pub fn multi_select(mut self, multi: bool) -> Self {
        self.multi_select = multi;
        self
    }

    pub fn prompt<S: Into<String>>(mut self, prompt: S) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn header<H: Into<Header>>(mut self, header: H) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn initial_index(mut self, index: usize) -> Self {
        self.initial_cursor = Some(InitialCursor::Index(index));
        self
    }

    pub fn query<S: Into<String>>(mut self, query: S) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.additional_args
            .extend(args.into_iter().map(Into::into));
        self
    }

    pub fn responsive_layout(mut self) -> Self {
        self.responsive_layout = true;
        self
    }

    pub fn input(mut self) -> Self {
        self.dialog_type = DialogType::Input;
        self.additional_args = Self::input_args();
        self
    }

    pub fn ghost<S: Into<String>>(mut self, text: S) -> Self {
        self.ghost_text = Some(text.into());
        self
    }

    pub fn password(mut self) -> Self {
        self.dialog_type = DialogType::Password { confirm: false };
        self.additional_args = Self::password_args();
        self
    }

    pub fn with_confirmation(mut self) -> Self {
        if let DialogType::Password { ref mut confirm } = self.dialog_type {
            *confirm = true;
        }
        self
    }

    pub fn confirm<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Confirmation {
            yes_text: "Yes".to_string(),
            no_text: "No".to_string(),
        };
        self.header = Some(Header::Default(message.into()));
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn yes_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { yes_text, .. } = &mut self.dialog_type {
            *yes_text = text.into();
        }
        self
    }

    pub fn no_text<S: Into<String>>(mut self, text: S) -> Self {
        if let DialogType::Confirmation { no_text, .. } = &mut self.dialog_type {
            *no_text = text.into();
        }
        self
    }

    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.dialog_type = DialogType::Message {
            ok_text: "OK".to_string(),
            title: None,
        };
        self.header = Some(Header::Default(message.into()));
        self.additional_args = Self::confirm_args();
        self
    }

    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        if let DialogType::Message { title: target, .. } = &mut self.dialog_type {
            *target = Some(title.into());
        }
        self
    }

    pub fn checklist<S: Into<String>>(mut self, confirm_text: S) -> Self {
        self.dialog_type = DialogType::Checklist {
            confirm_text: confirm_text.into(),
            allow_empty: true,
        };
        self.additional_args = Self::checklist_args();
        self
    }

    pub fn checklist_actions<I>(mut self, actions: I) -> Self
    where
        I: IntoIterator<Item = ChecklistAction>,
    {
        self.checklist_actions = actions.into_iter().collect();
        self
    }

    pub fn allow_empty_confirm(mut self, allow: bool) -> Self {
        if let DialogType::Checklist { allow_empty, .. } = &mut self.dialog_type {
            *allow_empty = allow;
        }
        self
    }

    pub fn checklist_dialog<T: FzfSelectable + Clone>(
        self,
        items: Vec<T>,
    ) -> Result<ChecklistResult<T>> {
        if !matches!(self.dialog_type, DialogType::Checklist { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for checklist"));
        }
        self.execute_checklist(items)
    }

    pub fn select<T: FzfSelectable + Clone>(self, items: Vec<T>) -> Result<FzfResult<T>> {
        FzfWrapper::from_builder(self).select(items)
    }

    pub fn select_menu<T: FzfSelectable + Clone>(
        self,
        items: Vec<super::types::MenuItem<T>>,
    ) -> Result<FzfResult<T>> {
        use super::types::MenuItem;

        let mut wrapper = FzfWrapper::from_builder(self);

        loop {
            match wrapper.select(items.clone())? {
                FzfResult::Selected(MenuItem::Entry(item)) => {
                    return Ok(FzfResult::Selected(item));
                }
                FzfResult::Selected(MenuItem::Separator(_)) => {
                    wrapper.initial_cursor = None;
                    continue;
                }
                FzfResult::MultiSelected(selected) => {
                    let entries: Vec<T> = selected
                        .into_iter()
                        .filter_map(|mi| match mi {
                            MenuItem::Entry(item) => Some(item),
                            MenuItem::Separator(_) => None,
                        })
                        .collect();
                    return Ok(FzfResult::MultiSelected(entries));
                }
                FzfResult::Cancelled => return Ok(FzfResult::Cancelled),
                FzfResult::Error(e) => return Ok(FzfResult::Error(e)),
            }
        }
    }

    pub fn select_encoded_streaming<T, C>(
        self,
        command: C,
    ) -> Result<FzfResult<DecodedStreamingMenuItem<T>>>
    where
        T: DeserializeOwned,
        C: Into<StreamingCommand>,
    {
        FzfWrapper::from_builder(self).select_encoded_streaming(command)
    }

    pub fn select_encoded_streaming_prefilled<T, C>(
        self,
        command: C,
        initial_input: &str,
    ) -> Result<FzfResult<DecodedStreamingMenuItem<T>>>
    where
        T: DeserializeOwned,
        C: Into<StreamingCommand>,
    {
        FzfWrapper::from_builder(self).select_encoded_streaming_prefilled(command, initial_input)
    }

    pub fn input_dialog(self) -> Result<String> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input()
    }

    pub fn password_dialog(self) -> Result<FzfResult<String>> {
        let confirm = if let DialogType::Password { confirm } = self.dialog_type {
            confirm
        } else {
            return Err(anyhow::anyhow!("Builder not configured for password"));
        };
        self.execute_password(confirm)
    }

    pub fn confirm_dialog(self) -> Result<ConfirmResult> {
        if !matches!(self.dialog_type, DialogType::Confirmation { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for confirmation"));
        }
        self.execute_confirm()
    }

    pub fn message_dialog(self) -> Result<()> {
        if !matches!(self.dialog_type, DialogType::Message { .. }) {
            return Err(anyhow::anyhow!("Builder not configured for message"));
        }
        self.execute_message()
    }

    pub fn input_result(self) -> Result<FzfResult<String>> {
        if !matches!(self.dialog_type, DialogType::Input) {
            return Err(anyhow::anyhow!("Builder not configured for input"));
        }
        self.execute_input_result()
    }

    fn base_args(margin: &str) -> Vec<String> {
        let mut args = vec![
            "--margin".to_string(),
            margin.to_string(),
            "--min-height".to_string(),
            "10".to_string(),
        ];
        args.extend(
            super::theme::theme_args()
                .into_iter()
                .map(|s| s.to_string()),
        );
        args
    }

    fn default_args() -> Vec<String> {
        Self::base_args("10%,2%")
    }

    fn input_args() -> Vec<String> {
        Self::base_args("20%,2%")
    }

    fn confirm_args() -> Vec<String> {
        let mut args = Self::base_args("20%,2%");
        args.push("--info=hidden".to_string());
        args.push("--color=header:-1".to_string());
        args.push("--no-input".to_string());
        args
    }

    fn password_args() -> Vec<String> {
        vec![]
    }

    fn checklist_args() -> Vec<String> {
        let mut args = Self::base_args("10%,2%");
        args.push("--height=95%".to_string());
        args
    }
}
