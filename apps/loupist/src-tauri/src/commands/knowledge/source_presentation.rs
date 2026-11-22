mod citations;
pub(super) use citations::{for_capture, CitationScope};
mod data;
mod preview;
pub(super) use preview::prose as catalog_preview;

use serde::{Deserialize, Serialize};

use crate::datasets::saved_rows::SavedRows;
use crate::knowledge::{RepresentationRole, SourceClass};

/// End-exclusive UTF-8 offsets into the original presentation text.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CitedPassage {
    pub start: usize,
    pub end: usize,
    pub marks: Vec<CitedPassageMark>,
}

/// End-exclusive UTF-8 offsets into the original presentation text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CitedPassageMark {
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub evidence_id: Option<String>,
}

/// A renderer-only projection of a field in the immutable delivered capture.
/// This is not a replacement span and never grants citation authority.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SourcePresentationDto {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub table: Option<data::SourceDataTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cited_passages: Option<Vec<CitedPassage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub focused_mark: Option<crate::domain::ChatEvidenceMatch>,
    pub source_pointer: &'static str,
    pub extraction_method: String,
    pub content_offset: usize,
    pub next_offset: Option<usize>,
    pub extraction_truncated: bool,
}

#[derive(Deserialize)]
struct WebEnvelope {
    content_class: String,
    untrusted_content: bool,
    content: String,
    content_bytes: usize,
    content_offset: usize,
    next_offset: Option<usize>,
    extraction_method: String,
    extraction_truncated: bool,
}

pub(super) fn source_presentation(
    source_class: SourceClass,
    role: RepresentationRole,
    media_type: &str,
    exact_text: &str,
) -> Option<SourcePresentationDto> {
    if source_class != SourceClass::ToolResult
        || !matches!(
            role,
            RepresentationRole::ProviderPayload
                | RepresentationRole::Excerpt
                | RepresentationRole::Structured
        )
        || media_type != "application/json"
    {
        return None;
    }
    if let Ok(record) = serde_json::from_str(exact_text) {
        if let Some(table) = SavedRows::parse(&record).and_then(|rows| data::table(&rows, None)) {
            return Some(SourcePresentationDto {
                text: exact_text.to_owned(),
                table: Some(table),
                cited_passages: None,
                focused_mark: None,
                source_pointer: "",
                extraction_method: "structured_data".into(),
                content_offset: 0,
                next_offset: None,
                extraction_truncated: false,
            });
        }
    }
    if role == RepresentationRole::Structured {
        return None;
    }
    let web: WebEnvelope = serde_json::from_str(exact_text).ok()?;
    if web.content_class != "untrusted_web"
        || !web.untrusted_content
        || web.content_bytes != web.content.len()
        || !matches!(
            web.extraction_method.as_str(),
            "direct_markdown" | "direct_text" | "readability" | "pdf" | "office" | "official_api"
        )
        || web
            .next_offset
            .is_some_and(|offset| web.content_offset.checked_add(web.content_bytes) != Some(offset))
    {
        return None;
    }
    Some(SourcePresentationDto {
        text: web.content,
        table: None,
        cited_passages: None,
        focused_mark: None,
        source_pointer: "/content",
        extraction_method: web.extraction_method,
        content_offset: web.content_offset,
        next_offset: web.next_offset,
        extraction_truncated: web.extraction_truncated,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod figure_tests;
