use anyhow::Result;

use crate::ui::catppuccin::fzf_mocha_args;
use crate::ui::nerd_font::NerdFont;

use super::{FzfResult, FzfSelectable, FzfWrapper};

pub fn select_one_with_style_at<T>(items: Vec<T>, initial_index: Option<usize>) -> Result<Option<T>>
where
    T: FzfSelectable + Clone,
{
    let mut builder = FzfWrapper::builder()
        .prompt(format!("{} ", char::from(NerdFont::Search)))
        .header("")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = initial_index {
        builder = builder.initial_index(index);
    }

    match builder.select_padded(items)? {
        FzfResult::Selected(item) => Ok(Some(item)),
        _ => Ok(None),
    }
}

pub fn select_one_with_style<T>(items: Vec<T>) -> Result<Option<T>>
where
    T: FzfSelectable + Clone,
{
    select_one_with_style_at(items, None)
}
