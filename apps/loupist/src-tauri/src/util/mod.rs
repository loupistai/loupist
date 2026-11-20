//! Small shared helpers that hold no app state.

pub(crate) mod credential_text;
pub(crate) mod json_pointer;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 digest of raw bytes, shared by every content-digest surface.
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Fit a display preview in its character budget, including the ellipsis.
/// Back up to a whole word when possible and keep grapheme clusters intact.
pub(crate) fn shorten(text: &str, limit: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if limit == 0 {
        return String::new();
    }
    let Some((last, character)) = text.char_indices().nth(limit - 1) else {
        return text.to_owned();
    };
    if last + character.len_utf8() == text.len() {
        return text.to_owned();
    }
    let mut end = 0;
    let mut word_end = None;
    for (at, grapheme) in text
        .grapheme_indices(true)
        .take_while(|(at, _)| *at <= last)
    {
        end = at;
        if grapheme.starts_with(char::is_whitespace) {
            word_end = Some(at);
        }
    }
    let prefix = &text[..word_end.unwrap_or(end)];
    format!("{}…", prefix.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortened_text_preserves_whole_words_or_graphemes_within_the_limit() {
        assert_eq!(shorten("net income statement", 12), "net income…");
        assert_eq!(shorten("annual reporting", 9), "annual…");
        assert_eq!(shorten("분기재무제표", 4), "분기재…");
        assert_eq!(shorten("한글", 4), "한…");
        assert_eq!(shorten("x👩🏽‍💻abcdef", 5), "x…");
        assert_eq!(shorten("x👩🏽‍💻abcdef", 6), "x👩🏽‍💻…");
        assert_eq!(shorten("short", 5), "short");
        assert_eq!(shorten("", 5), "");
    }
}
