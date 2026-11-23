//! Units stated around Markdown links. Table units stay within their table;
//! a column or a row can override a table-wide monetary caption.
use super::numbers::{self, Currency, Unit};
type Label = (Unit, Option<Currency>);
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeMap;

fn label(text: &str) -> Option<Label> {
    let lower = text.to_lowercase();
    let mut unit = None;
    let words = lower
        .split(|c: char| {
            !c.is_alphanumeric() && !matches!(c, '%' | '％' | '$' | '€' | '₩' | '¥' | '£')
        })
        .filter(|s| !s.is_empty());
    let mut prev = "";
    for word in words {
        let found = match word {
            "%" | "％" | "percent" | "pct" | "percentage" => Some(Unit::Percent),
            "bp" | "bps" => Some(Unit::BasisPoints),
            "bn" => Some(Unit::Amount(1e9)),
            "m" | "b" if prev == "$" || crate::finance_data::fx::is_currency_code(prev) => {
                Some(Unit::Amount(if word == "m" { 1e6 } else { 1e9 }))
            }
            _ if word.chars().next().is_some_and(char::is_alphabetic) => Unit::of_line(Some(word))
                .ok()
                .filter(|u| matches!(u, Unit::Amount(scale) if *scale > 1.0)),
            _ => None,
        };
        // A quantity elsewhere in the prose ("sold 2 million units")
        // states its own scale and gives none to another number.
        let follows_quantity = prev.ends_with(|c: char| c.is_numeric());
        prev = word;
        if let Some(found) = found.filter(|_| !follows_quantity) {
            if unit.is_some_and(|prior| prior != found) {
                return None;
            }
            unit = Some(found);
        }
    }
    let currency = numbers::stated_currency(text);
    unit.or_else(|| currency.map(|_| Unit::Amount(1.0)))
        .map(|unit| {
            (
                unit,
                if matches!(unit, Unit::Amount(_)) {
                    currency
                } else {
                    None
                },
            )
        })
}

// A unit-only stub ("USD millions | 2023 | 2024") governs the table. A
// measure named in the first column ("Revenue (USD m) | Employees") does not.
fn unit_stub(raw: &str) -> bool {
    raw.split(|c: char| !c.is_alphabetic())
        .filter(|word| !word.is_empty())
        .all(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "in" | "m" | "b" | "unit" | "units"
            ) || matches!(Unit::of_line(Some(word)), Ok(Unit::Amount(_)))
        })
}

fn caption(neighbour: &str) -> Option<Label> {
    // A caption must be a single adjoining paragraph. Up to one blank line
    // is common between a Markdown heading and its table.
    label(neighbour.trim().trim_matches('*'))
}

/// The end of the Markdown link that starts at `at`.
fn link_end(content: &str, at: usize) -> usize {
    content[at..]
        .find("](")
        .and_then(|open| {
            let target = at + open + 2;
            content[target..].find(')').map(|close| target + close + 1)
        })
        .unwrap_or(at)
}

/// What the answer's own words say about the figure linked at `at`.
struct AnswerWords<'a> {
    /// The table row label, or the nearest clause of the sentence that names
    /// a line, if any.
    label: &'a str,
    line: Option<numbers::LineNature>,
    falls: bool,
}

/// A table row's figure takes its row label. A sentence's figure takes the
/// nearest clause before it that names a line, within the sentence; links
/// and clause punctuation separate clauses.
fn answer_words(content: &str, at: usize) -> AnswerWords<'_> {
    let line_start = content[..at].rfind('\n').map_or(0, |start| start + 1);
    let line = &content[line_start..at];
    if let Some(row) = line.trim_start().strip_prefix('|') {
        let label = row.split('|').next().unwrap_or("");
        let nature = numbers::line_nature(label);
        return AnswerWords {
            label,
            line: nature,
            falls: nature == Some(numbers::LineNature::Loss) || numbers::says_outflow(label),
        };
    }
    let before = &line[line.ceil_char_boundary(line.len().saturating_sub(512))..];
    let sentence_start = before
        .char_indices()
        .rev()
        .find(|&(offset, c)| {
            matches!(c, '。' | '！' | '？')
                || (matches!(c, '.' | '!' | '?')
                    && before[offset + 1..].starts_with(char::is_whitespace))
        })
        .map_or(0, |(offset, c)| offset + c.len_utf8());
    let sentence = &before[sentence_start..];
    let mut clauses = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while cursor < sentence.len() {
        let rest = &sentence[cursor..];
        let c = rest.chars().next().unwrap_or(' ');
        if c == '[' {
            let end = link_end(sentence, cursor);
            if end > cursor {
                clauses.push(&sentence[start..cursor]);
                start = end;
                cursor = end;
                continue;
            }
        }
        if matches!(c, ',' | ';' | ':' | '，' | '；' | '：' | '、') {
            clauses.push(&sentence[start..cursor]);
            start = cursor + c.len_utf8();
        }
        cursor += c.len_utf8();
    }
    clauses.push(&sentence[start..]);
    let (label, line) = clauses
        .iter()
        .rev()
        .find_map(|clause| numbers::line_nature(clause).map(|nature| (*clause, Some(nature))))
        .unwrap_or((clauses.last().copied().unwrap_or(""), None));
    let end = link_end(content, at);
    let after = &content[end..content.floor_char_boundary((end + 96).min(content.len()))];
    let after = after.split('\n').next().unwrap_or("");
    AnswerWords {
        label,
        line,
        falls: line == Some(numbers::LineNature::Loss)
            || numbers::says_outflow(label)
            || numbers::states_fall(before, after),
    }
}

/// A bare one-to-three-digit link that trails text in its table cell or
/// clause, with nothing but spacing or punctuation after it, is a reference
/// marker (`숫자 없음 [2]`), even where a unit governs the cell.
fn reference_marker(content: &str, at: usize, text: &str) -> bool {
    let digits = text.trim();
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let line_start = content[..at].rfind('\n').map_or(0, |start| start + 1);
    let before = content[line_start..at].rsplit('|').next().unwrap_or("");
    let trails_text =
        before.trim_end().chars().next_back().is_some_and(|c| {
            !matches!(c, '(' | '[' | '$' | '₩' | '€' | '£' | '¥' | '-' | '−' | '+')
        });
    let next = content[link_end(content, at)..].chars().next();
    trails_text
        && next.is_none_or(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '|' | '.' | ',' | ';' | ':' | ')' | ']' | '!' | '?' | '。' | '、' | '，' | '['
                )
        })
}

pub(super) fn printed_number(
    content: &str,
    at: usize,
    text: &str,
    governing: Option<Label>,
) -> Option<numbers::Printed> {
    if reference_marker(content, at, text) {
        return None;
    }
    let words = answer_words(content, at);
    // A per-share row or clause keeps its own unit under a table's monetary scale.
    let governing = governing.map(|(unit, currency)| match unit {
        Unit::Amount(_) if numbers::per_share_label(words.label) => (Unit::Amount(1.0), currency),
        other => (other, currency),
    });
    let mut printed = numbers::parse_with_unit(text, governing)?;
    if !printed.signed {
        printed.line = words.line;
        printed.falls = words.falls;
    }
    Some(printed)
}

/// The figures of the answer's table row around one link, by column after the
/// row label, and the link's column.
pub(super) struct AnswerRow {
    pub column: usize,
    pub cells: Vec<Option<numbers::Printed>>,
}

pub(super) fn answer_row(
    content: &str,
    at: usize,
    links: &[(usize, super::EvidenceLink)],
    governing: &BTreeMap<usize, Label>,
) -> Option<AnswerRow> {
    let line_start = content[..at].rfind('\n').map_or(0, |start| start + 1);
    let line_end = content[at..]
        .find('\n')
        .map_or(content.len(), |end| at + end);
    let line = &content[line_start..line_end];
    let first = line
        .find('|')
        .filter(|bar| line[..*bar].trim().is_empty())?;
    let mut cells = Vec::new();
    let mut column = None;
    // The first cell is the row label.
    let mut bars = line[first + 1..]
        .match_indices('|')
        .map(|(offset, _)| line_start + first + 1 + offset);
    let mut cell_start = bars.next()? + 1;
    for end in bars.chain(std::iter::once(line_end)) {
        if end <= cell_start {
            continue;
        }
        let figure = links
            .iter()
            .find(|(link_at, _)| (cell_start..end).contains(link_at))
            .and_then(|(link_at, link)| {
                if *link_at == at {
                    column = Some(cells.len());
                }
                printed_number(
                    content,
                    *link_at,
                    &link.text,
                    governing.get(link_at).copied(),
                )
            });
        if !(end == line_end && content[cell_start..end].trim().is_empty()) {
            cells.push(figure);
        }
        cell_start = end + 1;
    }
    let column = column?;
    (cells.iter().flatten().count() >= 2).then_some(AnswerRow { column, cells })
}

pub(super) fn governing_units(content: &str) -> BTreeMap<usize, Label> {
    let mut out = BTreeMap::new();
    let mut headers = Vec::new();
    let mut in_table = false;
    let mut in_head = false;
    let mut col = 0;
    let mut table_unit = None;
    let mut row_unit = None;
    let mut cell_unit = None;
    let mut paragraph = None;
    for (event, range) in Parser::new_ext(content, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                headers.clear();
                let above = content[..range.start]
                    .strip_suffix('\n')
                    .unwrap_or(&content[..range.start]);
                let above = above
                    .strip_suffix('\n')
                    .unwrap_or(above)
                    .rsplit('\n')
                    .next()
                    .unwrap_or("");
                let below = content[range.end..]
                    .strip_prefix('\n')
                    .unwrap_or(&content[range.end..]);
                table_unit = caption(above).or_else(|| caption(below.lines().next().unwrap_or("")));
            }
            Event::End(TagEnd::Table) => in_table = false,
            Event::Start(Tag::TableHead) => {
                in_head = true;
                col = 0;
            }
            Event::End(TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) => {
                col = 0;
                row_unit = None;
            }
            Event::Start(Tag::TableCell) => {
                let raw = &content[range];
                if in_head {
                    let unit = label(raw);
                    if col == 0 && matches!(unit, Some((Unit::Amount(_), _))) && unit_stub(raw) {
                        table_unit = unit;
                    }
                    headers.push(unit);
                }
                if col == 0 && !in_head {
                    row_unit = label(raw);
                }
                cell_unit = headers
                    .get(col)
                    .copied()
                    .flatten()
                    .or(row_unit)
                    .or(table_unit);
                col += 1;
            }
            Event::Start(Tag::Paragraph) => paragraph = Some(range),
            Event::End(TagEnd::Paragraph) => paragraph = None,
            Event::Start(Tag::Link { dest_url, .. }) if super::evidence_id(&dest_url).is_some() => {
                let unit = if in_table {
                    cell_unit
                } else {
                    paragraph.as_ref().and_then(|p| {
                        let before = &content[p.start..range.start];
                        let after = &content[range.end..p.end];
                        let previous = before
                            [before.ceil_char_boundary(before.len().saturating_sub(256))..]
                            .rsplit(['.', '!', '?', ';', '\n'])
                            .next()
                            .unwrap_or("");
                        let next = after[..after.floor_char_boundary(after.len().min(256))]
                            .split(['.', '!', '?', ';', '\n'])
                            .next()
                            .unwrap_or("");
                        let a = label(previous);
                        let b = label(next);
                        if a.is_some() && b.is_some() && a != b {
                            None
                        } else {
                            a.or(b)
                        }
                    })
                };
                if let Some(u) = unit {
                    out.insert(range.start, u);
                }
            }
            _ => {}
        }
    }
    out
}
