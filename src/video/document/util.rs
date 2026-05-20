use pulldown_cmark::HeadingLevel;

pub fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn is_music_code_block(info: &str) -> bool {
    info.split(|c: char| c.is_whitespace())
        .next()
        .map(|lang| lang.eq_ignore_ascii_case("music"))
        .unwrap_or(false)
}

pub fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count()
}

pub fn strip_html_comments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    let mut cursor = 0usize;
    while let Some(start_rel) = input[cursor..].find("<!--") {
        let start = cursor + start_rel;
        output.push_str(&input[cursor..start]);

        let after_start = start + "<!--".len();
        if let Some(end_rel) = input[after_start..].find("-->") {
            let end = after_start + end_rel + "-->".len();
            cursor = end;
        } else {
            return output;
        }
    }

    output.push_str(&input[cursor..]);
    output
}

pub struct LineMap {
    offsets: Vec<usize>,
}

impl LineMap {
    pub fn new(text: &str) -> Self {
        let mut offsets = Vec::new();
        offsets.push(0);
        for (idx, _) in text.match_indices('\n') {
            offsets.push(idx + 1);
        }
        Self { offsets }
    }

    pub fn line_number(&self, byte_index: usize) -> usize {
        match self.offsets.binary_search(&byte_index) {
            Ok(pos) => pos + 1,
            Err(pos) => pos,
        }
    }
}
