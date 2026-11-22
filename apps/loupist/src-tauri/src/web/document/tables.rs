//! Keep source rows together when the HTML-to-Markdown converter would unwrap
//! a table without explicit TH elements (as in SEC filings).
use scraper::{ElementRef, Html, Node, Selector};

pub(super) fn preserve_tables(html: &str) -> std::borrow::Cow<'_, str> {
    if !html
        .as_bytes()
        .windows(6)
        .any(|tag| tag.eq_ignore_ascii_case(b"<table"))
    {
        return html.into();
    }
    let mut document = Html::parse_document(html);
    let tables = Selector::parse("table").unwrap();
    // A nested table has its own grid. Leave its outer layout container to
    // the ordinary converter, keeping the inner table in document order.
    let replacements = document
        .select(&tables)
        .filter(|table| table.select(&tables).next().is_none())
        .filter_map(|table| render_table(table).map(|text| (table.id(), text)))
        .collect::<Vec<_>>();
    for (id, text) in replacements {
        let mut node = document.tree.get_mut(id).unwrap();
        while let Some(mut child) = node.first_child() {
            child.detach();
        }
        if let Node::Element(element) = node.value() {
            element.name.local = "pre".into();
        }
        node.append(Node::Text(scraper::node::Text { text: text.into() }));
    }
    document.html().into()
}

struct Cell {
    text: String,
    width: usize,
    spans_rows: bool,
}

struct Row {
    cells: Vec<Cell>,
    header: bool,
}

fn read_row(row: ElementRef<'_>) -> Option<Row> {
    let mut header = true;
    let inside_head = row
        .parent()
        .and_then(ElementRef::wrap)
        .is_some_and(|p| p.value().name() == "thead");
    let cells: Vec<_> = row
        .child_elements()
        .filter(|c| matches!(c.value().name(), "th" | "td"))
        .map(|c| {
            let el = c.value();
            header &= inside_head || (el.name() == "th" && el.attr("scope") != Some("row"));
            // Preserve link targets and line breaks. Inline spans, including
            // XBRL tags inside a number, do not introduce whitespace.
            let content = if c.descendent_elements().any(|e| {
                matches!(
                    e.value().name(),
                    "a" | "br" | "p" | "div" | "img" | "ul" | "ol" | "pre"
                )
            }) {
                htmd::convert(&c.inner_html()).unwrap_or_else(|_| c.text().collect())
            } else {
                c.text().collect()
            };
            Cell {
                text: cell_text(&content),
                // HTML limits colspan to 1000. Never allocate per spanned column.
                width: el
                    .attr("colspan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 1000),
                spans_rows: el
                    .attr("rowspan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .is_some_and(|v| v != 1),
            }
        })
        .collect();
    cells
        .iter()
        .any(|c| !c.text.is_empty())
        .then_some(Row { cells, header })
}

fn render_table(table: ElementRef<'_>) -> Option<String> {
    let mut lines = Vec::new();
    let mut rows = Vec::new();
    for part in table.child_elements() {
        match part.value().name() {
            "caption" => lines.push(
                part.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            "tr" => rows.extend(read_row(part)),
            "thead" | "tbody" | "tfoot" => {
                rows.extend(
                    part.child_elements()
                        .filter(|r| r.value().name() == "tr")
                        .filter_map(read_row),
                );
            }
            _ => {}
        }
    }
    lines.extend(render_rows(&rows)?);
    Some(lines.join("\n"))
}

fn render_rows(rows: &[Row]) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    let first = rows.first()?;
    if rows.len() == 1 && first.cells.len() == 1 {
        return None; // A one-cell layout container, with no row/column relationships.
    }
    if let Some(grid) = regular_grid(rows) {
        let used: Vec<_> = (0..first.cells.len())
            .filter(|&i| grid.iter().any(|row| !row[i].is_empty()))
            .collect();
        for (i, row) in grid.iter().enumerate() {
            lines.push(pipe_row(used.iter().map(|&i| row[i].as_str())));
            if i == 0 {
                lines.push(pipe_row(used.iter().map(|_| "---")));
            }
        }
    } else {
        for (index, row) in rows.iter().enumerate() {
            let header = row.header || (index == 0 && year_header(row));
            let mut cells: Vec<String> = Vec::new();
            for cell in &row.cells {
                let mut text = cell.text.clone();
                if !text.is_empty() && (header || cell.spans_rows) {
                    if cell.width > 1 {
                        text.push_str(" [merged columns]");
                    }
                    if cell.spans_rows {
                        text.push_str(" [merged rows]");
                    }
                }
                if let Some(last) = cells.last_mut().filter(|v| joins_amount(v, &text)) {
                    last.push_str(&text);
                } else {
                    cells.push(text);
                }
            }
            lines.push(pipe_row(cells.iter().map(String::as_str)));
        }
    }
    Some(lines)
}

fn cell_text(content: &str) -> String {
    let mut text = String::new();
    for word in content.split_whitespace() {
        if !text.is_empty() && !joins_amount(&text, word) {
            text.push(' ');
        }
        text.push_str(word);
    }
    text.replace('|', "\\|")
}

fn pipe_row<'a>(cells: impl Iterator<Item = &'a str>) -> String {
    format!("| {} |", cells.collect::<Vec<_>>().join(" | "))
}

fn joins_amount(left: &str, right: &str) -> bool {
    let currency = |s: &str| matches!(s, "$" | "£" | "€" | "¥" | "₩");
    let prefix = left == "(" || currency(left.trim_matches('('));
    let number = |s: &str| {
        s.bytes().any(|b| b.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_digit() || "$£€¥₩.,()+−-%".contains(c))
    };
    (prefix && number(right))
        || (left == "(" && currency(right))
        || (currency(left) && right == "(")
        || (matches!(right, ")" | "%" | "%)") && number(left))
}

fn year_header(row: &Row) -> bool {
    let year = |s: &str| {
        s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()) && s.starts_with(['1', '2'])
    };
    let Some((label, headings)) = row.cells.split_first() else {
        return false;
    };
    (label.text.is_empty()
        || ["region", "year", "fiscal year"]
            .iter()
            .any(|s| label.text.eq_ignore_ascii_case(s)))
        && headings.iter().any(|c| year(&c.text))
        && headings
            .iter()
            .all(|c| c.text.is_empty() || year(&c.text) || c.text.eq_ignore_ascii_case("change"))
}

/// A filing's year header may be made entirely of TDs. Its colspans say
/// exactly which body cells form each amount; never move data across one.
fn regular_grid(rows: &[Row]) -> Option<Vec<Vec<String>>> {
    let (header, body) = rows.split_first()?;
    if body.is_empty()
        || body.iter().any(|r| r.header)
        || rows.iter().any(|r| r.cells.iter().any(|c| c.spans_rows))
    {
        return None;
    }
    if !header.header && !year_header(header) {
        return None;
    }
    let mut grid = vec![header.cells.iter().map(|c| c.text.clone()).collect()];
    for row in body {
        let mut cells = row.cells.iter();
        let mut values = Vec::with_capacity(header.cells.len().min(row.cells.len()));
        for column in &header.cells {
            let mut remaining = column.width;
            let mut value = String::new();
            while remaining > 0 {
                let cell = cells.next()?;
                remaining = remaining.checked_sub(cell.width)?;
                if !cell.text.is_empty() {
                    if !value.is_empty() && !joins_amount(&value, &cell.text) {
                        return None;
                    }
                    value.push_str(&cell.text);
                }
            }
            values.push(value);
        }
        if cells.next().is_some() {
            return None;
        }
        grid.push(values);
    }
    Some(grid)
}

// PDF text has a reading order and may contain an independently identifiable
// grid. Replace only complete runs of lines whose words agree with that grid,
// leaving captions, prose and ambiguous layouts in their original positions.
pub(super) fn pdf_page(
    document: &pdf_oxide::PdfDocument,
    index: usize,
) -> Result<String, pdf_oxide::error::Error> {
    let page = document.extract_text(index)?;
    if page.lines().count() < 2 {
        return Ok(page);
    }
    let tables = document.extract_tables(index).unwrap_or_default();
    if tables.is_empty() {
        return Ok(page);
    }
    let lines: Vec<_> = page.split_inclusive('\n').collect();
    let mut by_start = std::collections::HashMap::<&str, Vec<usize>>::new();
    let mut offsets = vec![0];
    for (i, line) in lines.iter().enumerate() {
        offsets.push(offsets[i] + line.len());
        if let Some(word) = line.split_whitespace().next() {
            by_start.entry(word).or_default().push(i);
        }
    }
    let mut replacements = Vec::new();
    for table in tables {
        let words: Vec<_> = table
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .flat_map(|c| c.text.split_whitespace())
            .collect();
        let Some(starts) = words.first().and_then(|w| by_start.get(w)) else {
            continue;
        };
        let mut ranges = starts.iter().filter_map(|&start| {
            let mut words_left = &words[..];
            for (i, line) in lines[start..].iter().enumerate() {
                for word in line.split_whitespace() {
                    let (next, rest) = words_left.split_first()?;
                    if *next != word {
                        return None;
                    }
                    words_left = rest;
                }
                if words_left.is_empty() {
                    return Some(
                        offsets[start]
                            ..offsets[start + i] + line.trim_end_matches(['\r', '\n']).len(),
                    );
                }
            }
            None
        });
        let Some(range) = ranges.next() else { continue };
        if ranges.next().is_some() {
            continue;
        }
        let rows = table
            .rows
            .iter()
            .map(|r| Row {
                header: r.is_header,
                cells: r
                    .cells
                    .iter()
                    .map(|c| Cell {
                        text: cell_text(&c.text),
                        width: c.colspan.max(1) as usize,
                        spans_rows: c.rowspan > 1,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        if let Some(rows) = render_rows(&rows) {
            replacements.push((range, rows.join("\n")));
        }
    }
    if replacements.is_empty() {
        return Ok(page);
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut formatted = String::with_capacity(page.len());
    let mut cursor = 0;
    let mut previous_end = 0;
    let mut grids = replacements.into_iter().peekable();
    while let Some((range, text)) = grids.next() {
        let overlaps = range.start < previous_end
            || grids.peek().is_some_and(|(next, _)| next.start < range.end);
        previous_end = previous_end.max(range.end);
        if !overlaps {
            formatted.push_str(&page[cursor..range.start]);
            formatted.push_str(&text);
            cursor = range.end;
        }
    }
    formatted.push_str(&page[cursor..]);
    Ok(formatted)
}
