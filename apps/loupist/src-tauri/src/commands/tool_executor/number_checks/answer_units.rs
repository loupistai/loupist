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
