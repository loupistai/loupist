//! The writing script of the message a user-written turn answers.
//!
//! The model writes its answer and the progress notes between tool calls in
//! the language of the user's current message. The Loupist's name, focus and
//! folder, saved material and earlier replies can be in another language: in
//! the v0.1.6 acceptance an English question in a Loupist with a Korean focus
//! was answered in English with its progress notes in Korean. The turn
//! context therefore names the script most of the message is written in, as
//! the host reads it, beside the message itself.
//!
//! The script, not a language: letters tell Hangul from Latin for certain,
//! but not English from French. A message without a clear majority script
//! (too short, evenly mixed, only tickers and figures) names none.

use serde_json::Value;

/// The turn-context block naming the current message's script, as an
/// ISO 15924 code. Per turn: left out of the envelope digest and carried
/// whole by an abbreviated envelope, like `previous_run`. The block has no
/// hint of its own: SYSTEM.md's Communication section says how to read it,
/// once in the cached prefix rather than on every turn.
pub(crate) const CURRENT_MESSAGE_SCRIPT: &str = "current_message_script";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Script {
    Latin,
    Hangul,
    Han,
    Kana,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
    Other,
}

fn script_of(c: char) -> Option<Script> {
    if !c.is_alphabetic() {
        return None;
    }
    Some(match c as u32 {
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x024F | 0x1E00..=0x1EFF => Script::Latin,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Script::Hangul,
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Kana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FFFF => Script::Han,
        0x0400..=0x052F => Script::Cyrillic,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0590..=0x05FF => Script::Hebrew,
        0x0E00..=0x0E7F => Script::Thai,
        0x0900..=0x097F => Script::Devanagari,
        _ => Script::Other,
    })
}

/// A run of Latin capitals and digits reads the same in every language: a
/// ticker, an acronym or a unit ("NVDA", "HBM", "Q3").
fn neutral(run: &[char]) -> bool {
    run.len() >= 2
        && run.iter().any(char::is_ascii_uppercase)
        && run
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The ISO 15924 code of the script holding most of the letters of `text`
/// written in words, or `None` without a clear majority. Links, tickers and
/// acronyms are not counted. Kana with Han is Japanese (`Jpan`).
pub(crate) fn writing_script(text: &str) -> Option<&'static str> {
    let mut counts = std::collections::BTreeMap::<Script, usize>::new();
    for word in text.split_whitespace() {
        if word.contains("://") || word.starts_with("www.") {
            continue;
        }
        // Runs of one script, digits joining the run they touch.
        let mut run: Vec<char> = Vec::new();
        let mut run_script = None;
        let mut flush = |run: &mut Vec<char>, script: Option<Script>| {
            if let Some(script) = script.filter(|_| !neutral(run)) {
                *counts.entry(script).or_default() +=
                    run.iter().filter(|c| c.is_alphabetic()).count();
            }
            run.clear();
        };
        for c in word.chars() {
            match script_of(c) {
                Some(script) if run_script.is_some_and(|current| current != script) => {
                    flush(&mut run, run_script);
                    run.push(c);
                    run_script = Some(script);
                }
                Some(script) => {
                    run.push(c);
                    run_script = Some(script);
                }
                None if c.is_ascii_digit() => run.push(c),
                None => {
                    flush(&mut run, run_script);
                    run_script = None;
                }
            }
        }
        flush(&mut run, run_script);
    }
    let kana = counts.remove(&Script::Kana).unwrap_or(0);
    let han = counts.remove(&Script::Han).unwrap_or(0);
    let mut scored: Vec<(&'static str, usize)> = counts
        .into_iter()
        .filter_map(|(script, count)| {
            let code = match script {
                Script::Latin => "Latn",
                Script::Hangul => "Hang",
                Script::Cyrillic => "Cyrl",
                Script::Greek => "Grek",
                Script::Arabic => "Arab",
                Script::Hebrew => "Hebr",
                Script::Thai => "Thai",
                Script::Devanagari => "Deva",
                Script::Han | Script::Kana | Script::Other => return None,
            };
            Some((code, count))
        })
        .collect();
    match (kana, han) {
        (0, 0) => {}
        (0, han) => scored.push(("Hani", han)),
        (kana, han) => scored.push(("Jpan", kana + han)),
    }
    let total: usize = scored.iter().map(|(_, count)| count).sum();
    let (code, count) = scored.into_iter().max_by_key(|(_, count)| *count)?;
    // The script holds at least three in five of the counted letters.
    (total >= 2 && count * 5 >= total * 3).then_some(code)
}

/// Names the script of the message a user-written turn answers, when it has
/// a clear majority.
pub(crate) fn attach_current_message_script(envelope: &str, message: &str) -> String {
    let Some(script) = writing_script(message) else {
        return envelope.to_owned();
    };
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    let Some(object) = value.as_object_mut() else {
        return envelope.to_owned();
    };
    object.insert(CURRENT_MESSAGE_SCRIPT.into(), Value::from(script));
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_most_of_the_words_are_written_in_is_named() {
        for (message, script) in [
            (
                "When does Adriana Kugler's term on the Fed Board end?",
                Some("Latn"),
            ),
            ("연준 이사 임기는 언제 끝나?", Some("Hang")),
            // Tickers, acronyms and links read the same in any language.
            ("NVDA AAPL MSFT 비교해줘", Some("Hang")),
            ("SK하이닉스 HBM 매출 비중 알려줘", Some("Hang")),
            (
                "애플(AAPL) 주가를 https://finance.example/aapl 에서 알려줘",
                Some("Hang"),
            ),
            (
                "Summarize Samsung Electronics' Q3 results for 삼성전자",
                Some("Latn"),
            ),
            ("トヨタの決算を要約して", Some("Jpan")),
            ("丰田汽车的营业利润率是多少", Some("Hani")),
            ("Какая выручка у Сбербанка?", Some("Cyrl")),
            // Nothing to read, or no clear majority.
            ("NVDA?", None),
            ("1,234.5", None),
            ("Apple 애플 사과", None),
        ] {
            assert_eq!(writing_script(message), script, "{message}");
        }
    }

    #[test]
    fn the_envelope_names_the_script_only_when_there_is_one() {
        let envelope = r#"{"live_time":{}}"#;
        let named: Value =
            serde_json::from_str(&attach_current_message_script(envelope, "What is EBITDA?"))
                .unwrap();
        assert_eq!(named[CURRENT_MESSAGE_SCRIPT], "Latn");
        assert_eq!(attach_current_message_script(envelope, "AAPL"), envelope);
    }

    /// A warm turn whose context is unchanged still names the new
    /// question's script: the block is outside the digest and carried whole.
    #[test]
    fn an_unchanged_context_still_names_the_new_questions_script() {
        use crate::agent_harness::{envelope_context_digest, maybe_abbreviate_envelope};
        let base = r#"{"live_time":{},"Loupist tone":"balanced"}"#;
        let english = attach_current_message_script(base, "How did the shares close?");
        let korean = attach_current_message_script(base, "주가는 어떻게 마감했어?");
        let digest = envelope_context_digest(&english);
        assert_eq!(digest, envelope_context_digest(&korean));
        let (short, _) = maybe_abbreviate_envelope(&korean, Some(&digest), true);
        let short: Value = serde_json::from_str(&short).unwrap();
        assert_eq!(short["context"]["unchanged"], true);
        assert_eq!(short[CURRENT_MESSAGE_SCRIPT], "Hang");
    }
}
