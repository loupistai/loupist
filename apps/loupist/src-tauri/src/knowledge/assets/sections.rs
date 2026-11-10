//! Section aligner for annual and quarterly filings (10-K, 10-Q, 사업보고서,
//! 분기보고서). Splits two retained copies of the same form by heading, then
//! compares normalized paragraphs: identical text is unchanged, a word-set
//! Jaccard similarity of at least 0.6 is changed, anything else is added or
//! removed. Deterministic and model-free; byte ranges point into the newer text.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

const CHANGED_SIMILARITY: f64 = 0.6;
/// Shorter lines are page numbers, running heads and table rules.
const MIN_PARAGRAPH_CHARS: usize = 24;
const MAX_SPANS_PER_SECTION: usize = 64;
/// Paragraphs compared per section: bounds the pairwise similarity work. A
/// longer section is compared on its first paragraphs.
pub const MAX_PARAGRAPHS_PER_SECTION: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Risk,
    Mdna,
    Business,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionDelta {
    pub section: Section,
    pub added: u32,
    pub removed: u32,
    pub changed: u32,
    /// UTF-8 byte ranges of added and changed paragraphs in the newer text.
    pub spans: Vec<(usize, usize)>,
}

/// Forms whose sections are compared.
pub fn sectionable(form: &str) -> bool {
    let form = form.trim().to_uppercase();
    matches!(form.as_str(), "10-K" | "10-Q" | "10-K/A" | "10-Q/A")
        || form.contains("사업보고서")
        || form.contains("분기보고서")
}

#[derive(Debug)]
struct Paragraph {
    start: usize,
    end: usize,
    normalized: String,
    words: BTreeSet<String>,
}

/// What a heading line starts, or `Some(None)` for a heading that ends the
/// current section without starting one we compare.
fn heading(line: &str) -> Option<Option<Section>> {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("item") {
        let rest = rest.trim_start();
        let token: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if token.is_empty() || !token.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
        // A heading names the item and stops; prose that mentions "Item 7" runs on.
        if trimmed.chars().count() > 120 {
            return None;
        }
        return Some(match token.as_str() {
            "1a" if lower.contains("risk factors") => Some(Section::Risk),
            "7" => Some(Section::Mdna),
            "2" if lower.contains("management") => Some(Section::Mdna),
            _ => None,
        });
    }
    let numeral: String = trimmed
        .chars()
        .take_while(|c| matches!(c, 'I' | 'V' | 'X'))
        .collect();
    if !numeral.is_empty()
        && trimmed[numeral.len()..].starts_with('.')
        && trimmed.chars().count() <= 60
    {
        return Some(if trimmed.contains("사업의 내용") {
            Some(Section::Business)
        } else if trimmed.contains("경영진단") {
            Some(Section::Mdna)
        } else {
            None
        });
    }
    None
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '%' | '.' | ','))
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Paragraphs of each compared section. A table of contents repeats the
/// headings with nothing under them, so the longest occurrence wins.
fn split(text: &str) -> BTreeMap<Section, Vec<Paragraph>> {
    let mut sections: BTreeMap<Section, Vec<Paragraph>> = BTreeMap::new();
    let mut current: Option<(Section, Vec<Paragraph>)> = None;
    let mut block_start: Option<usize> = None;
    let mut block_end = 0;
    let mut offset = 0;
    let flush =
        |current: &mut Option<(Section, Vec<Paragraph>)>, start: &mut Option<usize>, end: usize| {
            if let (Some((_, paragraphs)), Some(begin)) = (current.as_mut(), start.take()) {
                let raw = &text[begin..end];
                let normalized = normalize(raw);
                if normalized.chars().count() >= MIN_PARAGRAPH_CHARS
                    && paragraphs.len() < MAX_PARAGRAPHS_PER_SECTION
                {
                    let words = normalized.split(' ').map(str::to_owned).collect();
                    paragraphs.push(Paragraph {
                        start: begin,
                        end,
                        normalized,
                        words,
                    });
                }
            }
        };
    let close = |sections: &mut BTreeMap<Section, Vec<Paragraph>>,
                 current: &mut Option<(Section, Vec<Paragraph>)>| {
        if let Some((section, paragraphs)) = current.take() {
            let longer = sections
                .get(&section)
                .is_none_or(|existing| existing.len() < paragraphs.len());
            if longer {
                sections.insert(section, paragraphs);
            }
        }
    };
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content_end = line_start + line.trim_end_matches(['\n', '\r']).len();
        if let Some(next) = heading(line) {
            flush(&mut current, &mut block_start, block_end);
            close(&mut sections, &mut current);
            current = next.map(|section| (section, Vec::new()));
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut current, &mut block_start, block_end);
            continue;
        }
        if current.is_some() {
            if block_start.is_none() {
                block_start = Some(line_start + (line.len() - line.trim_start().len()));
            }
            block_end = content_end;
        }
    }
    flush(&mut current, &mut block_start, block_end);
    close(&mut sections, &mut current);
    sections
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// Compares every section both copies contain. A section missing from either
/// copy is not compared (its headings were not found).
pub fn align(newer: &str, older: &str) -> Vec<SectionDelta> {
    let new_sections = split(newer);
    let old_sections = split(older);
    let mut deltas = Vec::new();
    for (section, paragraphs) in &new_sections {
        let Some(previous) = old_sections.get(section) else {
            continue;
        };
        let old_text: BTreeSet<&str> = previous.iter().map(|p| p.normalized.as_str()).collect();
        let mut matched_old = vec![false; previous.len()];
        let mut delta = SectionDelta {
            section: *section,
            added: 0,
            removed: 0,
            changed: 0,
            spans: Vec::new(),
        };
        for paragraph in paragraphs {
            if old_text.contains(paragraph.normalized.as_str()) {
                if let Some(index) = previous
                    .iter()
                    .position(|p| p.normalized == paragraph.normalized)
                {
                    matched_old[index] = true;
                }
                continue;
            }
            let best = previous
                .iter()
                .enumerate()
                .map(|(index, old)| (index, jaccard(&paragraph.words, &old.words)))
                .max_by(|a, b| a.1.total_cmp(&b.1));
            match best {
                Some((index, score)) if score >= CHANGED_SIMILARITY => {
                    matched_old[index] = true;
                    delta.changed += 1;
                }
                _ => delta.added += 1,
            }
            if delta.spans.len() < MAX_SPANS_PER_SECTION {
                delta.spans.push((paragraph.start, paragraph.end));
            }
        }
        for (index, old) in previous.iter().enumerate() {
            if matched_old[index] {
                continue;
            }
            let survives = paragraphs
                .iter()
                .any(|new| jaccard(&new.words, &old.words) >= CHANGED_SIMILARITY);
            if !survives {
                delta.removed += 1;
            }
        }
        deltas.push(delta);
    }
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q2: &str = "PART I\n\nItem 1A. Risk Factors .......... 12\nItem 2. Management's Discussion and Analysis .......... 20\n\nItem 1A. Risk Factors\n\nOur largest customer accounted for 21% of revenue and a loss of that customer would hurt results.\n\nSupply of HBM substrates depends on a single qualified supplier in Morrow Components.\n\nForeign exchange movements affect the cost of imported equipment for the new line.\n\nItem 2. Management's Discussion and Analysis of Financial Condition\n\nOperating margin was 16.2% as qualification costs for the new line were capitalized.\n\nRevenue grew on demand for HBM packaging from data center customers.\n\nItem 3. Quantitative and Qualitative Disclosures About Market Risk\n\nInterest rate exposure is limited to the revolving facility.\n";

    const Q3: &str = "PART I\n\nItem 1A. Risk Factors\n\nOur largest customer accounted for 24% of revenue and a loss of that customer would hurt results.\n\nSupply of HBM substrates depends on a single qualified supplier in Morrow Components.\n\nExport controls on advanced packaging equipment could delay qualification of the new line.\n\nA second customer may begin qualifying substrates from a competing packaging house next year.\n\nItem 2. Management's Discussion and Analysis of Financial Condition\n\nOperating margin was 14.8% as qualification costs for the new line were expensed.\n\nRevenue grew on demand for HBM packaging from data center customers.\n\nItem 3. Quantitative and Qualitative Disclosures About Market Risk\n\nInterest rate exposure is limited to the revolving facility.\n";

    #[test]
    fn two_quarterly_reports_align_by_section() {
        let deltas = align(Q3, Q2);
        let risk = deltas.iter().find(|d| d.section == Section::Risk).unwrap();
        // 21% → 24% is a changed paragraph; export controls and the second
        // customer are new; foreign exchange was removed.
        assert_eq!((risk.added, risk.removed, risk.changed), (2, 1, 1));
        let mdna = deltas.iter().find(|d| d.section == Section::Mdna).unwrap();
        assert_eq!((mdna.added, mdna.removed, mdna.changed), (0, 0, 1));
        for (start, end) in &risk.spans {
            let text = &Q3[*start..*end];
            assert!(!text.trim().is_empty() && !text.contains('\n'), "{text:?}");
        }
        assert!(risk
            .spans
            .iter()
            .any(|(s, e)| Q3[*s..*e].contains("Export controls")));
    }

    #[test]
    fn identical_copies_report_no_changes_and_korean_headings_split() {
        assert!(align(Q3, Q3)
            .iter()
            .all(|d| d.added + d.removed + d.changed == 0));
        let old = "I. 회사의 개요\n\n회사는 반도체 후공정 장비를 만드는 회사이며 오산에 공장이 있습니다.\n\nII. 사업의 내용\n\n당사는 포장 라인 두 개를 운영하며 주요 고객은 두 곳입니다 그리고 매출의 대부분을 차지합니다.\n\nIII. 재무에 관한 사항\n\n재무 내용.\n";
        let new = "I. 회사의 개요\n\n회사는 반도체 후공정 장비를 만드는 회사이며 오산에 공장이 있습니다.\n\nII. 사업의 내용\n\n당사는 포장 라인 다섯 개를 운영하며 주요 고객은 두 곳입니다 그리고 매출의 대부분을 차지합니다.\n\n오산 신규 동에 포장 라인 세 개를 증설하기로 결정하였으며 투자 금액은 미정입니다.\n\nIII. 재무에 관한 사항\n\n재무 내용.\n";
        let deltas = align(new, old);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].section, Section::Business);
        assert_eq!(
            (deltas[0].added, deltas[0].removed, deltas[0].changed),
            (1, 0, 1)
        );
        assert!(sectionable("10-Q") && sectionable("분기보고서 (2026.09)") && !sectionable("8-K"));
    }

    #[test]
    fn a_long_section_is_compared_on_a_bounded_number_of_paragraphs() {
        let section = |prefix: &str| {
            let mut text = String::from("Item 1A. Risk Factors\n\n");
            for index in 0..1_000 {
                text.push_str(&format!(
                    "{prefix} risk paragraph number {index} about supply and demand.\n\n"
                ));
            }
            text
        };
        let deltas = align(&section("Newer"), &section("Older"));
        let risk = &deltas[0];
        assert!(risk.added + risk.changed <= MAX_PARAGRAPHS_PER_SECTION as u32);
        assert!(risk.removed <= MAX_PARAGRAPHS_PER_SECTION as u32);
    }
}
