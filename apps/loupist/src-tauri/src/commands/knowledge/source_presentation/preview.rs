//! A catalog preview is display text. The reader and its citation offsets
//! continue to use the exact saved passage.
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

pub(in crate::commands::knowledge) fn prose(
    text: &str,
    starts_inside_source: bool,
    truncated_end: bool,
) -> Option<String> {
    let text = if truncated_end {
        text.trim_end_matches(char::is_alphanumeric)
    } else {
        text
    };
    let mut paragraph = String::new();
    let mut in_paragraph = false;
    let mut link = false;
    let mut has_prose = false;
    let mut clipped_start = false;
    let mut fallback = None;
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::Paragraph | Tag::Item) => {
                in_paragraph = true;
                has_prose = false;
                clipped_start = starts_inside_source
                    && range.start == 0
                    && text.starts_with(char::is_alphanumeric);
                paragraph.clear();
            }
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => link = true,
            Event::End(TagEnd::Link | TagEnd::Image) => link = false,
            Event::Text(s) | Event::Code(s) if in_paragraph => {
                has_prose |= !link && s.chars().any(char::is_alphabetic);
                paragraph.push_str(&s);
            }
            Event::SoftBreak | Event::HardBreak if in_paragraph => paragraph.push('\n'),
            Event::End(TagEnd::Paragraph | TagEnd::Item) if in_paragraph => {
                in_paragraph = false;
                if !has_prose {
                    continue;
                }
                let mut lines = paragraph.lines().peekable();
                while let Some(line) = lines.next() {
                    if debris(line) {
                        clipped_start = false;
                        continue;
                    }
                    let mut block = line.trim().to_owned();
                    while let Some(next) = lines.next_if(|next| !debris(next)) {
                        block.push(' ');
                        block.push_str(next.trim());
                    }
                    let mut sentences = sentences(&block).peekable();
                    if std::mem::take(&mut clipped_start) {
                        sentences.next();
                    }
                    while let Some(sentence) = sentences.next() {
                        if sentence
                            .unicode_words()
                            .filter(|w| w.chars().any(char::is_alphabetic))
                            .take(2)
                            .count()
                            < 2
                            && !sentence.chars().any(|c| matches!(c, '。' | '！' | '？'))
                        {
                            continue;
                        }
                        let ends = sentence
                            .trim_end_matches(['"', '”', '\'', '’', ')'])
                            .ends_with(['.', '!', '?', '。', '！', '？']);
                        if ends {
                            let more = truncated_end
                                || sentences.peek().is_some()
                                || lines.peek().is_some()
                                || !text[range.end..].trim().is_empty();
                            return Some(fit(sentence, more));
                        }
                        fallback.get_or_insert_with(|| {
                            fit(sentence, truncated_end || range.end < text.trim_end().len())
                        });
                    }
                }
            }
            _ => {}
        }
    }
    fallback
}

// Unicode sentence boundaries can fall between “U.S.” and “Technology”.
// Keep dotted initials and common company suffixes with the sentence.
fn sentences(block: &str) -> impl Iterator<Item = &str> {
    let mut parts = block.split_sentence_bounds();
    let mut end = 0;
    std::iter::from_fn(move || {
        let start = end;
        loop {
            let part = parts.next()?;
            end += part.len();
            let last = part.split_whitespace().last().unwrap_or_default();
            let abbreviation = ["Inc.", "Corp.", "Ltd.", "Co.", "e.g.", "i.e."].contains(&last)
                || (last.ends_with('.')
                    && last.matches('.').count() >= 2
                    && last
                        .trim_end_matches('.')
                        .split('.')
                        .all(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic()));
            if !abbreviation || end == block.len() {
                return Some(block[start..end].trim());
            }
        }
    })
}

fn fit(sentence: &str, more: bool) -> String {
    let text = if more {
        format!("{}…", sentence.trim_end_matches(['.', '…']))
    } else {
        sentence.to_owned()
    };
    crate::util::shorten(&text, 240)
}

fn debris(line: &str) -> bool {
    let line = line.trim();
    let first = line.split_whitespace().next().unwrap_or_default();
    if [
        "contents",
        "table of contents",
        "back to top",
        "skip to content",
    ]
    .iter()
    .any(|s| line.eq_ignore_ascii_case(s))
        || line.is_empty()
        || line.starts_with(['|', '[', ']', '{', '}', '#'])
        || first.ends_with(')') && first.contains('_')
        || line.contains("](#")
        || line.contains("](")
        || line.contains(" | ")
    {
        return true;
    }
    let mut words = 0;
    let mut numeric = 0;
    for word in line.split_whitespace() {
        words += 1;
        if !word.chars().any(char::is_alphabetic) {
            numeric += 1;
        }
    }
    numeric >= 2 && numeric * 2 >= words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_filing_navigation_and_table_cells_give_way_to_the_first_sentence() {
        let excerpt = "HTM_i7bfbfbe54b9647b1b4ba4ff4e0aba09d_175)\n\n[Consolidated Statements of Comprehensive Income](#i7bfb_178)\n\nnes U.S. Technology Supersector Index $ 100 $ 146 $ 216 $ 156 $ 215 $ 322\n\n| Year | Net sales |\n| --- | --- |\n| 2024 | $391,035 |\n\nApple reported higher [net sales](#net-sales) in 2024. Additional statements follow.";
        assert_eq!(
            prose(excerpt, true, false).as_deref(),
            Some("Apple reported higher net sales in 2024…")
        );
        assert_eq!(
            prose("ported the result. Net sales increased.", true, false).as_deref(),
            Some("Net sales increased.")
        );
        assert_eq!(
            prose("Net sales increased during the quart", false, true).as_deref(),
            Some("Net sales increased during the…")
        );
        assert_eq!(
            prose("[Contents](#top)\n\n$ 100 $ 146 $ 216", false, false),
            None
        );
    }
}
