use std::collections::BTreeSet;

use super::arguments::bounded_text;
use super::*;

/// A web read that never reached its host becomes the typed `offline`
/// category, naming that host.
fn web_error(error: crate::web::WebError) -> BrokerError {
    match error {
        crate::web::WebError::Offline(host) => BrokerError::Offline(host),
        error => BrokerError::Execution(error.to_string()),
    }
}

struct LoupistSearchBackend<'a> {
    state: &'a crate::app::AppState,
}

impl crate::web::SearchBackend for LoupistSearchBackend<'_> {
    async fn attempt(
        &self,
        provider: crate::web::SearchProviderId,
        query: &crate::web::WebSearchQuery,
        remaining: std::time::Duration,
        cancel: &crate::web::SearchCancel,
    ) -> Result<crate::web::SearchHits, crate::web::WebError> {
        match provider {
            crate::web::SearchProviderId::ExaPublic => crate::web::search_exa_once(query).await,
            _ => {
                crate::providers::connection::run_native_web_search(
                    self.state, provider, query, remaining, cancel,
                )
                .await
            }
        }
    }
}

impl AppToolExecutor {
    pub(super) async fn web_search(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.require_capability("community.web-research")?;
        let cutoff = effective_as_of(policy, &params)?;
        let object = exact_object(
            &params,
            &["query"],
            &[
                "limit",
                "as_of",
                "recency",
                "include_domains",
                "exclude_domains",
            ],
        )?;
        let query = object
            .get("query")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.trim().is_empty()
                    && value.len() <= 4_096
                    && !value.contains('\0')
                    && !value
                        .chars()
                        .any(|character| character.is_control() && !character.is_whitespace())
            })
            .ok_or(BrokerError::Malformed)?;
        let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(5);
        if !(1..=10).contains(&limit) {
            return Err(BrokerError::Malformed);
        }
        let recency = object
            .get("recency")
            .and_then(Value::as_str)
            .map(|value| crate::web::WebRecency::parse(value).ok_or(BrokerError::Malformed))
            .transpose()?;
        let include_domains = parse_search_domains(object.get("include_domains"))?;
        let exclude_domains = parse_search_domains(object.get("exclude_domains"))?;
        let (mut output, mut sources) = crate::web::execute_search(
            crate::web::SearchRequest {
                query: crate::web::WebSearchQuery {
                    query: query.trim().to_owned(),
                    limit: limit as u8,
                    recency,
                    include_domains,
                    exclude_domains,
                },
                policy: self.capture.web_search_policy,
                chat_provider: self.capture.chat_provider(),
                cancel: self.capture.search_cancel.clone(),
            },
            &LoupistSearchBackend { state: &self.state },
        )
        .await
        .map_err(web_error)?;
        let as_of = cutoff.map(|cutoff| apply_web_search_cutoff(&mut output, &mut sources, cutoff));
        self.capture
            .stage(delivery_id, |pending| {
                for source in sources {
                    pending.web_sources.insert(source.source_id.clone(), source);
                }
            })
            .await;
        let mut value = serde_json::to_value(output)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        if let Some(as_of) = as_of {
            if let Some(object) = value.as_object_mut() {
                object.insert("as_of".into(), as_of);
            }
        }
        Ok(value)
    }

    pub(super) async fn web_fetch(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.require_capability("community.web-research")?;
        let cutoff = effective_as_of(policy, &params)?;
        let object = exact_object(
            &params,
            &[],
            &[
                "source_id",
                "url",
                "as_of",
                "offset",
                "result_ref",
                "find",
                "occurrence",
            ],
        )?;
        if ["result_ref", "find", "occurrence"]
            .iter()
            .any(|key| object.contains_key(*key))
        {
            if ["source_id", "url", "offset"]
                .iter()
                .any(|key| object.contains_key(*key))
            {
                return Err(BrokerError::Execution("For a passage use only result_ref, find, optional occurrence and as_of. Use source_id or url with an issued offset for ordinary pagination.".into()));
            }
            let result_ref = bounded_text(object.get("result_ref"), 1, 128)?;
            let literal = object
                .get("find")
                .and_then(Value::as_str)
                .filter(|text| {
                    !text.trim().is_empty() && text.chars().count() <= 256 && !text.contains('\0')
                })
                .ok_or(BrokerError::Malformed)?;
            let occurrence = object
                .get("occurrence")
                .map(|value| {
                    value
                        .as_u64()
                        .filter(|value| *value <= 1000)
                        .map(|value| value as usize)
                        .ok_or(BrokerError::Malformed)
                })
                .transpose()?
                .unwrap_or(0);
            return self
                .web_fetch_passage(&result_ref, literal, occurrence, cutoff)
                .await;
        }
        let source_id = object
            .get("source_id")
            .and_then(Value::as_str)
            .filter(|value| value.starts_with("web:") && value.len() <= 128);
        let url = object
            .get("url")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && value.len() <= 2_048);
        let content_offset = object
            .get("offset")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|offset| usize::try_from(offset).ok())
                    .filter(|offset| *offset < crate::web::MAX_FETCH_EXTRACTED_BYTES)
                    .ok_or(BrokerError::Malformed)
            })
            .transpose()?
            .unwrap_or(0);
        let mut source = match (source_id, url) {
            (Some(source_id), None) => self
                .capture
                .web_sources
                .lock()
                .await
                .get(source_id)
                .cloned()
                .ok_or(BrokerError::MethodDenied)?,
            (None, Some(url)) => crate::web::source_from_url(url)
                .map_err(|error| BrokerError::Execution(error.to_string()))?,
            _ => return Err(BrokerError::Malformed),
        };
        // A direct URL can declare publication only in its fetched document.
        // Reject already-known future publications early, then check the actual
        // fetched metadata before staging any result or evidence.
        if cutoff.is_some_and(|cutoff| {
            record_date(source.published_at.as_deref()).is_some_and(|date| date > cutoff)
        }) {
            return Err(BrokerError::Execution(
                "as_of_later: web source publication is unknown or after the requested as-of cutoff".into(),
            ));
        }
        let existing_snapshot = self
            .capture
            .web_fetch_snapshots
            .lock()
            .await
            .get(&source.source_id)
            .cloned();
        // A URL the model wrote opens only where this turn may still go; a
        // search result's source_id and a page already read need no new check.
        if existing_snapshot.is_none() && url.is_some() {
            self.web_fetch_gate(&source.url).await?;
        }
        let mut run_snapshot = match existing_snapshot {
            Some(snapshot) => snapshot,
            None if content_offset == 0 => RunWebFetchSnapshot {
                fetched: crate::web::fetch(&source).await.map_err(web_error)?,
                issued_offsets: BTreeSet::from([0]),
            },
            None => return Err(BrokerError::Malformed),
        };
        require_issued_web_offset(&run_snapshot.issued_offsets, content_offset)?;
        let page = run_snapshot
            .fetched
            .page(content_offset)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        let publication = page.published_at.clone();
        if cutoff.is_some_and(|cutoff| is_after_cutoff(publication.as_deref(), cutoff)) {
            return Err(BrokerError::Execution(
                "as_of_later: web source publication is unknown or after the requested as-of cutoff".into(),
            ));
        }
        source.published_at = publication.clone();
        if let Some(next_offset) = page.next_offset {
            run_snapshot.issued_offsets.insert(next_offset);
        }
        let mut output = serde_json::to_value(page)
            .map_err(|error| BrokerError::Execution(error.to_string()))?;
        if let Some(cutoff) = cutoff {
            if let Some(object) = output.as_object_mut() {
                object.insert(
                    "as_of".into(),
                    json!({
                        "cutoff": cutoff.to_string(),
                        "publication_date": publication,
                        "publication_date_unknown": publication.is_none()
                    }),
                );
            }
        }
        let snapshot_source_id = source.source_id.clone();
        self.capture
            .stage(delivery_id, |pending| {
                pending.web_sources.insert(source.source_id.clone(), source);
                pending
                    .web_fetch_snapshots
                    .insert(snapshot_source_id, run_snapshot);
            })
            .await;
        Ok(output)
    }

    /// After third-party text, a fetch can carry data out in its URL (B88).
    /// The turn's taint decides; a new site is refused with `web_search` as
    /// the way to it.
    async fn web_fetch_gate(&self, url: &str) -> Result<(), BrokerError> {
        use crate::custom_connections::taint::WebFetchTarget;
        let taint = &self.capture.turn_taint;
        if !taint.untrusted_text() {
            return Ok(());
        }
        let holdings = self
            .state
            .store
            .holdings_latest()
            .ok()
            .flatten()
            .map(|snapshot| crate::privacy::holdings::canaries(&snapshot.positions))
            .unwrap_or_default();
        match taint.web_fetch_target(url, &holdings) {
            WebFetchTarget::Allowed => Ok(()),
            WebFetchTarget::Refused(next) => Err(BrokerError::UntrustedTextPaused.with_next(next)),
        }
    }

    async fn web_fetch_passage(
        &self,
        result_ref: &str,
        literal: &str,
        occurrence: usize,
        cutoff: Option<chrono::NaiveDate>,
    ) -> Result<Value, BrokerError> {
        let original = self.capture.run_result(result_ref).await.ok_or_else(||
            BrokerError::Execution("Passage result_ref must name an acknowledged web_fetch result from this turn. Fetch the source first, then use its returned result_ref.".into()))?;
        if original.producer.runtime_id != "native-web"
            || original.producer.tool_name != "web_fetch"
            || original.producer.origin != RunResultOrigin::AcquiredOriginal
        {
            return Err(BrokerError::Execution("Passage result_ref must name an original web_fetch result from this turn; search hits, calculations and local files cannot supply it.".into()));
        }
        let source_id = original
            .payload
            .get("source_id")
            .and_then(Value::as_str)
            .ok_or(BrokerError::MethodDenied)?;
        let digest = original
            .payload
            .get("content_sha256")
            .and_then(Value::as_str)
            .ok_or(BrokerError::MethodDenied)?;
        let snapshots = self.capture.web_fetch_snapshots.lock().await;
        let snapshot = snapshots.get(source_id).ok_or_else(||
            BrokerError::Execution("The acquired web snapshot is unavailable in this turn. Fetch the source again before selecting a passage.".into()))?;
        if !snapshot.fetched.matches_source(source_id, digest) {
            return Err(BrokerError::Execution("Passage result_ref does not match the retained web snapshot. Fetch the source again before selecting a passage.".into()));
        }
        let publication = original.payload.get("published_at").and_then(Value::as_str);
        if cutoff.is_some_and(|cutoff| is_after_cutoff(publication, cutoff)) {
            return Err(BrokerError::Execution(
                "as_of_later: web source publication is unknown or after the requested as-of cutoff".into(),
            ));
        }
        let (passage, match_offset) = snapshot.fetched.literal_passage(literal, occurrence)
            .ok_or_else(|| BrokerError::Execution(format!("No literal match at occurrence {occurrence} in the retained extracted text. Try a shorter exact phrase, check the zero-based occurrence, or inspect the original if extraction is incomplete. No new network request was made.")))?;
        let coverage = format!("Host-selected literal passage; UTF-8 bytes {}–{} of {} extracted bytes (end exclusive)",
            passage.content_offset, passage.content_offset + passage.content_bytes, passage.extracted_bytes);
        let mut output = serde_json::to_value(passage).map_err(|_| BrokerError::Malformed)?;
        output["coverage"] = json!(coverage);
        output["selection"] = json!({"type":"literal_passage", "find":literal, "occurrence":occurrence,
            "match_offset":match_offset, "offset_unit":"utf8_bytes", "source_result_ref":result_ref});
        output[super::RESOLVED_UPSTREAM_FIELD] = json!([result_ref]);
        if let Some(cutoff) = cutoff {
            output["as_of"] = json!({"cutoff":cutoff.to_string(),"publication_date":publication,"publication_date_unknown":publication.is_none()});
        }
        Ok(output)
    }
}

fn apply_web_search_cutoff(
    output: &mut crate::web::WebSearchOutput,
    sources: &mut Vec<crate::web::WebSource>,
    cutoff: chrono::NaiveDate,
) -> Value {
    let unknown = output
        .results
        .iter()
        .filter(|result| record_date(result.published_at.as_deref()).is_none())
        .count();
    let before = output.results.len();
    output
        .results
        .retain(|result| !is_after_cutoff(result.published_at.as_deref(), cutoff));
    sources.retain(|source| !is_after_cutoff(source.published_at.as_deref(), cutoff));
    let excluded = before - output.results.len();
    output.returned_count = output.results.len();
    if excluded > 0 {
        output.warnings.push(format!(
            "Excluded {excluded} web results unavailable at {cutoff}; {unknown} have unknown publication dates."
        ));
    }
    json!({
        "cutoff": cutoff.to_string(),
        "undated_results_kept": false,
        "unknown_publication_excluded": unknown,
        "post_cutoff_excluded": excluded.saturating_sub(unknown)
    })
}

fn require_issued_web_offset(
    issued_offsets: &BTreeSet<usize>,
    content_offset: usize,
) -> Result<(), BrokerError> {
    issued_offsets
        .contains(&content_offset)
        .then_some(())
        .ok_or(BrokerError::Malformed)
}

fn parse_search_domains(value: Option<&Value>) -> Result<Vec<String>, BrokerError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or(BrokerError::Malformed)?;
    if items.is_empty() || items.len() > 10 {
        return Err(BrokerError::Malformed);
    }
    let mut domains = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let domain = item
            .as_str()
            .ok_or(BrokerError::Malformed)
            .and_then(|value| {
                crate::web::validate_search_domain(value).map_err(|_| BrokerError::Malformed)
            })?;
        if !seen.insert(domain.clone()) {
            return Err(BrokerError::Malformed);
        }
        domains.push(domain);
    }
    Ok(domains)
}

#[cfg(test)]
mod paging_tests {
    use super::*;

    fn passage_executor(temporary: &tempfile::TempDir) -> (AppToolExecutor, ToolPolicy) {
        use crate::commands::tests::{chat, profile, seed_profile};
        let workspace = temporary.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        state
            .store
            .create_chat(&chat("chat-a", "loupist-a", 1))
            .unwrap();
        (
            AppToolExecutor {
                state,
                capture: Arc::new(ToolCapture::default()),
                loupist_id: "loupist-a".into(),
                capability_ids: BTreeSet::from(["community.web-research".into()]),
            },
            ToolPolicy {
                loupist_id: "loupist-a".into(),
                session_id: "chat-a".into(),
                use_memory: false,
                propose_memory_updates: false,
                memory_proposal_budget: 0,
                as_of: None,
            },
        )
    }

    #[tokio::test]
    async fn web_passage_selects_late_exact_text_from_an_acknowledged_fetch_without_refetching() {
        let text = format!(
            "# Annual report\n{}\n## 2025년 연결 손익계산서\n영업이익: 12.166조 원\n{}",
            "회사 개요와 사업 현황입니다.\n".repeat(12000),
            "추가 주석입니다.\n".repeat(1000)
        );
        let _network = crate::web::script::install(vec![(
            "https://example.com/report",
            vec![crate::web::script::ok(
                "text/markdown; charset=utf-8",
                text.as_bytes().to_vec(),
            )],
        )]);
        let temporary = tempfile::tempdir().unwrap();
        let (executor, policy) = passage_executor(&temporary);
        let first = executor
            .execute_for_delivery(
                &policy,
                ToolMethod::WebFetch,
                json!({"url":"https://example.com/report"}),
                "first",
            )
            .await
            .unwrap();
        assert!(!first["content"].as_str().unwrap().contains("12.166"));
        let original_ref = first["result_ref"].as_str().unwrap();
        let select = |find: &str| {
            executor.execute(
                &policy,
                ToolMethod::WebFetch,
                json!({"result_ref":original_ref,"find":find}),
            )
        };
        assert!(
            select("영업이익:").await.is_err(),
            "an unacknowledged fetch grants no passage selection"
        );
        executor.commit_delivery(&policy, "first").await.unwrap();

        let selected = select("영업이익:").await.unwrap();
        let content =
            crate::custom_connections::taint::delimited_body(selected["content"].as_str().unwrap())
                .unwrap();
        let offset = selected["content_offset"].as_u64().unwrap() as usize;
        assert!(content.len() <= 8 * 1024);
        assert_eq!(&text[offset..offset + content.len()], content);
        assert!(content.contains("영업이익: 12.166조 원"));
        let selected_ref = selected["result_ref"].as_str().unwrap();
        let retained = executor.capture.run_result(selected_ref).await.unwrap();
        assert_eq!(retained.producer.origin, RunResultOrigin::AcquiredOriginal);
        assert_eq!(retained.upstream_result_refs, [original_ref]);
        let receipts = executor.capture.evidence_refs.lock().await.clone();
        let receipt = receipts
            .iter()
            .find(|item| item.id == evidence::evidence_id(selected_ref))
            .unwrap();
        let matched = receipt.excerpt_match.as_ref().unwrap();
        assert_eq!(&receipt.excerpt[matched.start..matched.end], "영업이익:");
        assert_eq!(
            receipt.upstream_evidence_ids,
            [evidence::evidence_id(original_ref)]
        );

        assert!(select("영업이익: 99").await.is_err());
        assert!(executor
            .execute(
                &policy,
                ToolMethod::WebFetch,
                json!({"result_ref":original_ref,"find":"영업이익:","as_of":"2020-01-01"}),
            )
            .await
            .is_err());
        assert_eq!(
            executor.capture.evidence_refs.lock().await.len(),
            receipts.len()
        );
        // Passage offsets cannot be repurposed as pagination cursors; issued ones can.
        assert!(matches!(
            executor
                .execute(
                    &policy,
                    ToolMethod::WebFetch,
                    json!({"source_id":first["source_id"],"offset":offset}),
                )
                .await,
            Err(BrokerError::Malformed)
        ));
        let next = executor
            .execute(
                &policy,
                ToolMethod::WebFetch,
                json!({"source_id":first["source_id"],"offset":first["next_offset"]}),
            )
            .await
            .unwrap();
        assert_eq!(next["content_offset"], first["next_offset"]);
        assert_eq!(crate::web::script::requests().len(), 1);
    }

    #[test]
    fn historical_web_search_excludes_unknown_and_future_publication_without_backdating_retrieval()
    {
        let dates = [
            None,
            Some("not-a-date"),
            Some("2020-01-02T23:59:00Z"),
            Some("2020-01-03"),
            Some("2020-01-02T23:59:00-05:00"),
        ];
        let mut sources = dates
            .into_iter()
            .enumerate()
            .map(|(index, published)| crate::web::WebSource {
                source_id: format!("web:{index}"),
                title: format!("Report {index}"),
                url: format!("https://example.com/{index}"),
                snippet: "A report".into(),
                published_at: published.map(str::to_owned),
            })
            .collect::<Vec<_>>();
        let mut output = crate::web::WebSearchOutput {
            selected_provider: "test".into(),
            retrieved_at: "2026-10-03T12:00:00Z".into(),
            untrusted_content: true,
            results: sources
                .iter()
                .map(|source| crate::web::WebSourceOutput {
                    source_id: source.source_id.clone(),
                    title: source.title.clone(),
                    url: source.url.clone(),
                    snippet: source.snippet.clone(),
                    published_at: source.published_at.clone(),
                })
                .collect(),
            attempts: vec![],
            received_count: 5,
            returned_count: 5,
            dropped: crate::web::SearchDroppedCounts::default(),
            warnings: vec![],
        };
        let receipt = apply_web_search_cutoff(
            &mut output,
            &mut sources,
            chrono::NaiveDate::from_ymd_opt(2020, 1, 2).unwrap(),
        );
        assert_eq!(output.retrieved_at, "2026-10-03T12:00:00Z");
        let kept: Vec<_> = output
            .results
            .iter()
            .map(|source| source.source_id.as_str())
            .collect();
        assert_eq!(kept, ["web:2"]);
        assert_eq!(sources.len(), 1);
        assert_eq!(receipt["unknown_publication_excluded"], 2);
        assert_eq!(receipt["post_cutoff_excluded"], 2);
    }
}
