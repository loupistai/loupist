use super::*;
use crate::domain::{
    ChatEvidenceDelivery, ChatEvidenceDeliveryCoverage, ChatEvidenceKind, ChatEvidenceMatch,
    ChatEvidenceRef,
};

const MAX_EXCERPT: usize = crate::domain::MAX_CHAT_EVIDENCE_EXCERPT_BYTES;
const MAX_TOTAL_EXCERPTS: usize = crate::domain::MAX_CHAT_EVIDENCE_TOTAL_EXCERPT_BYTES;

pub(super) fn evidence_id(result_ref: &str) -> String {
    format!("ev_{}", &sha256(result_ref.as_bytes())[..32])
}

pub(super) fn citation_url(result_ref: &str) -> String {
    format!("#evidence-{}", evidence_id(result_ref))
}

/// The result a citation_url names, or a refusal that lists the URLs this turn
/// did return, so a model that guessed one can copy a real one instead of retrying.
pub(super) fn cited_result<'a>(
    registry: &'a RunResultRegistry,
    url: &str,
) -> Result<&'a RunResult, String> {
    if let Some(result) = registry
        .values()
        .find(|result| citation_url(&result.result_ref) == url)
    {
        return Ok(result);
    }
    let mut recent: Vec<&RunResult> = registry.values().collect();
    recent.sort_by(|a, b| b.retrieved_at.cmp(&a.retrieved_at));
    let returned: Vec<String> = recent
        .iter()
        .take(6)
        .map(|result| {
            format!(
                "{} ({})",
                citation_url(&result.result_ref),
                result.producer.tool_name
            )
        })
        .collect();
    Err(if returned.is_empty() {
        "not a citation_url from this turn; this turn has returned none, so read the source first or leave the input unsourced".into()
    } else {
        format!(
            "not a citation_url from this turn; copy one this turn returned: {}",
            returned.join(", ")
        )
    })
}

fn bounded(value: &str, maximum: usize) -> String {
    let mut end = value.len().min(maximum);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].replace('\0', "")
}

fn string_at(payload: &Value, pointers: &[&str], maximum: usize) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| bounded(value, maximum))
    })
}

fn saved_read_ref(result: &RunResult) -> Option<String> {
    if result.producer.origin != RunResultOrigin::SavedContext {
        return None;
    }
    let value = result.payload.pointer("/readRef")?.as_str()?.trim();
    (value.len() <= 256 && !value.is_empty() && !value.contains('\0')).then(|| value.to_string())
}

fn safe_url(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value).ok()?;
    let normalized = parsed.to_string();
    (matches!(parsed.scheme(), "https" | "http")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && normalized.len() <= 2048)
        .then_some(normalized)
}

pub(super) fn model_delivery(payload: &Value) -> ChatEvidenceDelivery {
    let text = serde_json::to_string_pretty(payload).unwrap_or_default();
    let excerpt = bounded(&text, MAX_EXCERPT);
    ChatEvidenceDelivery {
        coverage: ChatEvidenceDeliveryCoverage::BoundedSample,
        excerpt_truncated: text.len() > excerpt.len(),
        excerpt,
        content_sha256: sha256(&serde_json::to_vec(payload).unwrap_or_default()),
    }
}

fn excerpt_match(result: &RunResult, excerpt: &str) -> Option<ChatEvidenceMatch> {
    if result.producer.runtime_id != "native-web"
        || result.producer.tool_name != "web_fetch"
        || result.producer.origin != RunResultOrigin::AcquiredOriginal
    {
        return None;
    }
    let payload = &result.payload;
    let selection = payload.get("selection")?;
    if selection.get("type")?.as_str()? != "literal_passage"
        || selection.get("offset_unit")?.as_str()? != "utf8_bytes"
    {
        return None;
    }
    let parent = selection.get("source_result_ref")?.as_str()?;
    if !result
        .upstream_result_refs
        .iter()
        .any(|item| item == parent)
    {
        return None;
    }
    let literal = selection.get("find")?.as_str()?;
    if literal.trim().is_empty() || literal.contains('\0') || literal.chars().count() > 256 {
        return None;
    }
    let content = payload.get("content")?.as_str()?;
    // Projection may remove NULs or shorten content. Never guess offsets into
    // transformed text, and never highlight a match omitted from the excerpt.
    if !content.starts_with(excerpt) {
        return None;
    }
    let content_offset = usize::try_from(payload.get("content_offset")?.as_u64()?).ok()?;
    let match_offset = usize::try_from(selection.get("match_offset")?.as_u64()?).ok()?;
    let start = match_offset.checked_sub(content_offset)?;
    let end = start.checked_add(literal.len())?;
    (content.get(start..end) == Some(literal) && excerpt.get(start..end) == Some(literal))
        .then_some(ChatEvidenceMatch { start, end })
}

pub(super) fn project(result: &RunResult) -> ChatEvidenceRef {
    let payload = &result.payload;
    let title = string_at(
        payload,
        &["/title", "/name", "/path", "/metadata/title", "/data/title"],
        512,
    )
    .or_else(|| result.origin.as_deref().map(|origin| bounded(origin, 512)))
    .unwrap_or_else(|| result.producer.tool_name.replace('_', " "));
    let url = [
        "/final_url",
        "/url",
        "/provenance/url",
        "/metadata/url",
        "/source/url",
    ]
    .iter()
    .find_map(|pointer| {
        payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .and_then(safe_url)
    })
    .or_else(|| result.origin.as_deref().and_then(safe_url));
    let text = string_at(
        payload,
        &[
            "/content",
            "/text",
            "/markdown",
            "/document",
            "/data/content",
        ],
        MAX_EXCERPT + 4,
    )
    .unwrap_or_else(|| serde_json::to_string_pretty(payload).unwrap_or_default());
    let excerpt = bounded(&text, MAX_EXCERPT);
    let excerpt_truncated = text.len() > excerpt.len()
        || payload
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || payload
            .get("extraction_truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || payload
            .get("next_offset")
            .is_some_and(|value| !value.is_null())
        || payload
            .pointer("/model_delivery/coverage")
            .and_then(Value::as_str)
            == Some("bounded_sample");
    let kind = match result.producer.origin {
        RunResultOrigin::AcquiredOriginal => ChatEvidenceKind::Original,
        RunResultOrigin::WorkspaceFile => ChatEvidenceKind::LocalFile,
        RunResultOrigin::Discovery => ChatEvidenceKind::Discovery,
        RunResultOrigin::Derived => ChatEvidenceKind::Derived,
        RunResultOrigin::SavedContext => ChatEvidenceKind::SavedContext,
    };
    ChatEvidenceRef {
        id: evidence_id(&result.result_ref),
        title,
        kind,
        url,
        provider: result.producer.provider.clone(),
        tool_name: result.producer.tool_name.clone(),
        read_ref: saved_read_ref(result),
        retrieved_at: if result.producer.runtime_id == "native-web"
            && result.producer.tool_name == "web_fetch"
        {
            // Native web payloads carry the host's original network retrieval
            // time. Selecting a cached passage does not retrieve the source again.
            string_at(payload, &["/retrieved_at"], 128)
                .unwrap_or_else(|| result.retrieved_at.clone())
        } else {
            result.retrieved_at.clone()
        },
        published_at: string_at(payload, &["/published_at", "/provenance/published_at"], 128),
        as_of: string_at(
            payload,
            &[
                "/as_of/cutoff",
                "/as_of",
                "/provenance/as_of",
                "/metadata/as_of",
            ],
            128,
        ),
        coverage: string_at(
            payload,
            &["/coverage", "/content_class", "/model_delivery/coverage"],
            256,
        ),
        excerpt_match: excerpt_match(result, &excerpt),
        excerpt,
        excerpt_truncated,
        content_sha256: result.response_digest.clone(),
        warnings: result.warnings.clone(),
        dataset_ref: payload
            .get("dataset_ref")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
        model_delivery: result.model_delivery.clone(),
        upstream_evidence_ids: result
            .upstream_result_refs
            .iter()
            .map(|result_ref| evidence_id(result_ref))
            .collect(),
    }
}

impl ToolCapture {
    pub(super) async fn publish_evidence(
        &self,
        items: Vec<ChatEvidenceRef>,
        call_id: Option<String>,
    ) {
        let mut retained = self.evidence_refs.lock().await;
        let mut remaining = MAX_TOTAL_EXCERPTS.saturating_sub(
            retained
                .iter()
                .map(|item| {
                    item.excerpt.len()
                        + item
                            .model_delivery
                            .as_ref()
                            .map_or(0, |delivery| delivery.excerpt.len())
                })
                .sum::<usize>(),
        );
        for mut evidence in items {
            if retained.len() >= MAX_RUN_RESULTS
                || retained.iter().any(|item| item.id == evidence.id)
            {
                continue;
            }
            if evidence.excerpt.len() > remaining {
                evidence.excerpt = bounded(&evidence.excerpt, remaining);
                evidence.excerpt_truncated = true;
                if evidence
                    .excerpt_match
                    .as_ref()
                    .is_some_and(|matched| !matched.is_valid_for(&evidence.excerpt))
                {
                    evidence.excerpt_match = None;
                }
            }
            remaining = remaining.saturating_sub(evidence.excerpt.len());
            if let Some(delivery) = &mut evidence.model_delivery {
                if delivery.excerpt.len() > remaining {
                    delivery.excerpt = bounded(&delivery.excerpt, remaining);
                    delivery.excerpt_truncated = true;
                }
                remaining = remaining.saturating_sub(delivery.excerpt.len());
            }
            retained.push(evidence.clone());
            if let Some(sink) = &self.evidence_sink {
                sink(
                    evidence,
                    call_id.clone().or_else(super::current_tool_call_id),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(origin: RunResultOrigin, payload: Value) -> RunResult {
        RunResult {
            result_ref: "result:test".into(),
            producer: RunResultProducer {
                origin,
                runtime_id: "native-web".into(),
                tool_name: "web_fetch".into(),
                provider: None,
            },
            origin: Some("https://issuer.example/report".into()),
            request_digest: "a".repeat(64),
            response_digest: "b".repeat(64),
            retrieved_at: "2026-10-11T00:00:00Z".into(),
            payload,
            model_delivery: None,
            warnings: vec![],
            upstream_result_refs: vec![],
        }
    }

    #[test]
    fn evidence_is_a_bounded_snapshot_with_safe_origin_and_honest_kind() {
        let result = source(
            RunResultOrigin::AcquiredOriginal,
            json!({"title":"Annual report", "content":"매출".repeat(6000), "url":"https://user:secret@bad.example/", "retrieved_at":"2025-11-02T00:00:00Z", "next_offset":16000}),
        );
        let evidence = project(&result);
        assert_eq!(evidence.kind, ChatEvidenceKind::Original);
        assert_eq!(
            evidence.url.as_deref(),
            Some("https://issuer.example/report")
        );
        assert!(evidence.excerpt.len() <= MAX_EXCERPT);
        assert!(evidence.excerpt_truncated);
        assert_eq!(evidence.content_sha256, result.response_digest);
        for (origin, kind) in [
            (RunResultOrigin::Discovery, ChatEvidenceKind::Discovery),
            (RunResultOrigin::Derived, ChatEvidenceKind::Derived),
        ] {
            assert_eq!(project(&source(origin, json!({"data": 1}))).kind, kind);
        }
        // Normalization can lengthen a URL past what a receipt may persist.
        let raw = format!("https://example.com/{}", "한".repeat(500));
        assert!(raw.len() < 2048);
        assert!(safe_url(&raw).is_none());
        // Only saved context may name a read receipt.
        let saved = json!({"readRef": "rdg_journal", "text": "Capacity review"});
        assert_eq!(
            project(&source(RunResultOrigin::SavedContext, saved.clone()))
                .read_ref
                .as_deref(),
            Some("rdg_journal")
        );
        assert!(project(&source(RunResultOrigin::AcquiredOriginal, saved))
            .read_ref
            .is_none());
    }

    #[test]
    fn evidence_highlights_only_an_exact_host_selected_passage_inside_the_excerpt() {
        let prefix = "앞🙂 ";
        let mut result = source(
            RunResultOrigin::AcquiredOriginal,
            json!({
                "content": format!("{prefix}영업이익: 12.166조 원"),
                "content_offset": 9000,
                "selection": {
                    "type": "literal_passage", "offset_unit": "utf8_bytes",
                    "source_result_ref": "result:original", "find": "영업이익:",
                    "match_offset": 9000 + prefix.len()
                }
            }),
        );
        result.upstream_result_refs = vec!["result:original".into()];
        let evidence = project(&result);
        let matched = evidence.excerpt_match.unwrap();
        assert_eq!(&evidence.excerpt[matched.start..matched.end], "영업이익:");

        let mut provider_claimed = result.clone();
        provider_claimed.producer.runtime_id = "mcp:example".into();
        let mut derived = result.clone();
        derived.producer.origin = RunResultOrigin::Derived;
        let mut misplaced = result.clone();
        misplaced.payload["selection"]["match_offset"] = json!(9001);
        let mut outside = result.clone();
        outside.payload["content"] = json!(format!("{}영업이익:", "x".repeat(MAX_EXCERPT)));
        outside.payload["selection"]["match_offset"] = json!(9000 + MAX_EXCERPT);
        for unsupported in [provider_claimed, derived, misplaced, outside] {
            assert!(project(&unsupported).excerpt_match.is_none());
        }
    }

    #[test]
    fn evidence_distinguishes_retained_result_from_delivered_sample() {
        let mut result = source(
            RunResultOrigin::AcquiredOriginal,
            json!({"data":[1,2,3,4,5]}),
        );
        let sample = json!({"data_summary":{"sample":{"data":[1,2,3]}}});
        result.model_delivery = Some(model_delivery(&sample));
        let evidence = project(&result);
        assert!(evidence.excerpt.contains('5'));
        let delivery = evidence.model_delivery.unwrap();
        assert!(!delivery.excerpt.contains('5'));
        assert_eq!(
            delivery.coverage,
            ChatEvidenceDeliveryCoverage::BoundedSample
        );
        assert_ne!(delivery.content_sha256, evidence.content_sha256);
    }

    #[tokio::test]
    async fn evidence_is_published_only_for_acknowledged_deliveries() {
        let capture = ToolCapture::default();
        let producer = source(RunResultOrigin::AcquiredOriginal, json!({})).producer;
        let request = json!({"url":"https://issuer.example/report"});
        let mut refs = Vec::new();
        for delivery in ["delivered", "discarded"] {
            refs.push(
                capture
                    .stage_run_result_with_delivery(
                        delivery,
                        producer.clone(),
                        &request,
                        json!({"content":"Revenue: 25", "title":"Filing"}),
                        vec![],
                        None,
                    )
                    .await
                    .unwrap(),
            );
        }
        assert!(capture.run_result(&refs[0]).await.is_none());
        assert!(capture.evidence_refs.lock().await.is_empty());
        capture.commit_delivery("delivered").await;
        capture.discard_delivery("discarded").await;
        assert!(capture.run_result(&refs[0]).await.is_some());
        assert!(capture.run_result(&refs[1]).await.is_none());
        let evidence = capture.evidence_refs.lock().await.clone();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].id, evidence_id(&refs[0]));
        assert_eq!(evidence[0].excerpt, "Revenue: 25");
    }

    #[tokio::test]
    async fn evidence_total_excerpt_budget_preserves_all_receipts() {
        let capture = ToolCapture::default();
        let items = (0..MAX_RUN_RESULTS)
            .map(|index| {
                let mut result = source(
                    RunResultOrigin::Derived,
                    json!({"content": "x".repeat(MAX_EXCERPT)}),
                );
                result.result_ref = format!("result:{index}");
                result.model_delivery =
                    Some(model_delivery(&json!({"sample":"y".repeat(MAX_EXCERPT)})));
                project(&result)
            })
            .collect();
        capture.publish_evidence(items, None).await;
        let retained = capture.evidence_refs.lock().await;
        assert_eq!(retained.len(), MAX_RUN_RESULTS);
        assert_eq!(
            retained
                .iter()
                .map(|item| item.excerpt.len()
                    + item
                        .model_delivery
                        .as_ref()
                        .map_or(0, |delivery| delivery.excerpt.len()))
                .sum::<usize>(),
            MAX_TOTAL_EXCERPTS
        );
    }
}
