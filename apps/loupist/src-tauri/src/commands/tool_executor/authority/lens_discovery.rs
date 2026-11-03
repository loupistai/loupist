//! Non-authoritative comparison hints for proposed Lens revisions.
use super::*;

impl AppToolExecutor {
    pub(super) fn lens_overlap_warnings(
        &self,
        policy: &ToolPolicy,
        revision: &StagedLensRevision,
    ) -> Vec<Value> {
        // Saving alone cannot authorize discovery of other retained documents.
        if !policy.use_memory {
            return Vec::new();
        }
        let result = crate::knowledge::KnowledgeStore::for_loupist(
            self.state.store.clone(),
            &policy.loupist_id,
        )
        .and_then(|store| {
            let (standing, searches) = store.read(|read| {
                let standing = read.load_standing_selection(&policy.loupist_id)?;
                let mut searches = Vec::new();
                for text in [
                    revision.title.as_deref().unwrap_or_default(),
                    revision.document.applicability.as_str(),
                    revision.document.markdown.as_str(),
                ] {
                    let query: String = text.chars().take(200).collect();
                    if query.trim().is_empty() || query.trim() == "*" { continue; }
                    searches.push(read.prepare_search(&policy.loupist_id, "lens", &query, 20,
                        &crate::knowledge::SearchOptions::default())?);
                }
                Ok((standing, searches))
            })?;
            let mut hits = std::collections::BTreeMap::new();
            // Use the existing lexical/financial-alias search, without model or
            // embedding calls. Similar vocabulary is a comparison hint, never a
            // semantic verdict that two methods duplicate or contradict one another.
            let searches: Vec<_> = searches.into_iter().map(|prepared| prepared.rank(None)).collect();
            let current = store.read(|read| read.visible_search_revisions(&policy.loupist_id))?;
            for mut ranked in searches {
                ranked.retain_current(&current);
                for hit in ranked.documents {
                    if hit.record_id == revision.record_id
                        || standing.as_ref().is_some_and(|(id, _)| *id == hit.record_id)
                        || (hit.matched_terms.len() < 2 && hit.score < 600)
                    {
                        continue;
                    }
                    let prior = hits.entry(hit.record_id.clone()).or_insert_with(|| hit.clone());
                    if hit.score > prior.score {
                        *prior = hit;
                    }
                }
            }
            let mut hits: Vec<_> = hits.into_values().collect();
            hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.record_id.cmp(&b.record_id)));
            Ok(hits.into_iter().take(3).map(|hit| json!({
                "code": "related_lens",
                "message": "Shared terms suggest related scope. Read this method and compare assumptions and limits for duplication or conflict; the save is not blocked and no conflict is established.",
                "recordId": hit.record_id,
                "revisionId": hit.revision_id,
                "title": hit.title,
                "matchedTerms": hit.matched_terms,
                "tool": "lens_read",
                "readArgs": {"recordId": hit.record_id, "revisionId": hit.revision_id},
                "notEvidence": true
            })).collect())
        });
        result.unwrap_or_else(|_| vec![json!({
            "code": "related_lens_check_unavailable",
            "message": "Related methods could not be checked. Search the Lens library before treating this as a new or compatible method.",
            "notEvidence": true
        })])
    }
}
