//! Read-only projection of previously unverified receipts.
use super::{
    answer_units, check_printed, kind_of, located_links, EvidenceLink, RunResult, RunResultOrigin,
    RunResultProducer, RunResultRegistry,
};
use crate::{
    app::AppState,
    chat::{
        artifacts::ChatArtifactPayload,
        grid::{GridCell, GridDocument},
    },
    commands::types::{ChatArtifactViewDto, ChatEvidenceViewDto, ChatThreadDto},
    domain::{
        answer_checks::{clip, ComparisonReason, ComparisonReplay, ComparisonScope},
        ChatEvidenceKind, ChatEvidenceRef, ChatMessage, ChatSession, MemoryAccess, NumberCheck,
        NumberCheckKind, NumberCheckStatus,
    },
    knowledge::numbers::{self, Printed},
    util::sha256,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

const MAX_CACHED_PROJECTIONS: usize = 128;
const CHECKER_VERSION: u32 = 2;

#[derive(Clone, Default)]
pub(crate) struct ReplayCache(Arc<Mutex<CachedProjections>>);

type CachedProjections = VecDeque<(String, Vec<NumberCheck>)>;

fn unresolved(check: &NumberCheck) -> bool {
    matches!(
        check.status,
        NumberCheckStatus::NotFound | NumberCheckStatus::Differs
    )
}

#[derive(Clone, Copy)]
struct Replay<'a> {
    state: &'a AppState,
    workspace: &'a str,
}

impl Replay<'_> {
    fn project<'a>(
        self,
        id: &str,
        content: &str,
        checks: &[NumberCheck],
        evidence: impl Iterator<Item = &'a ChatEvidenceRef>,
        replace: impl FnOnce(&mut ReplayResults<'_>, &mut [NumberCheck]),
    ) -> Option<Vec<NumberCheck>> {
        if !checks.iter().any(unresolved) {
            return None;
        }
        let evidence: BTreeMap<_, _> = evidence
            .filter(|item| {
                checks
                    .iter()
                    .any(|check| unresolved(check) && check.evidence_id == item.id)
            })
            .map(|item| (item.id.as_str(), item))
            .collect();
        let pinned: Vec<_> = evidence
            .values()
            .map(|item| {
                let dataset_present = item
                    .dataset_ref
                    .as_ref()
                    .filter(|_| item.excerpt_truncated)
                    .map(|reference| {
                        self.state
                            .store
                            .dataset_metadata(
                                self.workspace,
                                &reference.dataset_id,
                                reference.revision,
                            )
                            .is_ok()
                    });
                (
                    &item.id,
                    &item.title,
                    &item.content_sha256,
                    &item.excerpt_match,
                    &item.dataset_ref,
                    &item.tool_name,
                    &item.provider,
                    &item.kind,
                    item.excerpt_truncated,
                    sha256(item.excerpt.as_bytes()),
                    dataset_present,
                )
            })
            .collect();
        let key = sha256(
            &serde_json::to_vec(&(
                CHECKER_VERSION,
                self.workspace,
                id,
                sha256(content.as_bytes()),
                checks,
                pinned,
            ))
            .ok()?,
        );
        if let Ok(mut cache) = self.state.number_replay_cache.0.lock() {
            if let Some(at) = cache.iter().position(|(cached, _)| cached == &key) {
                let entry = cache.remove(at)?;
                let projected = entry.1.clone();
                cache.push_back(entry);
                return Some(projected);
            }
        }

        let mut results = ReplayResults {
            replay: self,
            evidence,
            loaded: BTreeMap::new(),
            remaining_payload_bytes: super::super::MAX_RUN_RESULT_REGISTRY_BYTES,
        };
        let mut projected = checks.to_vec();
        for check in projected.iter_mut().filter(|check| unresolved(check)) {
            if !results.evidence.contains_key(check.evidence_id.as_str()) {
                evidence_unavailable(check);
            }
        }
        replace(&mut results, &mut projected);
        if let Ok(mut cache) = self.state.number_replay_cache.0.lock() {
            cache.retain(|(cached, _)| cached != &key);
            if cache.len() == MAX_CACHED_PROJECTIONS {
                cache.pop_front();
            }
            cache.push_back((key, projected.clone()));
        }
        Some(projected)
    }

    fn retained(
        self,
        evidence: &ChatEvidenceRef,
        kind: NumberCheckKind,
        budget: &mut usize,
    ) -> Option<RunResult> {
        let origin = match evidence.kind {
            ChatEvidenceKind::Original => RunResultOrigin::AcquiredOriginal,
            ChatEvidenceKind::Discovery => return None,
            ChatEvidenceKind::Derived => RunResultOrigin::Derived,
            ChatEvidenceKind::LocalFile => RunResultOrigin::WorkspaceFile,
            ChatEvidenceKind::UserProvided => RunResultOrigin::UserProvided,
            ChatEvidenceKind::SavedContext => RunResultOrigin::SavedContext,
        };
        let payload = match kind {
            NumberCheckKind::Passage if evidence.excerpt.trim().is_empty() => return None,
            NumberCheckKind::Passage if evidence.tool_name == "world_read" => {
                json!({"scope": "source", "text": evidence.excerpt})
            }
            NumberCheckKind::Passage => json!({"content": evidence.excerpt}),
            _ => match serde_json::from_str::<Value>(&evidence.excerpt)
                .ok()
                .filter(|_| !evidence.excerpt_truncated)
            {
                Some(payload) => payload,
                None if kind == NumberCheckKind::Data => {
                    let reference = evidence.dataset_ref.as_ref()?;
                    let (stored, mut payload, _, bytes) = self
                        .state
                        .store
                        .dataset_bounded(
                            self.workspace,
                            &reference.dataset_id,
                            reference.revision,
                            *budget,
                        )
                        .ok()?;
                    *budget -= bytes;
                    if stored.digest != reference.digest || !payload.is_object() {
                        return None;
                    }
                    payload["dataset_ref"] = json!(reference);
                    payload
                }
                None => return None,
            },
        };
        let runtime_id = if evidence.tool_name == "economy_fred_series" {
            "openbb"
        } else {
            "retained"
        };
        let result = RunResult {
            result_ref: evidence.id.clone(),
            producer: RunResultProducer {
                origin,
                runtime_id: runtime_id.into(),
                tool_name: evidence.tool_name.clone(),
                provider: evidence.provider.clone(),
            },
            origin: evidence.url.clone(),
            request_digest: String::new(),
            response_digest: evidence.content_sha256.clone(),
            retrieved_at: evidence.retrieved_at.clone(),
            payload,
            model_delivery: None,
            warnings: Vec::new(),
            upstream_result_refs: Vec::new(),
        };
        (kind_of(&result) == kind).then_some(result)
    }

    fn message<'a>(
        self,
        message: &ChatMessage,
        checks: &[NumberCheck],
        evidence: impl Iterator<Item = &'a ChatEvidenceRef>,
    ) -> Option<Vec<NumberCheck>> {
        self.project(
            &message.id,
            &message.content,
            checks,
            evidence,
            |results, checks| {
                let units = answer_units::governing_units(&message.content);
                let links = located_links(&message.content);
                for check in checks.iter_mut().filter(|check| unresolved(check)) {
                    let Some((at, link)) =
                        links.get(check.link_index as usize).filter(|(_, link)| {
                            link.evidence_id == check.evidence_id && link.text == check.text
                        })
                    else {
                        continue;
                    };
                    if let Some(printed) =
                        numbers::parse_with_unit(&link.text, units.get(at).copied())
                    {
                        results.check(check, link, printed);
                    }
                }
            },
        )
    }
}

struct ReplayResults<'a> {
    replay: Replay<'a>,
    evidence: BTreeMap<&'a str, &'a ChatEvidenceRef>,
    loaded: BTreeMap<(&'a str, u8), Option<RunResult>>,
    remaining_payload_bytes: usize,
}

fn evidence_unavailable(check: &mut NumberCheck) {
    check.comparison_reason = None;
    check.comparison_replay = Some(ComparisonReplay::EvidenceUnavailable);
    check.compared_value = None;
    check.nearest = None;
    check.nearest_source_unit = None;
    check.nearest_source_scale = None;
    check.span = None;
    check.pointer = None;
    check.context = None;
    check.fields.clear();
}

impl ReplayResults<'_> {
    fn check(&mut self, check: &mut NumberCheck, link: &EvidenceLink, printed: Printed) {
        let Some(&evidence) = self.evidence.get(link.evidence_id.as_str()) else {
            return;
        };
        let Some(result) = self
            .loaded
            .entry((&evidence.id, check.kind as u8))
            .or_insert_with(|| {
                self.replay
                    .retained(evidence, check.kind, &mut self.remaining_payload_bytes)
            })
            .as_ref()
        else {
            if evidence.excerpt.trim().is_empty() && evidence.dataset_ref.is_none() {
                evidence_unavailable(check);
            }
            return;
        };
        let mut projected = check_printed(
            link,
            printed,
            result,
            Some(evidence),
            &RunResultRegistry::default(),
        );
        if projected.comparison_reason == Some(ComparisonReason::NoCandidate)
            && check.kind == NumberCheckKind::Passage
        {
            projected.comparison_replay = Some(ComparisonReplay::NoCandidate {
                searched: ComparisonScope::RetainedPassage {
                    title: clip(&evidence.title, 512),
                },
            });
        }
        if check.calc.is_some() {
            // The calculation's input provenance is already pinned in the receipt.
            projected.calc.clone_from(&check.calc);
        }
        *check = projected;
    }
}

fn available_evidence<'a>(
    views: &'a [ChatEvidenceViewDto],
    message: Option<&'a ChatMessage>,
) -> impl Iterator<Item = &'a ChatEvidenceRef> {
    views.iter().filter_map(move |view| {
        let evidence = &view.snapshot;
        let removed_source = view.memory_origin.is_none()
            && evidence.tool_name == "world_read"
            && evidence.read_ref.as_ref().is_some_and(|read_ref| {
                message.is_some_and(|message| {
                    message.memory_refs.iter().any(|memory| {
                        memory.access == MemoryAccess::ExactRead
                            && memory.kind == "source"
                            && &memory.read_ref == read_ref
                    })
                })
            });
        (!removed_source).then_some(evidence)
    })
}

pub(in crate::commands) fn project_chat(
    state: &AppState,
    chat: &ChatSession,
    dto: &mut ChatThreadDto,
) {
    let replay = Replay {
        state,
        workspace: &chat.loupist_id,
    };
    for (stored, rendered) in chat.messages.iter().zip(&mut dto.messages) {
        let evidence = available_evidence(&rendered.evidence_refs, Some(stored));
        if let Some(checks) = replay.message(stored, &rendered.number_checks, evidence) {
            rendered.number_checks = checks;
        }
    }
}

pub(in crate::commands) fn project_grid(
    state: &AppState,
    chat: &ChatSession,
    dto: &mut ChatArtifactViewDto,
) {
    let ChatArtifactPayload::Grid { grid, .. } = &mut dto.revision.payload else {
        return;
    };
    let mut cells: Vec<(&mut NumberCheck, Option<Printed>)> = match grid.as_mut() {
        GridDocument::Assets(grid) => grid
            .cells
            .iter_mut()
            .flat_map(|row| row.iter_mut().zip(&grid.columns))
            .filter_map(|(cell, column)| {
                let GridCell::Value(cell) = cell else {
                    return None;
                };
                unresolved(&cell.check).then(|| {
                    (
                        &mut cell.check,
                        super::super::grid::read_value(&cell.value, column.unit),
                    )
                })
            })
            .collect(),
        GridDocument::Conflict(grid) => grid
            .reports
            .iter_mut()
            .filter_map(|report| {
                let check = report.check.as_mut().filter(|check| unresolved(check))?;
                Some((check, numbers::parse_printed(&report.says)))
            })
            .collect(),
    };
    if cells.is_empty() {
        return;
    }
    let checks: Vec<_> = cells.iter().map(|(check, _)| (**check).clone()).collect();
    let source_message = chat
        .messages
        .iter()
        .chain(chat.versions.iter().flat_map(|version| &version.messages))
        .find(|message| message.id == dto.revision.source_message_id);
    let evidence = available_evidence(&dto.evidence_refs, source_message);
    let replay = Replay {
        state,
        workspace: &chat.loupist_id,
    };
    let id = format!(
        "{}/{}",
        dto.revision.source_message_id, dto.revision.artifact_id
    );
    let result = replay.project(
        &id,
        &dto.revision.digest,
        &checks,
        evidence,
        |results, checks| {
            for (check, (_, printed)) in checks.iter_mut().zip(&cells) {
                if let Some(printed) = printed {
                    let link = EvidenceLink {
                        index: check.link_index,
                        text: check.text.clone(),
                        evidence_id: check.evidence_id.clone(),
                    };
                    results.check(check, &link, *printed);
                }
            }
        },
    );
    if let Some(projected) = result {
        for ((check, _), next) in cells.iter_mut().zip(projected) {
            **check = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChatRole;

    fn message(id: &str, content: &str) -> ChatMessage {
        crate::test_fixtures::chat_message(id, ChatRole::Assistant, content)
    }

    fn saved_evidence(id: &str, tool: &str, kind: &str, text: &str) -> ChatEvidenceRef {
        serde_json::from_value(json!({
            "id": id, "title": "Saved evidence", "tool_name": tool, "kind": kind,
            "excerpt": text, "excerpt_truncated": false, "content_sha256": sha256(text.as_bytes()),
            "retrieved_at": "2026-11-21T11:59:00Z", "warnings": []
        }))
        .unwrap()
    }

    fn unresolved_check(index: u32, id: &str, text: &str) -> NumberCheck {
        serde_json::from_value(json!({
            "link_index":index, "text":text, "evidence_id":id, "kind":"passage", "status":"not_found"
        })).unwrap()
    }

    #[test]
    fn saved_receipts_use_the_current_checker_and_leave_storage_untouched() {
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        let profile = crate::test_fixtures::loupist_profile("loupist-a");
        state.store.create_loupist(&profile).unwrap();
        let mut chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
        let ms = r#"[{"end":"2023-09-30","val":383285000000,"accn":"0000320193-24-000123"}]"#;
        let tw: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/c002-tsmc-close.json"))
                .unwrap();
        let mut reply = message("reply",
            "| Total revenue (USD millions) |\n|---|\n| [383,285](#evidence-ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) |\n\n[−4.81%](#evidence-ev_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb), [372원](#evidence-ev_cccccccccccccccccccccccccccccccc), [−342.56달러](#evidence-ev_dddddddddddddddddddddddddddddddd), [100.0%](#evidence-ev_dddddddddddddddddddddddddddddddd), [55.0%](#evidence-ev_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee).");
        reply.evidence_refs = vec![
            saved_evidence(
                "ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "web_fetch",
                "original",
                ms,
            ),
            saved_evidence(
                "ev_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "web_fetch",
                "original",
                tw["excerpt"].as_str().unwrap(),
            ),
            saved_evidence(
                "ev_cccccccccccccccccccccccccccccccc",
                "web_fetch",
                "original",
                "Common shares: KRW 372 (0.2%)",
            ),
            saved_evidence(
                "ev_dddddddddddddddddddddddddddddddd",
                "web_fetch",
                "original",
                "Closing price USD\n$342.56",
            ),
        ];
        reply.number_checks = [
            ("ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "383,285"),
            ("ev_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "−4.81%"),
            ("ev_cccccccccccccccccccccccccccccccc", "372원"),
            ("ev_dddddddddddddddddddddddddddddddd", "−342.56달러"),
            ("ev_dddddddddddddddddddddddddddddddd", "100.0%"),
            ("ev_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "55.0%"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, text))| unresolved_check(index as u32, id, text))
        .collect();
        reply.number_checks[4].status = NumberCheckStatus::Matched;
        chat.messages.push(reply);
        state.store.create_chat(&chat).unwrap();
        let baseline = state.store.get_chat(&chat.id).unwrap().unwrap();
        for _ in 0..2 {
            let dto = crate::commands::types::chat_dto_with_memory(&state, &baseline).unwrap();
            let result = &dto.messages[0].number_checks;
            assert_eq!(
                result.iter().map(|item| item.status).collect::<Vec<_>>(),
                [
                    NumberCheckStatus::Matched,
                    NumberCheckStatus::Matched,
                    NumberCheckStatus::Matched,
                    NumberCheckStatus::NotFound,
                    NumberCheckStatus::Matched,
                    NumberCheckStatus::NotFound,
                ],
                "{result:?}"
            );
            assert!(result[0].scale_conversion.is_some());
            assert_eq!(
                result[3].comparison_reason,
                Some(crate::domain::answer_checks::ComparisonReason::SignConflict)
            );
            assert!(result[3].compared_value.is_some());
            assert_eq!(result[4], baseline.messages[0].number_checks[4]);
            assert_eq!(
                result[5].comparison_replay,
                Some(ComparisonReplay::EvidenceUnavailable)
            );
            assert!(result[5].comparison_reason.is_none());
            assert_eq!(result.iter().filter(|item| unresolved(item)).count(), 2);
        }
        assert_eq!(state.number_replay_cache.0.lock().unwrap().len(), 1);
        let retained = state.store.get_chat(&chat.id).unwrap().unwrap();
        assert_eq!(
            retained.messages[0].number_checks,
            baseline.messages[0].number_checks
        );
    }

    #[test]
    fn missing_figures_distinguish_the_saved_passage_from_missing_evidence() {
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        let mut chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
        let retained = "ev_0123456789abcdef0123456789abcdef";
        let missing = "ev_abcdef0123456789abcdef0123456789";
        let mut answer = message("reply", &format!(
            "[US$383.285 billion](#evidence-{retained}) and [US$391.035 billion](#evidence-{retained}). [US$383.285 billion](#evidence-{missing})."
        ));
        let mut passage = saved_evidence(retained, "web_fetch", "original",
            "Table of Contents\nConsolidated Statements of Comprehensive Income\nConsolidated Balance Sheets");
        passage.title = "Apple Inc. 2024 Form 10-K (Item 8 excerpt)".into();
        passage.excerpt_truncated = true;
        answer.evidence_refs.push(passage.clone());
        answer.number_checks = vec![
            unresolved_check(0, retained, "US$383.285 billion"),
            unresolved_check(1, retained, "US$391.035 billion"),
            unresolved_check(2, missing, "US$383.285 billion"),
        ];
        chat.messages.push(answer.clone());
        let dto = crate::commands::types::chat_dto_with_memory(&state, &chat).unwrap();
        let checks = &dto.messages[0].number_checks;
        for check in &checks[..2] {
            assert_eq!(check.status, NumberCheckStatus::NotFound);
            assert_eq!(check.comparison_reason, Some(ComparisonReason::NoCandidate));
            assert_eq!(
                check.comparison_replay,
                Some(ComparisonReplay::NoCandidate {
                    searched: ComparisonScope::RetainedPassage {
                        title: passage.title.clone()
                    },
                })
            );
            assert!(check.compared_value.is_none());
            let serialized = serde_json::to_value(check).unwrap();
            assert_eq!(
                serialized["comparisonReplay"],
                json!({"reason":"no_candidate","searched":{"kind":"retained_passage", "title":passage.title}})
            );
        }
        assert_eq!(
            checks[2].comparison_replay,
            Some(ComparisonReplay::EvidenceUnavailable)
        );
        assert!(checks[2].comparison_reason.is_none());
        assert_eq!(chat.messages[0].number_checks, answer.number_checks);
    }

    #[test]
    fn stored_owner_replays_are_checked_again_from_their_pinned_text() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/owner-number-checks.json"
        ))
        .unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        let replay = Replay {
            state: &state,
            workspace: "loupist-a",
        };
        for (index, item) in fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| {
                fixture["evidence"][item["evidence"].as_str().unwrap()]["tool"] == "world_read"
            })
            .take(2)
            .enumerate()
        {
            let evidence = &fixture["evidence"][item["evidence"].as_str().unwrap()];
            let figure = item["figure"].as_str().unwrap();
            let mut reply = message(
                &format!("owner-{index}"),
                &format!("[{figure}](#evidence-ev_ffffffffffffffffffffffffffffffff)"),
            );
            reply.evidence_refs = vec![saved_evidence(
                "ev_ffffffffffffffffffffffffffffffff",
                "world_read",
                "saved_context",
                evidence["excerpt"].as_str().unwrap(),
            )];
            reply.number_checks = vec![unresolved_check(
                0,
                "ev_ffffffffffffffffffffffffffffffff",
                figure,
            )];
            let result = replay
                .message(&reply, &reply.number_checks, reply.evidence_refs.iter())
                .unwrap();
            assert!(!unresolved(&result[0]), "{figure}: {result:?}");
        }
        assert_eq!(state.number_replay_cache.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_missing_source_revision_projects_an_unavailable_diagnostic() {
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile("loupist-a"))
            .unwrap();
        let mut chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
        let id = "ev_0123456789abcdef0123456789abcdef";
        let mut reply = message("source", &format!("[372원](#evidence-{id})"));
        let mut evidence = saved_evidence(
            id,
            "world_read",
            "saved_context",
            "Common shares: KRW 372 (0.2%)",
        );
        evidence.read_ref = Some("read-source".into());
        reply.evidence_refs = vec![evidence];
        reply.number_checks = vec![unresolved_check(0, id, "372원")];
        reply.memory_refs = vec![serde_json::from_value(json!({
            "readRef": "read-source", "kind": "source", "title": "Dividend release",
            "excerpt": "Common shares: KRW 372 (0.2%)", "asOf": null, "section": null,
            "access": "exact_read", "fullRecordDigest": null,
            "recordId": "rec_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "revisionId": "rev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "objectId": "dividends", "receiptId": "missing-receipt"
        }))
        .unwrap()];
        reply.observed_exact_count = 1;
        reply.refs_digest = crate::domain::memory_refs_digest(&reply.memory_refs).unwrap();
        chat.messages.push(reply);
        let dto = crate::commands::types::chat_dto_with_memory(&state, &chat).unwrap();
        let mut projected = dto.messages[0].number_checks[0].clone();
        assert_eq!(
            projected.comparison_replay.take(),
            Some(ComparisonReplay::EvidenceUnavailable)
        );
        assert_eq!(projected, chat.messages[0].number_checks[0]);
    }

    #[test]
    fn saved_datasets_replay_only_the_pinned_revision_while_it_exists() {
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        state
            .store
            .create_loupist(&crate::test_fixtures::loupist_profile("loupist-a"))
            .unwrap();
        let stored = state.store.dataset_put("loupist-a", "test-revenue",
            &json!({"data":{"rows":[{"period":"2024", "unit":"USD","value":383285000000_i64}]}}),
            json!({})).unwrap();
        let id = "ev_0123456789abcdef0123456789abcdef";
        let mut reply = message("data", &format!("[383,285 USD million](#evidence-{id})"));
        let mut evidence = saved_evidence(id, "test_data", "original", "{\"data\":");
        evidence.excerpt_truncated = true;
        evidence.dataset_ref = Some(stored.clone());
        reply.evidence_refs = vec![evidence];
        let mut check = unresolved_check(0, id, "383,285 USD million");
        check.kind = NumberCheckKind::Data;
        reply.number_checks = vec![check.clone()];
        let replay = Replay {
            state: &state,
            workspace: "loupist-a",
        };
        let current = replay
            .message(&reply, &reply.number_checks, reply.evidence_refs.iter())
            .unwrap();
        assert_eq!(current[0].status, NumberCheckStatus::Matched);
        assert!(current[0].scale_conversion.is_some());
        {
            let db = state.store.lock_loupist("loupist-a").unwrap();
            db.execute("DELETE FROM datasets WHERE id = ?1", [&stored.dataset_id])
                .unwrap();
        }
        assert_eq!(
            replay
                .message(&reply, &reply.number_checks, reply.evidence_refs.iter())
                .unwrap(),
            [check]
        );
    }

    #[test]
    fn saved_grid_cells_use_the_current_checks_in_the_artifact_view() {
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        let chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
        let id = "ev_0123456789abcdef0123456789abcdef";
        let old = unresolved_check(0, id, "372원");
        let stored: crate::chat::artifacts::ChatArtifactSnapshot = serde_json::from_value(json!({
            "artifact": { "id":"dividends", "chat_session_id":chat.id, "kind":"grid",
                "title":"Dividends", "current_revision":1, "created_at_ms":0, "updated_at_ms":0 },
            "revision": { "artifact_id":"dividends", "revision":1, "digest":"a".repeat(64),
                "source_message_id":"reply", "created_at_ms":0,
                "payload": { "kind":"grid", "schema":"loupist-grid/1", "grid": {
                    "layout":"assets", "rows":[
                        {"symbol":"005930", "market":"KRX", "label":"Samsung Electronics"},
                        {"symbol":"000660", "market":"KRX", "label":"SK Hynix"}
                    ], "columns":[{"id":"dividend", "label":"Dividend", "unit":"amount"}],
                    "cells":[[{"value":"372원", "evidence_id":id, "check":old}],
                        [{"empty":"not_reported"}]]
                } } },
            "chart_dataset":null,
            "evidence_refs":[saved_evidence(id, "web_fetch", "original", "Common shares: KRW 372 (0.2%)")]
        })).unwrap();
        let mut view = ChatArtifactViewDto::from(stored.clone());
        project_grid(&state, &chat, &mut view);
        let payload = serde_json::to_value(&view.revision.payload).unwrap();
        assert_eq!(payload["grid"]["cells"][0][0]["check"]["status"], "matched");
        let retained = serde_json::to_value(&stored.revision.payload).unwrap();
        assert_eq!(
            retained["grid"]["cells"][0][0]["check"]["status"],
            "not_found"
        );
    }

    #[test]
    #[ignore = "requires the read-only audit timing fixture"]
    fn replay_projection_timings() {
        use std::{hint::black_box, time::Instant};
        let path = std::env::var_os("LOUPIST_REPLAY_BENCH_FIXTURE").unwrap();
        let chat: ChatSession = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let state = AppState::for_test(scratch.path().join("app"));
        let count = chat
            .messages
            .iter()
            .flat_map(|item| &item.number_checks)
            .filter(|item| unresolved(item))
            .count();
        for stage in ["baseline", "cold", "warm"] {
            let mut samples = Vec::new();
            for _ in 0..30 {
                if stage == "cold" {
                    state.number_replay_cache.0.lock().unwrap().clear();
                }
                let start = Instant::now();
                let mut dto = crate::commands::types::chat_dto(&chat).unwrap();
                if stage != "baseline" {
                    project_chat(&state, &chat, &mut dto);
                }
                black_box(dto);
                samples.push(start.elapsed());
            }
            samples.sort();
            eprintln!(
                "{} messages, {count} unresolved checks, {stage}: median {:?}, p95 {:?}",
                chat.messages.len(),
                samples[15],
                samples[28]
            );
        }
    }
}
