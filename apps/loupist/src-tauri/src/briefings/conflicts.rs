//! Briefing navigation pointers from current, disputed World claims in the run window.
use crate::{
    app::{AppState, CommandError},
    commands::map_store,
    knowledge::{ClaimStatus, WorldSnapshotBody},
    store::user_context::MapContext,
};
use std::collections::BTreeSet;

pub(super) fn entity_ids(
    state: &AppState,
    loupist: &str,
    since: i64,
    until: i64,
    context: &MapContext,
) -> Result<Vec<String>, CommandError> {
    let connection = state.store.lock_loupist(loupist).map_err(map_store)?;
    let mut statement = connection.prepare("SELECT record.record_id,snapshot.snapshot_json FROM knowledge_records record JOIN knowledge_record_heads head ON head.record_id=record.record_id JOIN knowledge_world_snapshots snapshot ON snapshot.revision_id=head.revision_id WHERE record.owner_loupist_id=?1 AND record.kind='world' ORDER BY record.record_id").map_err(|e| CommandError::internal(e.to_string()))?;
    let snapshots = statement
        .query_map([loupist], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| CommandError::internal(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CommandError::internal(e.to_string()))?;
    let mut result = BTreeSet::new();
    for (record, json) in snapshots {
        let body: WorldSnapshotBody =
            serde_json::from_str(&json).map_err(|e| CommandError::internal(e.to_string()))?;
        let mut events = connection.prepare("SELECT DISTINCT subject_id FROM knowledge_growth_events WHERE loupist_id=?1 AND record_id=?2 AND at_ms>?3 AND at_ms<=?4 AND kind IN ('claim_added','claim_disputed') AND subject_id IS NOT NULL").map_err(|e| CommandError::internal(e.to_string()))?;
        let changed = events
            .query_map(rusqlite::params![loupist, record, since, until], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| CommandError::internal(e.to_string()))?
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|e| CommandError::internal(e.to_string()))?;
        let (mut followed, _) = crate::knowledge::match_items(&context.watchlist, &body.nodes);
        followed.extend(crate::knowledge::match_items(&context.holdings, &body.nodes).0);
        let mut neighborhood = followed.clone();
        for claim in &body.claims {
            if claim.status != ClaimStatus::Withdrawn
                && claim
                    .endpoints
                    .iter()
                    .any(|end| followed.contains(&end.node_id))
            {
                neighborhood.extend(claim.endpoints.iter().map(|end| end.node_id.clone()));
            }
        }
        let known: BTreeSet<_> = body.nodes.iter().map(|node| &node.id).collect();
        for claim in &body.claims {
            if claim.status == ClaimStatus::Disputed
                && changed.contains(&claim.id)
                && claim
                    .endpoints
                    .iter()
                    .any(|end| neighborhood.contains(&end.node_id))
            {
                result.extend(
                    claim
                        .endpoints
                        .iter()
                        .filter(|end| known.contains(&end.node_id))
                        .map(|end| end.node_id.clone()),
                );
            }
        }
    }
    Ok(result.into_iter().take(20).collect())
}
