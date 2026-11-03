use serde::{Deserialize, Serialize};

use crate::knowledge::{RepresentationRole, SourceClass};

/// A renderer-only projection of a field in the immutable delivered capture.
/// This is not a replacement span and never grants citation authority.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePresentationDto {
    pub text: String,
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
            RepresentationRole::ProviderPayload | RepresentationRole::Excerpt
        )
        || media_type != "application/json"
    {
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
        source_pointer: "/content",
        extraction_method: web.extraction_method,
        content_offset: web.content_offset,
        next_offset: web.next_offset,
        extraction_truncated: web.extraction_truncated,
    })
}

#[cfg(test)]
mod tests;
