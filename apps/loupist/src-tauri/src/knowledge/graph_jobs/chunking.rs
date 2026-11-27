//! How a long Source span is cut into World episodes.
//!
//! Version 1 cuts at blank lines and then every [`EPISODE_CHUNK_CHARS`]
//! characters. Version 2 cuts between blocks (a Markdown table, an HTML
//! `<table>`, a fenced block, a paragraph), never inside a table row, cuts a
//! table into runs of at most [`TABLE_CHUNK_ROWS`] rows, and opens each
//! continuation of a table with its header and unit line. A job
//! stores the version that cut it, so a Source keeps one cut for its life.

use super::{KnowledgeError, KnowledgeResult, EPISODE_CHUNK_CHARS};
use crate::knowledge::numbers::{names_currency, unambiguous_currency_markers, Unit};
use std::ops::Range;

pub(crate) const CHUNKER_PARAGRAPHS: u32 = 1;
pub(crate) const CHUNKER_BLOCKS: u32 = 2;
/// The version a Source with no job is cut with.
pub(crate) const CURRENT_CHUNKER: u32 = CHUNKER_BLOCKS;

/// Repeated context never exceeds this many characters.
const CONTEXT_CHARS: usize = 600;
/// A table chunk carries at most this many data rows. The long-table
/// Sources of the evaluation print five or six figures a row, so a chunk
/// holds at most 120 to 144 cells. Cut by characters alone, the Markdown one
/// put 67 rows, 402 cells, in its first chunk, and its extraction call timed
/// out; at 40 rows a call still took 75 to 110 s of its 120 s and sometimes
/// timed out.
const TABLE_CHUNK_ROWS: usize = 24;
/// How far above a table its unit line may stand.
const UNIT_LINE_REACH: usize = 3;
/// Opens the repeated context of a continued table. It counts against
/// [`CONTEXT_CHARS`], so it stays short.
const CONTEXT_MARKER: &str =
    "[Continued table. The lines down to its first row are repeated from earlier in this \
     Source to read the rows; they are not new facts.]\n";

/// One episode: the text the model reads and its true range in the Source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    /// Repeated context, then the Source text in `chars`.
    pub body: String,
    /// Characters of `body` that are repeated context, not Source text.
    pub context_chars: usize,
    /// Character offsets into the Source.
    pub chars: Range<u32>,
}

/// Cut `body` with chunker `version`. A body within the budget is one chunk
/// under every version, so its deduplication key never changes.
pub(crate) fn chunk_episode(body: &str, version: u32) -> KnowledgeResult<Vec<Chunk>> {
    let cuts = if body.chars().count() <= EPISODE_CHUNK_CHARS {
        vec![Cut {
            context: None,
            bytes: 0..body.len(),
        }]
    } else {
        match version {
            CHUNKER_PARAGRAPHS => paragraphs(body),
            CHUNKER_BLOCKS => blocks(body),
            _ => {
                return Err(KnowledgeError::Invalid(
                    "graph episode was cut by an unknown chunker",
                ))
            }
        }
    };
    let mut start = 0u32;
    Ok(cuts
        .into_iter()
        .map(|cut| {
            let text = &body[cut.bytes];
            let end = start + text.chars().count() as u32;
            let context = cut.context.unwrap_or_default();
            let chunk = Chunk {
                context_chars: context.chars().count(),
                body: context + text,
                chars: start..end,
            };
            start = end;
            chunk
        })
        .collect())
}

/// A chunk before it is rendered: optional repeated context and a byte range.
struct Cut {
    context: Option<String>,
    bytes: Range<usize>,
}

/// Version 1: blank-line paragraphs, then fixed character runs.
fn paragraphs(body: &str) -> Vec<Cut> {
    let mut packer = Packer::default();
    let mut offset = 0;
    for paragraph in body.split_inclusive("\n\n") {
        let range = offset..offset + paragraph.len();
        offset = range.end;
        let chars = paragraph.chars().count();
        if chars > EPISODE_CHUNK_CHARS {
            packer.flush();
            packer.split_oversized(body, range);
            continue;
        }
        if !packer.fits(chars) {
            packer.flush();
        }
        packer.push(range, chars);
    }
    packer.finish()
}

/// Version 2: a block that fits a chunk is never split; a longer one fills
/// the open chunk and continues piece by piece, a table between rows. A
/// table of more than [`TABLE_CHUNK_ROWS`] rows is cut into the fewest equal
/// runs of rows within that bound.
fn blocks(body: &str) -> Vec<Cut> {
    let mut packer = Packer::default();
    for block in parse_blocks(body) {
        let chars = body[block.bytes.clone()].chars().count();
        let rows = block.table.as_ref().map_or(0, |_| block.pieces.len());
        if chars <= EPISODE_CHUNK_CHARS && rows <= TABLE_CHUNK_ROWS {
            if !packer.fits(chars) {
                packer.flush();
            }
            packer.push(block.bytes, chars);
            continue;
        }
        let context = block.table.as_ref().and_then(|table| context(body, table));
        let context_chars = context.as_ref().map_or(0, |text| text.chars().count());
        let per_chunk = match rows {
            0 => usize::MAX,
            rows => rows.div_ceil(rows.div_ceil(TABLE_CHUNK_ROWS)),
        };
        let mut in_chunk = 0;
        for (index, piece) in block.pieces.into_iter().enumerate() {
            let chars = body[piece.clone()].chars().count();
            if chars > EPISODE_CHUNK_CHARS {
                packer.flush();
                packer.split_oversized(body, piece);
                in_chunk = 1;
                continue;
            }
            if !packer.fits(chars) || in_chunk == per_chunk {
                packer.flush();
                in_chunk = 0;
                // The first piece holds the header itself.
                if let Some(context) = context
                    .as_ref()
                    .filter(|_| index > 0 && context_chars + chars <= EPISODE_CHUNK_CHARS)
                {
                    packer.open_with(context.clone(), context_chars);
                }
            }
            packer.push(piece, chars);
            in_chunk += 1;
        }
    }
    packer.finish()
}

/// Greedy packing of adjacent byte ranges into chunks within the budget.
#[derive(Default)]
struct Packer {
    done: Vec<Cut>,
    context: Option<String>,
    bytes: Option<Range<usize>>,
    chars: usize,
}

impl Packer {
    fn fits(&self, chars: usize) -> bool {
        self.chars + chars <= EPISODE_CHUNK_CHARS
    }

    fn push(&mut self, range: Range<usize>, chars: usize) {
        self.bytes = Some(match self.bytes.take() {
            Some(current) => current.start..range.end,
            None => range,
        });
        self.chars += chars;
    }

    /// Start the next chunk with repeated context. Only called right after a flush.
    fn open_with(&mut self, context: String, chars: usize) {
        self.context = Some(context);
        self.chars = chars;
    }

    fn flush(&mut self) {
        if let Some(bytes) = self.bytes.take() {
            self.done.push(Cut {
                context: self.context.take(),
                bytes,
            });
        }
        self.context = None;
        self.chars = 0;
    }

    /// A piece longer than the budget: full runs, the remainder left open.
    fn split_oversized(&mut self, body: &str, range: Range<usize>) {
        let mut start = range.start;
        let mut chars = 0;
        for (at, ch) in body[range.clone()].char_indices() {
            chars += 1;
            if chars == EPISODE_CHUNK_CHARS {
                let end = range.start + at + ch.len_utf8();
                self.push(start..end, chars);
                self.flush();
                start = end;
                chars = 0;
            }
        }
        if start < range.end {
            self.push(start..range.end, chars);
        }
    }

    fn finish(mut self) -> Vec<Cut> {
        self.flush();
        self.done
    }
}

/// A block of the Source, the pieces it may be split into, and its table.
struct Block {
    bytes: Range<usize>,
    pieces: Vec<Range<usize>>,
    table: Option<Table>,
}

/// Where a table's header ends and where it starts in the Source.
struct Table {
    start: usize,
    header: Range<usize>,
}

/// Lines with their byte ranges, each including its line ending.
fn lines(body: &str) -> Vec<Range<usize>> {
    let mut offset = 0;
    body.split_inclusive('\n')
        .map(|line| {
            let range = offset..offset + line.len();
            offset = range.end;
            range
        })
        .collect()
}

fn parse_blocks(body: &str) -> Vec<Block> {
    let text = Text {
        body,
        lower: body.to_ascii_lowercase(),
    };
    let lines = lines(body);
    let mut blocks = Vec::new();
    let mut prose: Option<usize> = None;
    let mut index = 0;
    while index < lines.len() {
        let found = fenced(body, &lines, index)
            .or_else(|| markdown_table(body, &lines, index))
            .or_else(|| html_table(&text, &lines, index));
        let Some((block, next)) = found else {
            prose.get_or_insert(lines[index].start);
            index += 1;
            continue;
        };
        if let Some(start) = prose.take() {
            blocks.extend(paragraph_blocks(body, start..block.bytes.start));
        }
        blocks.push(block);
        index = next;
    }
    if let Some(start) = prose {
        blocks.extend(paragraph_blocks(body, start..body.len()));
    }
    blocks
}

/// Each blank-line paragraph of prose is a block; a longer one splits at
/// line ends.
fn paragraph_blocks(body: &str, bytes: Range<usize>) -> Vec<Block> {
    let mut offset = bytes.start;
    body[bytes]
        .split_inclusive("\n\n")
        .map(|paragraph| {
            let range = offset..offset + paragraph.len();
            offset = range.end;
            let pieces = if paragraph.chars().count() > EPISODE_CHUNK_CHARS {
                lines(paragraph)
                    .into_iter()
                    .map(|line| range.start + line.start..range.start + line.end)
                    .collect()
            } else {
                vec![range.clone()]
            };
            Block {
                bytes: range,
                pieces,
                table: None,
            }
        })
        .collect()
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let mark = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = rest.chars().take_while(|c| *c == mark).count();
    (indent <= 3 && run >= 3).then_some((mark, run))
}

/// A fenced block runs to its closing fence or the end of the Source. It
/// splits only between lines.
fn fenced(body: &str, lines: &[Range<usize>], index: usize) -> Option<(Block, usize)> {
    let (mark, run) = fence_marker(&body[lines[index].clone()])?;
    let close = lines[index + 1..]
        .iter()
        .position(|line| {
            let text = body[line.clone()].trim_end();
            fence_marker(text).is_some_and(|(found, length)| {
                found == mark && length >= run && text.trim_start().chars().all(|c| c == mark)
            })
        })
        .map_or(lines.len(), |at| index + 1 + at + 1);
    let pieces = lines[index..close].to_vec();
    let bytes = lines[index].start..lines[close - 1].end;
    Some((
        Block {
            bytes,
            pieces,
            table: None,
        },
        close,
    ))
}

fn pipe_line(text: &str) -> bool {
    text.trim_start().starts_with('|')
}

fn delimiter_row(text: &str) -> bool {
    let row = text.trim();
    let row = row.strip_prefix('|').unwrap_or(row);
    let row = row.strip_suffix('|').unwrap_or(row);
    let mut cells = row.split('|').map(str::trim).peekable();
    cells.peek().is_some()
        && cells.all(|cell| {
            let dashes = cell.trim_start_matches(':').trim_end_matches(':');
            !dashes.is_empty() && dashes.chars().all(|c| c == '-')
        })
}

/// Consecutive lines that start with `|`, with a delimiter row under at
/// least one header row. Each body row is a piece; the header and
/// delimiter travel with the first.
fn markdown_table(body: &str, lines: &[Range<usize>], index: usize) -> Option<(Block, usize)> {
    let run = lines[index..]
        .iter()
        .take_while(|line| pipe_line(&body[(*line).clone()]))
        .count();
    let delimiter = (1..run).find(|at| delimiter_row(&body[lines[index + at].clone()]))?;
    let table = &lines[index..index + run];
    let header = table[0].start..table[delimiter].end;
    let bytes = table[0].start..table[run - 1].end;
    Some((
        Block {
            pieces: row_pieces(
                bytes.clone(),
                table[delimiter + 1..].iter().map(|row| row.end),
            ),
            table: Some(Table {
                start: header.start,
                header,
            }),
            bytes,
        },
        index + run,
    ))
}

/// The Source with an ASCII-lowercase copy for tag search (same byte offsets).
struct Text<'a> {
    body: &'a str,
    lower: String,
}

impl Text<'_> {
    /// The first `tag` (`<tr`, `</table`) in `range` that a `>`, `/` or
    /// whitespace ends.
    fn tag(&self, range: Range<usize>, tag: &str) -> Option<usize> {
        let mut from = range.start;
        while from < range.end {
            let at = from + self.lower[from..range.end].find(tag)?;
            let next = self.lower.as_bytes().get(at + tag.len());
            if next.is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/')) {
                return Some(at);
            }
            from = at + 1;
        }
        None
    }
}

/// An HTML `<table>` from the line it opens on through the line it closes on.
/// Rows of the outer table are the split points; a nested table stays inside
/// its row. Leading rows with a `<th>` and no `<td>` are the header; the
/// header and anything before the first row travel with the first body row.
fn html_table(text: &Text<'_>, lines: &[Range<usize>], index: usize) -> Option<(Block, usize)> {
    let end = text.body.len();
    let open = text.tag(lines[index].clone(), "<table")?;
    const TAGS: [&str; 3] = ["<table", "</table", "<tr"];
    let mut next = TAGS.map(|tag| text.tag(open..end, tag));
    let mut depth = 0;
    let mut rows = Vec::new();
    let close = loop {
        let (which, at) = next
            .iter()
            .enumerate()
            .filter_map(|(which, at)| at.map(|at| (which, at)))
            .min_by_key(|(_, at)| *at)?;
        next[which] = text.tag(at + 1..end, TAGS[which]);
        match which {
            0 => depth += 1,
            1 => {
                depth -= 1;
                if depth == 0 {
                    break at;
                }
            }
            _ if depth == 1 => rows.push(at),
            _ => {}
        }
    };
    let close_line = index + lines[index..].iter().position(|line| line.end > close)?;
    let bytes = lines[index].start..lines[close_line].end;
    let mut bounds = rows;
    bounds.push(bytes.end);
    let mut header_end = bounds[0];
    for row in bounds.windows(2) {
        let row = row[0]..row[1];
        if text.tag(row.clone(), "<th").is_some() && text.tag(row.clone(), "<td").is_none() {
            header_end = row.end;
        } else {
            break;
        }
    }
    let rows = bounds.into_iter().filter(|bound| *bound > header_end);
    Some((
        Block {
            pieces: row_pieces(bytes.clone(), rows),
            table: Some(Table {
                start: bytes.start,
                header: bytes.start..header_end,
            }),
            bytes,
        },
        close_line + 1,
    ))
}

/// One piece per row end, from the table's start; the first piece carries
/// what precedes its row. A table with no body row is one piece.
fn row_pieces(bytes: Range<usize>, ends: impl Iterator<Item = usize>) -> Vec<Range<usize>> {
    let mut start = bytes.start;
    let pieces: Vec<_> = ends
        .map(|end| {
            let piece = start..end;
            start = end;
            piece
        })
        .collect();
    if pieces.is_empty() {
        vec![bytes]
    } else {
        pieces
    }
}

/// Whether a line states a currency, a scale or a unit, as `numbers.rs` reads
/// one: a currency marker anywhere, or a unit for the whole line, a bracket,
/// or the text after a colon.
fn states_unit(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    if !unambiguous_currency_markers(line).is_empty() {
        return true;
    }
    let placed = |text: &str| matches!(Unit::of_line(Some(text)), Ok(unit) if unit != Unit::Plain);
    let mut segments = vec![line];
    for (open, close) in [('(', ')'), ('[', ']'), ('（', '）'), ('【', '】')] {
        let mut rest = line;
        while let Some(at) = rest.find(open) {
            let inner = &rest[at + open.len_utf8()..];
            let Some(end) = inner.find(close) else {
                break;
            };
            segments.push(&inner[..end]);
            rest = &inner[end + close.len_utf8()..];
        }
    }
    let mut more = Vec::new();
    for segment in &segments {
        if let Some((_, after)) = segment.split_once([':', '：']) {
            more.push(after);
        }
    }
    segments.extend(more);
    segments.into_iter().any(placed)
}

/// The sentences of a line. A `.`, `!` or `?` before a space ends one unless
/// the next word is a currency, as after an abbreviated scale ("Mio. EUR");
/// a `。` always ends one.
fn sentences(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    for (at, ch) in line.char_indices() {
        let end = at + ch.len_utf8();
        let rest = &line[end..];
        let ends = match ch {
            '。' => true,
            '.' | '!' | '?' => {
                rest.starts_with(char::is_whitespace)
                    && !rest.split_whitespace().next().is_some_and(|word| {
                        names_currency(word.trim_end_matches(['.', ',', ';', ')']))
                    })
            }
            _ => false,
        };
        if ends {
            found.push(line[start..end].trim());
            start = end;
        }
    }
    found.push(line[start..].trim());
    found.retain(|sentence| !sentence.is_empty());
    found
}

/// The sentence of `line` that states the table's unit: the last that
/// states one, preferring a sentence with no digit, which states no amount.
fn unit_sentence(line: &str) -> Option<&str> {
    let stating: Vec<_> = sentences(line)
        .into_iter()
        .filter(|sentence| states_unit(sentence))
        .collect();
    stating
        .iter()
        .rev()
        .find(|sentence| !sentence.contains(|c: char| c.is_ascii_digit()))
        .or(stating.last())
        .copied()
}

/// The unit statement of the nearest line within [`UNIT_LINE_REACH`] lines
/// above `start` that states one: a parenthesised unit line, or the
/// sentence of an introducing paragraph that states it.
fn unit_line(body: &str, start: usize) -> Option<&str> {
    body[..start]
        .split_inclusive('\n')
        .rev()
        .take(UNIT_LINE_REACH)
        .find_map(unit_sentence)
}

/// What a continuation chunk of `table` repeats: the marker, the unit line
/// and the header, within [`CONTEXT_CHARS`]. The unit line comes first; a
/// header that does not fit is left out whole.
fn context(body: &str, table: &Table) -> Option<String> {
    let mut context = CONTEXT_MARKER.to_string();
    let budget = |text: &str| text.chars().count() <= CONTEXT_CHARS;
    if let Some(line) = unit_line(body, table.start) {
        let mut candidate = context.clone() + line.trim_end_matches(['\r', '\n']) + "\n";
        if budget(&candidate) {
            std::mem::swap(&mut context, &mut candidate);
        }
    }
    let header = &body[table.header.clone()];
    let mut candidate = context.clone() + header;
    if !header.ends_with('\n') {
        candidate.push('\n');
    }
    if budget(&candidate) {
        context = candidate;
    }
    (context.len() > CONTEXT_MARKER.len()).then_some(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// splitmix64: a seeded generator with no dependency.
    struct Seeded(u64);

    impl Seeded {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.below(items.len())]
        }
    }

    /// Unit lines of fictional reports from several markets.
    const UNIT_LINES: &[&str] = &[
        "(단위: 백만원)",
        "(in millions of USD)",
        "(単位：百万円)",
        "Angaben in Mio. EUR",
        "(em milhões de BRL)",
        "(INR in crore)",
        "单位：亿元",
        "(HKD 万)",
        "Figures in GBP thousands",
    ];
    /// Unit statements that close a sentence introducing a table.
    const UNIT_SENTENCES: &[&str] = &[
        "All amounts are in ₹ crore.",
        "Angaben in Mio. EUR.",
        "Figures are unaudited (in millions of yen).",
        "단위: 억원",
        "Os valores em R$ milhões.",
        "Monetary amounts are in £m.",
        "金额单位：亿元。",
    ];
    /// Sentences that open such a paragraph and state no unit.
    const INTRODUCTIONS: &[&str] = &[
        "Segment information for the half year ended 30 September 2026.",
        "Unaudited.",
        "부문별 실적입니다.",
        "Umsatz nach Segmenten im Halbjahr 2026.",
        "事業別の見通し。",
    ];
    /// Prose that states no unit, in several scripts.
    const WORDS: &[&str] = &[
        "segment",
        "outlook",
        "부문",
        "실적",
        "見通し",
        "事業",
        "Umsatz",
        "Ausblick",
        "receita",
        "perspectiva",
        "业务",
        "展望",
        "quarter",
        "Halbjahr",
        "conselho",
    ];
    const CELLS: &[&str] = &[
        "1,204",
        "3.5",
        "−12",
        "(87)",
        "67,500",
        "1.234,5",
        "4,08",
        "▲120",
        "Nordwind AG",
        "솔숲식품",
        "青葉精工",
        "Ipê Têxtil",
        "Lotus Retail",
        "—",
    ];

    struct Generated {
        body: String,
        /// Character ranges of table rows, with their table's expected context.
        rows: Vec<(Range<usize>, usize)>,
        contexts: Vec<Option<String>>,
    }

    fn chars(text: &str) -> usize {
        text.chars().count()
    }

    fn prose(rng: &mut Seeded, out: &mut String) {
        let words = 5 + rng.below(80);
        for at in 0..words {
            if at > 0 {
                out.push(' ');
            }
            out.push_str(rng.pick(WORDS));
        }
        out.push_str(".\n\n");
    }

    fn row_text(rng: &mut Seeded, columns: usize, long: bool) -> Vec<String> {
        (0..columns)
            .map(|column| {
                if long && column == 0 {
                    "長".repeat(EPISODE_CHUNK_CHARS + rng.below(500))
                } else {
                    rng.pick(CELLS).to_string()
                }
            })
            .collect()
    }

    fn generate(seed: u64) -> Generated {
        let mut rng = Seeded(seed);
        let mut body = String::new();
        let mut rows = Vec::new();
        let mut contexts = Vec::new();
        let blocks = 2 + rng.below(6);
        for _ in 0..blocks {
            match rng.below(4) {
                0 => prose(&mut rng, &mut body),
                1 => {
                    body.push_str("```\n");
                    for _ in 0..rng.below(40) {
                        body.push_str(rng.pick(CELLS));
                        body.push('\n');
                    }
                    body.push_str("```\n\n");
                }
                kind => {
                    let unit = match rng.below(3) {
                        0 => None,
                        1 => Some(*rng.pick(UNIT_LINES)),
                        _ => {
                            for _ in 0..rng.below(3) {
                                body.push_str(rng.pick(INTRODUCTIONS));
                                body.push(' ');
                            }
                            Some(*rng.pick(UNIT_SENTENCES))
                        }
                    };
                    if let Some(unit) = unit {
                        body.push_str(unit);
                        body.push_str(if rng.below(2) == 0 { "\n" } else { "\n\n" });
                    }
                    let html = kind == 3;
                    let columns = 2 + rng.below(5);
                    let header_row = row_text(&mut rng, columns, false);
                    let header = if html {
                        let cells: String =
                            header_row.iter().map(|c| format!("<th>{c}</th>")).collect();
                        format!(
                            "<table class=\"report\">\n<thead><tr>{cells}</tr></thead>\n<tbody>\n"
                        )
                    } else {
                        format!(
                            "| {} |\n|{}|\n",
                            header_row.join(" | "),
                            vec![" --- "; columns].join("|")
                        )
                    };
                    body.push_str(&header);
                    let mut context = CONTEXT_MARKER.to_string();
                    if let Some(unit) = unit {
                        context.push_str(unit);
                        context.push('\n');
                    }
                    context.push_str(&header);
                    contexts.push((chars(&context) <= CONTEXT_CHARS).then_some(context));
                    let count = 1 + rng.below(400);
                    for _ in 0..count {
                        let long = rng.below(300) == 0;
                        let cells = row_text(&mut rng, columns, long);
                        let row = if html {
                            let cells: String =
                                cells.iter().map(|c| format!("<td>{c}</td>")).collect();
                            format!("<tr>{cells}</tr>\n")
                        } else {
                            format!("| {} |\n", cells.join(" | "))
                        };
                        let from = chars(&body);
                        body.push_str(&row);
                        rows.push((from..chars(&body), contexts.len() - 1));
                    }
                    if html {
                        body.push_str("</tbody>\n</table>\n");
                    }
                    body.push('\n');
                    prose(&mut rng, &mut body);
                }
            }
        }
        Generated {
            body,
            rows,
            contexts,
        }
    }

    /// The invariants every version keeps: the Source text of the chunks,
    /// without repeated context, is the body; ranges are exact; no chunk
    /// exceeds the budget.
    fn exact(body: &str, chunks: &[Chunk], seed: u64) {
        let source: Vec<char> = body.chars().collect();
        let mut at = 0u32;
        for chunk in chunks {
            assert_eq!(chunk.chars.start, at, "seed {seed}: ranges are contiguous");
            let text: String = chunk.body.chars().skip(chunk.context_chars).collect();
            let range: String = source[chunk.chars.start as usize..chunk.chars.end as usize]
                .iter()
                .collect();
            assert_eq!(
                text, range,
                "seed {seed}: the range is the chunk's Source text"
            );
            assert!(!text.is_empty(), "seed {seed}: no empty chunk");
            assert!(
                chars(&chunk.body) <= EPISODE_CHUNK_CHARS,
                "seed {seed}: chunk of {} characters",
                chars(&chunk.body)
            );
            at = chunk.chars.end;
        }
        assert_eq!(
            at as usize,
            source.len(),
            "seed {seed}: the chunks cover the body"
        );
    }

    #[test]
    fn version_two_never_splits_a_row_and_repeats_its_table_context() {
        let seeds = std::env::var("LOUPIST_CHUNK_SEED")
            .ok()
            .and_then(|seed| seed.parse::<u64>().ok())
            .map_or(0..300, |seed| seed..seed + 1);
        let mut continued = 0;
        for seed in seeds {
            let generated = generate(seed);
            let body = &generated.body;
            let chunks = chunk_episode(body, CHUNKER_BLOCKS).unwrap();
            exact(body, &chunks, seed);
            exact(
                body,
                &chunk_episode(body, CHUNKER_PARAGRAPHS).unwrap(),
                seed,
            );
            for (row, table) in &generated.rows {
                let row_chars = row.end - row.start;
                for chunk in &chunks[1..] {
                    let cut = chunk.chars.start as usize;
                    if row_chars <= EPISODE_CHUNK_CHARS {
                        assert!(
                            cut <= row.start || cut >= row.end,
                            "seed {seed}: row {row:?} split at {cut}"
                        );
                    }
                    // A continuation that opens on this row repeats its table's context.
                    let expected = &generated.contexts[*table];
                    if cut == row.start {
                        if let Some(context) = expected
                            .as_ref()
                            .filter(|context| chars(context) + row_chars <= EPISODE_CHUNK_CHARS)
                        {
                            assert!(
                                chunk.body.starts_with(context.as_str()),
                                "seed {seed}: chunk at {cut} lacks its context\n{context:?}\n{:?}",
                                chunk.body.chars().take(300).collect::<String>()
                            );
                            assert_eq!(chunk.context_chars, chars(context), "seed {seed}");
                            continued += 1;
                        }
                    }
                }
            }
            for chunk in &chunks {
                let range = chunk.chars.start as usize..chunk.chars.end as usize;
                // A Source within the budget stays one chunk.
                let long = chars(body) > EPISODE_CHUNK_CHARS;
                for table in (0..generated.contexts.len()).filter(|_| long) {
                    let rows = generated
                        .rows
                        .iter()
                        .filter(|(row, of)| *of == table && range.contains(&row.start))
                        .count();
                    assert!(
                        rows <= TABLE_CHUNK_ROWS,
                        "seed {seed}: a chunk carries {rows} rows of table {table}"
                    );
                }
                if chunk.context_chars > 0 {
                    assert!(
                        generated
                            .rows
                            .iter()
                            .any(|(row, _)| row.start == chunk.chars.start as usize),
                        "seed {seed}: context opens only a continued table row"
                    );
                }
            }
        }
        assert!(continued > 0, "the corpus continues some table");
    }

    #[test]
    fn a_source_within_the_budget_is_one_identical_chunk_under_both_versions() {
        let body = format!(
            "(단위: 백만원)\n| 과목 | 당기 |\n|---|---|\n{}",
            "| 매출 | 67,500 |\n".repeat(300)
        );
        assert!(chars(&body) <= EPISODE_CHUNK_CHARS);
        let one = chunk_episode(&body, CHUNKER_PARAGRAPHS).unwrap();
        assert_eq!(one, chunk_episode(&body, CHUNKER_BLOCKS).unwrap());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].body, body);
        assert!(chunk_episode(&"x".repeat(EPISODE_CHUNK_CHARS + 1), 3).is_err());
    }

    #[test]
    fn unit_lines_of_every_market_are_found_and_prose_is_not() {
        for line in UNIT_LINES {
            assert!(states_unit(line), "{line}");
        }
        for line in [
            "Revenue by segment",
            "부문별 실적",
            "Umsatz nach Segmenten im Halbjahr",
            "Receita por segmento",
            "事業別の見通し",
            "Outlook for FY2026-27",
            "",
        ] {
            assert!(!states_unit(line), "{line}");
        }
        for unit in UNIT_SENTENCES {
            for introduction in INTRODUCTIONS {
                let body = format!("{introduction} {unit}\n\n| a | b |\n");
                let start = body.find('|').unwrap();
                assert_eq!(unit_line(&body, start), Some(*unit), "{body}");
            }
        }
    }

    /// The Sources of the long-table evaluation, by scenario id.
    fn long_table_source(id: &str) -> String {
        let suite: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../evals/world/smoke-long-tables-v1.json"
        ))
        .unwrap();
        let scenario = suite["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scenario| scenario["id"] == id)
            .unwrap();
        scenario["steps"][0]["sources"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_long_table_is_cut_into_equal_runs_of_rows() {
        for (id, row, runs) in [
            ("LONG-IN-MARKDOWN", "| ", [21, 21, 21, 21]),
            ("LONG-UK-HTML", "<tr><th scope=\"row\">", [24, 24, 24, 0]),
        ] {
            let body = long_table_source(id);
            let chunks = chunk_episode(&body, CHUNKER_BLOCKS).unwrap();
            let rows: Vec<usize> = chunks
                .iter()
                .map(|chunk| {
                    let text: String = chunk.body.chars().skip(chunk.context_chars).collect();
                    text.lines()
                        .filter(|line| line.starts_with(row) && !delimiter_row(line))
                        .count()
                })
                .collect();
            let expected: Vec<usize> = runs.into_iter().filter(|rows| *rows > 0).collect();
            // The first Markdown chunk also holds the header row.
            let header = usize::from(id == "LONG-IN-MARKDOWN");
            let mut found = rows.clone();
            found[0] -= header;
            assert_eq!(found, expected, "{id}");
        }
    }

    #[test]
    fn a_continued_table_repeats_the_unit_its_introducing_sentence_states() {
        for (id, unit) in [
            ("LONG-IN-MARKDOWN", "All amounts are in ₹ crore.\n"),
            ("LONG-UK-HTML", "Monetary amounts are in £m.\n"),
        ] {
            let body = long_table_source(id);
            let chunks = chunk_episode(&body, CHUNKER_BLOCKS).unwrap();
            assert!(chunks.len() > 1, "{id}");
            for chunk in &chunks[1..] {
                let context: String = chunk.body.chars().take(chunk.context_chars).collect();
                assert!(
                    context.starts_with(&(CONTEXT_MARKER.to_string() + unit)),
                    "{id}: {context:?}"
                );
            }
        }
    }

    /// Version 1 as it shipped, kept verbatim as the oracle for stored cuts.
    fn shipped_version_one(body: &str) -> Vec<String> {
        if body.chars().count() <= EPISODE_CHUNK_CHARS {
            return vec![body.to_string()];
        }
        let mut chunks = Vec::new();
        let mut current = String::new();
        let mut current_chars = 0;
        for paragraph in body.split_inclusive("\n\n") {
            let paragraph_chars = paragraph.chars().count();
            if paragraph_chars > EPISODE_CHUNK_CHARS {
                if !current.is_empty() {
                    chunks.push(std::mem::take(&mut current));
                }
                let mut piece = String::new();
                let mut piece_chars = 0;
                for ch in paragraph.chars() {
                    piece.push(ch);
                    piece_chars += 1;
                    if piece_chars >= EPISODE_CHUNK_CHARS {
                        chunks.push(std::mem::take(&mut piece));
                        piece_chars = 0;
                    }
                }
                current = piece;
                current_chars = piece_chars;
                continue;
            }
            if !current.is_empty() && current_chars + paragraph_chars > EPISODE_CHUNK_CHARS {
                chunks.push(std::mem::take(&mut current));
                current_chars = 0;
            }
            current.push_str(paragraph);
            current_chars += paragraph_chars;
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }

    #[test]
    fn version_one_cuts_every_body_as_it_shipped() {
        let mut bodies: Vec<String> = (0..300).map(|seed| generate(seed).body).collect();
        bodies.push(format!("{}\n\n{}", "é".repeat(6_998), "한".repeat(14_000)));
        bodies.push(format!(
            "{}\n\n{}\n\nx",
            "a".repeat(3_000),
            "b".repeat(4_001)
        ));
        for (seed, body) in bodies.iter().enumerate() {
            let bodies: Vec<_> = chunk_episode(body, CHUNKER_PARAGRAPHS)
                .unwrap()
                .into_iter()
                .map(|chunk| chunk.body)
                .collect();
            assert_eq!(bodies, shipped_version_one(body), "seed {seed}");
        }
    }

    #[test]
    fn version_one_keeps_its_paragraph_and_character_cut() {
        for body in [
            "한".repeat(EPISODE_CHUNK_CHARS * 3 + 19),
            format!(
                "{}\n\n{}\n\n{}",
                "é".repeat(6_998),
                "한".repeat(7_001),
                "tail"
            ),
            "x".repeat(1_000_000),
        ] {
            let chunks = chunk_episode(&body, CHUNKER_PARAGRAPHS).unwrap();
            exact(&body, &chunks, 0);
            assert!(chunks.iter().all(|chunk| chunk.context_chars == 0));
        }
        let chunks = chunk_episode(&format!("{}\n\ntail", "한".repeat(6_998)), 1).unwrap();
        let bodies: Vec<_> = chunks.into_iter().map(|chunk| chunk.body).collect();
        assert_eq!(
            bodies,
            vec![format!("{}\n\n", "한".repeat(6_998)), "tail".into()]
        );
    }
}
