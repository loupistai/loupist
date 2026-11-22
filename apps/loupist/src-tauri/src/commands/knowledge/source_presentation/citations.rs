use super::super::{KnowledgeSourceReadRequest, SourceFigureFailure, SourceFigureRequest};
use super::{CitedPassage, CitedPassageMark, SourcePresentationDto};
use crate::domain::{
    ChatEvidenceKind, ChatEvidenceMatch, ChatEvidenceRef, ChatMessage, ChatMessageStatus, ChatRole,
    ChatSession, MemoryAccess, NumberCheck, NumberCheckStatus,
};
use crate::knowledge::{KnowledgeResult, PublishedCapture, PublishedSpan};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::{hash_map::Entry, BTreeSet, HashMap};
use std::ops::Range;

const MAX_ANSWERS: usize = 32;
const MAX_MARKS: usize = 256;

pub(in crate::commands::knowledge) struct CitationScope<'a> {
    pub request: &'a KnowledgeSourceReadRequest,
    pub source_id: &'a str,
    pub capture: &'a PublishedCapture,
    pub span: &'a PublishedSpan,
    pub revision_created_at: i64,
    pub exact_text: &'a str,
}

#[derive(PartialEq, Eq)]
struct AnswerId {
    thread: String,
    message: String,
}

impl From<&SourceFigureRequest> for AnswerId {
    fn from(figure: &SourceFigureRequest) -> Self {
        Self {
            thread: figure.thread_id.clone(),
            message: figure.message_id.clone(),
        }
    }
}

struct OriginalRead {
    answer: AnswerId,
    evidence_id: Option<String>,
}

impl CitationScope<'_> {
    fn original_read(&self, tx: &Transaction<'_>) -> KnowledgeResult<Option<OriginalRead>> {
        // A retained original read belongs to the revision created by that save.
        if self.revision_created_at != self.capture.received_at_ms {
            return Ok(None);
        }
        let original = tx
            .query_row(
                "SELECT lineage.chat_id, g.origin_message_id, lineage.request_ref
             FROM knowledge_capture_lineage lineage
             JOIN knowledge_growth_events g ON g.record_id=lineage.record_id
                 AND g.origin_chat_id=lineage.chat_id
             WHERE g.loupist_id=?1 AND lineage.record_id=?2 AND lineage.capture_id=?3
                 AND g.at_ms=?4 AND g.kind='source_retained'
                 AND g.origin_message_id IS NOT NULL",
                params![
                    self.request.loupist_id,
                    self.request.record_id,
                    self.capture.capture_id,
                    self.capture.received_at_ms,
                ],
                |row| {
                    Ok(OriginalRead {
                        answer: AnswerId {
                            thread: row.get(0)?,
                            message: row.get(1)?,
                        },
                        evidence_id: row
                            .get::<_, Option<String>>(2)?
                            .as_deref()
                            .map(crate::commands::tool_executor::evidence_id),
                    })
                },
            )
            .optional()?;
        Ok(original)
    }

    fn recent_answers(
        &self,
        tx: &Transaction<'_>,
        original: Option<&OriginalRead>,
    ) -> KnowledgeResult<Vec<AnswerId>> {
        // Scan saved answer metadata in SQLite. Only the newest candidates are
        // decoded into Chat sessions, and the requested answer is read by ID.
        let mut query = tx.prepare(
            "WITH chats AS (
                SELECT id, CASE WHEN json_valid(data_json) THEN data_json ELSE '{}' END AS body
                FROM chat_sessions WHERE loupist_id=?1 AND (id=?2 OR instr(data_json, ?4) > 0)
             ), messages AS (
                SELECT c.id AS thread, m.value AS body FROM chats c
                JOIN json_each(c.body, '$.messages') m
                WHERE m.type='object'
                UNION ALL
                SELECT c.id, m.value FROM chats c
                JOIN json_each(c.body, '$.versions') v
                JOIN json_each(CASE WHEN v.type='object' THEN v.value END, '$.messages') m
                WHERE m.type='object'
             )
             SELECT thread, body ->> 'id' AS id
             FROM messages
             WHERE body ->> 'role'='assistant' AND body ->> 'status'='complete'
               AND json_type(body, '$.id')='text' AND json_type(body, '$.createdAtMs')='integer'
               AND EXISTS (
                 SELECT 1 FROM json_each(body, '$.numberChecks') n
                 JOIN json_each(body, '$.evidenceRefs') e
                   ON json_extract(body, n.fullkey || '.evidence_id') = json_extract(body, e.fullkey || '.id')
                 WHERE json_extract(body, n.fullkey || '.status') IN ('matched','rounded')
                   AND ((thread=?2 AND body ->> 'id'=?3
                         AND json_extract(body, e.fullkey || '.tool_name')='web_fetch'
                         AND json_extract(body, e.fullkey || '.content_sha256')=?8)
                     OR (json_extract(body, e.fullkey || '.tool_name')='world_read' AND EXISTS (
                       SELECT 1 FROM json_each(body, '$.memoryRefs') m
                       WHERE json_extract(body, m.fullkey || '.readRef')=json_extract(body, e.fullkey || '.read_ref')
                         AND json_extract(body, m.fullkey || '.kind')='source'
                         AND json_extract(body, m.fullkey || '.access')='exact_read'
                         AND json_extract(body, m.fullkey || '.recordId')=?4
                         AND json_extract(body, m.fullkey || '.revisionId')=?5
                         AND json_extract(body, m.fullkey || '.objectId')=?6
                         AND json_extract(body, m.fullkey || '.spanId')=?7
                     )))
               )
             GROUP BY thread, id
             ORDER BY max(body ->> 'createdAtMs') DESC, thread, id LIMIT ?9",
        )?;
        let answers = query.query_map(
            params![
                self.request.loupist_id,
                original.map(|read| read.answer.thread.as_str()),
                original.map(|read| read.answer.message.as_str()),
                self.request.record_id,
                self.request.revision_id,
                self.source_id,
                self.span.span_id,
                self.span.selected_digest,
                MAX_ANSWERS as i64,
            ],
            |row| {
                Ok(AnswerId {
                    thread: row.get(0)?,
                    message: row.get(1)?,
                })
            },
        )?;
        Ok(answers.collect::<Result<_, _>>()?)
    }

    fn admits_evidence(
        &self,
        original: Option<&OriginalRead>,
        id: &AnswerId,
        answer: &ChatMessage,
        evidence: &ChatEvidenceRef,
        text: &str,
    ) -> bool {
        match (evidence.tool_name.as_str(), evidence.kind) {
            ("world_read", ChatEvidenceKind::SavedContext) => {
                !evidence.excerpt.is_empty()
                    && self.exact_text.starts_with(&evidence.excerpt)
                    && evidence
                        .read_ref
                        .as_deref()
                        .filter(|read_ref| !read_ref.is_empty())
                        .is_some_and(|read_ref| {
                            answer.memory_refs.iter().any(|read| {
                                read.read_ref == read_ref
                                    && read.receipt_id.as_deref() == Some(read_ref)
                                    && read.access == MemoryAccess::ExactRead
                                    && read.kind == "source"
                                    && read.record_id == self.request.record_id
                                    && read.revision_id == self.request.revision_id
                                    && read.object_id == self.source_id
                                    && read.span_id.as_deref() == Some(self.span.span_id.as_str())
                            })
                        })
            }
            ("web_fetch", ChatEvidenceKind::Original) => {
                evidence.content_sha256 == self.span.selected_digest
                    && text.starts_with(&evidence.excerpt)
                    && original.is_some_and(|read| {
                        read.answer == *id
                            && read.evidence_id.as_ref().map_or_else(
                                || {
                                    answer
                                        .evidence_refs
                                        .iter()
                                        .filter(|item| {
                                            item.tool_name == "web_fetch"
                                                && item.kind == ChatEvidenceKind::Original
                                                && item.content_sha256 == self.span.selected_digest
                                        })
                                        .count()
                                        == 1
                                },
                                |id| id == &evidence.id,
                            )
                    })
                    && self.original_span(answer)
            }
            _ => false,
        }
    }

    fn original_span(&self, answer: &ChatMessage) -> bool {
        let mut targets = answer
            .memory_update
            .iter()
            .flat_map(|update| {
                update.changes.iter().filter_map(|change| {
                    change.knowledge.as_ref().filter(|target| {
                        target.kind == "source" && target.record_id == self.request.record_id
                    })
                })
            })
            .peekable();
        // Older answers might only have the retention event and exact digest.
        targets.peek().is_none()
            || targets.any(|target| {
                target.revision_id == self.request.revision_id
                    && target.object_id == self.source_id
                    && target.span_id.as_deref() == Some(self.span.span_id.as_str())
            })
    }
}

#[derive(Default)]
struct ReadableChats(HashMap<String, Option<ChatSession>>);

impl ReadableChats {
    fn answer<'a>(
        &'a mut self,
        tx: &Transaction<'_>,
        loupist: &str,
        id: &AnswerId,
    ) -> KnowledgeResult<Option<&'a ChatMessage>> {
        let chat = match self.0.entry(id.thread.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let data: Option<String> = tx
                    .query_row(
                        "SELECT data_json FROM chat_sessions WHERE id=?1 AND loupist_id=?2",
                        params![id.thread, loupist],
                        |row| row.get(0),
                    )
                    .optional()?;
                entry.insert(
                    data.and_then(|data| serde_json::from_str::<ChatSession>(&data).ok())
                        .filter(|chat| chat.loupist_id == loupist && chat.id == id.thread),
                )
            }
        };
        Ok(chat.as_ref().and_then(|chat| {
            chat.messages
                .iter()
                .chain(chat.versions.iter().flat_map(|version| &version.messages))
                .find(|message| message.id == id.message)
                .filter(|message| {
                    message.role == ChatRole::Assistant
                        && message.status == ChatMessageStatus::Complete
                })
        }))
    }
}

fn verified_mark(
    evidence: &ChatEvidenceRef,
    check: &NumberCheck,
    text: &str,
) -> Option<ChatEvidenceMatch> {
    if !matches!(
        check.status,
        NumberCheckStatus::Matched | NumberCheckStatus::Rounded
    ) {
        return None;
    }
    if let Some(span) = &check.span {
        if text.starts_with(&evidence.excerpt) && span.is_valid_for(&evidence.excerpt) {
            return Some(span.clone());
        }
    }
    let ctx = check.context.as_ref()?;
    if ctx.start >= ctx.end || ctx.text.get(ctx.start..ctx.end).is_none() {
        return None;
    }
    // The context from a saved web read is in the decoded content, while
    // its evidence excerpt contains the serialized web envelope.
    let start = text.find(&ctx.text)?;
    let next = start + ctx.text.chars().next()?.len_utf8();
    (!text[next..].contains(&ctx.text)).then_some(ChatEvidenceMatch {
        start: start + ctx.start,
        end: start + ctx.end,
    })
}

fn focused(
    scope: &CitationScope<'_>,
    original: Option<&OriginalRead>,
    id: &AnswerId,
    answer: Option<&ChatMessage>,
    figure: &SourceFigureRequest,
    text: &str,
) -> Result<ChatEvidenceMatch, SourceFigureFailure> {
    let answer = answer.ok_or(SourceFigureFailure::AnswerUnavailable)?;
    let evidence = answer
        .evidence_refs
        .iter()
        .find(|evidence| {
            evidence.id == figure.evidence_id
                && scope.admits_evidence(original, id, answer, evidence, text)
        })
        .ok_or(SourceFigureFailure::EvidenceMismatch)?;
    answer
        .number_checks
        .iter()
        .find(|check| check.link_index == figure.link_index && check.evidence_id == evidence.id)
        .and_then(|check| verified_mark(evidence, check, text))
        .ok_or(SourceFigureFailure::MarkUnavailable)
}

fn focus_data_table(
    tx: &Transaction<'_>,
    scope: &CitationScope<'_>,
    view: &mut SourcePresentationDto,
    figure: &SourceFigureRequest,
) -> KnowledgeResult<Option<SourceFigureFailure>> {
    use crate::commands::tool_executor::number_checks::SavedFigureChecks;
    use crate::datasets::saved_rows::SavedRows;
    let id = AnswerId::from(figure);
    let mut chats = ReadableChats::default();
    let Some(answer) = chats.answer(tx, &scope.request.loupist_id, &id)? else {
        return Ok(Some(SourceFigureFailure::AnswerUnavailable));
    };
    let Some(evidence) = answer.evidence_refs.iter().find(|evidence| {
        evidence.id == figure.evidence_id
            && scope.admits_evidence(None, &id, answer, evidence, &view.text)
    }) else {
        return Ok(Some(SourceFigureFailure::EvidenceMismatch));
    };
    let record = serde_json::from_str(scope.exact_text).ok();
    let table = record.as_ref().and_then(SavedRows::parse).and_then(|rows| {
        let checks = SavedFigureChecks::new(&rows);
        let pointer = checks.pointer(&answer.content, figure.link_index, &evidence.id)?;
        super::data::table(&rows, Some(pointer))
    });
    if let Some(table) = table.filter(|table| table.focused_cell.is_some()) {
        view.table = Some(table);
        Ok(None)
    } else {
        Ok(Some(SourceFigureFailure::MarkUnavailable))
    }
}

/// Builds display ranges exclusively from checked answer marks that resolve
/// inside the exact retained text. Inspection itself grants no citation authority.
pub(in crate::commands::knowledge) fn for_capture(
    tx: &Transaction<'_>,
    scope: CitationScope<'_>,
    view: &mut SourcePresentationDto,
) -> KnowledgeResult<Option<SourceFigureFailure>> {
    if view.table.is_some() {
        return match &scope.request.figure {
            Some(figure) => focus_data_table(tx, &scope, view, figure),
            None => Ok(None),
        };
    }
    let mut chats = ReadableChats::default();
    let original = scope.original_read(tx)?;
    let mut recent = scope.recent_answers(tx, original.as_ref())?;
    let mut marks = BTreeSet::new();
    let mut anchors = Vec::new();
    let mut failure = None;
    if let Some(figure) = &scope.request.figure {
        let id = AnswerId::from(figure);
        let answer = chats.answer(tx, &scope.request.loupist_id, &id)?;
        match focused(&scope, original.as_ref(), &id, answer, figure, &view.text) {
            Ok(mark) => {
                marks.insert((mark.start, mark.end, figure.evidence_id.clone()));
                view.focused_mark = Some(mark);
                if !recent.contains(&id) {
                    recent.truncate(MAX_ANSWERS - 1);
                    recent.push(id);
                }
            }
            Err(reason) => failure = Some(reason),
        }
    }
    for id in recent {
        let Some(answer) = chats.answer(tx, &scope.request.loupist_id, &id)? else {
            continue;
        };
        for evidence in answer.evidence_refs.iter().filter(|evidence| {
            scope.admits_evidence(original.as_ref(), &id, answer, evidence, &view.text)
        }) {
            let mut has_mark = false;
            for check in answer
                .number_checks
                .iter()
                .filter(|check| check.evidence_id == evidence.id)
            {
                if let Some(mark) = verified_mark(evidence, check, &view.text) {
                    let mark = (mark.start, mark.end, evidence.id.clone());
                    if marks.len() < MAX_MARKS {
                        marks.insert(mark);
                        has_mark = true;
                    } else if marks.contains(&mark) {
                        has_mark = true;
                    }
                }
            }
            if has_mark && view.text.starts_with(&evidence.excerpt) {
                anchors.extend(
                    evidence
                        .excerpt_match
                        .iter()
                        .filter(|anchor| anchor.is_valid_for(&evidence.excerpt))
                        .cloned(),
                );
            }
        }
    }
    if !marks.is_empty() {
        let marks = marks
            .into_iter()
            .map(|(start, end, evidence_id)| CitedPassageMark {
                start,
                end,
                evidence_id: Some(evidence_id),
            })
            .collect();
        view.cited_passages = Some(passages(&view.text, marks, &anchors));
    }
    Ok(failure)
}
fn is_pipe_row(line: &str) -> bool {
    line.len() > 1 && line.starts_with('|') && line.ends_with('|') && !line.contains('\n')
}

// Include a whole row or sentence. Long unbroken paragraphs are bounded at
// nearby whitespace so the visible passage still centers on the citation.
fn context(text: &str, mark: &Range<usize>) -> Range<usize> {
    let left = &text[..mark.start];
    let right = &text[mark.end..];
    let line_start = left.rfind('\n').map_or(0, |i| i + 1);
    let line_end = mark.end + right.find('\n').unwrap_or(right.len());
    let line = &text[line_start..line_end];
    if is_pipe_row(line.trim()) {
        let start = line_start + line.len() - line.trim_start().len();
        let end = line_start + line.trim_end().len();
        // Verified marks stay inside the range even if they include whitespace.
        return start.min(mark.start)..end.max(mark.end);
    }
    let before = &text[line_start..mark.start];
    let after = &text[mark.end..line_end];
    let sentence_start = before
        .rmatch_indices(['.', '?', '!', '。', '？', '！'])
        .find(|(i, c)| before[i + c.len()..].starts_with(char::is_whitespace) || c.len() > 1)
        .map(|(i, c)| i + c.len());
    let sentence_end = after
        .match_indices(['.', '?', '!', '。', '？', '！'])
        .find(|(i, c)| after[i + c.len()..].starts_with(char::is_whitespace) || c.len() > 1)
        .map(|(i, c)| i + c.len());
    let start = sentence_start.unwrap_or(0).max(
        before
            .char_indices()
            .rev()
            .skip(180)
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(i, c)| i + c.len_utf8()),
    );
    let end = sentence_end.unwrap_or(after.len()).min(
        after
            .char_indices()
            .skip(180)
            .find(|(_, c)| c.is_whitespace())
            .map_or(after.len(), |(i, _)| i),
    );
    let start = line_start + start;
    let end = mark.end + end;
    start + (text[start..mark.start].len() - text[start..mark.start].trim_start().len())
        ..mark.end + text[mark.end..end].trim_end().len()
}

const HEADING_LOOKBACK_BYTES: usize = 1024;

fn preceding_heading(text: &str, row: &Range<usize>, mark_start: usize) -> Option<usize> {
    let row_start = text[..row.start].rfind('\n')? + 1;
    let limit = text.ceil_char_boundary(mark_start.saturating_sub(HEADING_LOOKBACK_BYTES));
    let preceding = text.get(limit..row_start - 1)?;
    let table = is_pipe_row(text[row.clone()].trim());
    let mut start = row_start;
    let mut table_start = None;
    let mut expect_header = false;
    for line in preceding.rsplit('\n') {
        start -= line.len() + 1;
        if start == limit && start > 0 && text.as_bytes()[start - 1] != b'\n' {
            break;
        }
        let at = start + line.len() - line.trim_start().len();
        let line = line.trim();
        if is_pipe_row(line) {
            if !table {
                break;
            }
            if expect_header {
                return Some(at);
            }
            expect_header = line.contains('-')
                && line
                    .chars()
                    .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'));
            if !expect_header {
                table_start = Some(at);
            }
        } else {
            if table_start.is_some() || expect_header {
                break;
            }
            // Flattened tables can contain blank lines and fragmented rows
            // between the caption and the verified number.
            if line.ends_with([':', '：'])
                || (line.starts_with('#') && line.trim_start_matches('#').starts_with(' '))
            {
                return Some(at);
            }
            if line.ends_with(['.', '?', '!', '。', '？', '！']) || (table && !line.is_empty()) {
                break;
            }
        }
    }
    table_start
}

fn passages(
    text: &str,
    marks: Vec<CitedPassageMark>,
    anchors: &[ChatEvidenceMatch],
) -> Vec<CitedPassage> {
    let mut ranges = Vec::new();
    for mark in &marks {
        let mut row = context(text, &(mark.start..mark.end));
        let heading = preceding_heading(text, &row, mark.start).or_else(|| {
            // A verified literal lookup can supply context for a heading
            // without recognizable formatting. It never opens its own passage.
            let limit = mark.start.saturating_sub(HEADING_LOOKBACK_BYTES);
            let anchor = anchors
                .iter()
                .filter(|anchor| anchor.start >= limit && anchor.end <= row.start)
                .max_by_key(|anchor| anchor.end)?;
            let start = context(text, &(anchor.start..anchor.end)).start;
            (start >= limit).then_some(start)
        });
        if let Some(heading) = heading {
            row.start = heading;
        }
        ranges.push(row);
    }
    ranges.sort_by_key(|r| r.start);
    let mut joined: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        if let Some(last) = joined.last_mut() {
            if r.start <= last.end {
                last.end = last.end.max(r.end);
                continue;
            }
        }
        joined.push(r);
    }
    joined
        .into_iter()
        .map(|r| CitedPassage {
            start: r.start,
            end: r.end,
            marks: marks
                .iter()
                .filter(|m| r.start <= m.start && m.end <= r.end)
                .cloned()
                .collect(),
        })
        .collect()
}
