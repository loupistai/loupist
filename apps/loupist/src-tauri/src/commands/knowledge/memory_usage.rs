//! How often each principle or method shaped a turn, for "What {Loupist} knows".
//! A use is a completed turn that received the item in its bounded context or
//! an acknowledged exact read, or applied it to a World entity. Repeated delivery
//! in one turn counts once; this measures delivery, not influence or correctness.

use super::*;
use std::collections::BTreeMap;

/// The chats a record was last used in, newest first.
const MAX_USED_IN: usize = 5;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryUsageRequest {
    pub loupist_id: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MemoryUsedIn {
    pub chat_id: String,
    pub at_ms: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MemoryUsage {
    pub record_id: String,
    pub used_count: u64,
    pub last_used_at_ms: i64,
    pub used_in: Vec<MemoryUsedIn>,
}

#[tauri::command]
pub fn knowledge_memory_usage(
    state: State<'_, AppState>,
    request: MemoryUsageRequest,
) -> Result<Vec<MemoryUsage>, CommandError> {
    usage(&state, &request.loupist_id)
}

pub(crate) fn usage(state: &AppState, loupist_id: &str) -> Result<Vec<MemoryUsage>, CommandError> {
    admit(state, loupist_id)?;
    let rows = with_loupist_knowledge(state, loupist_id, |tx, _| {
        let mut statement = tx.prepare(
            "SELECT u.record_id, t.chat_session_id, MAX(u.at_ms) FROM (
                SELECT record_id, turn_id, issued_at_ms AS at_ms FROM knowledge_lens_deliveries WHERE loupist_id=?1
                UNION ALL
                SELECT record_id, turn_id, created_at_ms AS at_ms FROM knowledge_lens_anchors WHERE loupist_id=?1 AND role='applied' AND turn_id IS NOT NULL
            ) u JOIN knowledge_turns t ON t.turn_id=u.turn_id AND t.loupist_id=?1
            JOIN knowledge_records r ON r.record_id=u.record_id AND r.owner_loupist_id=?1 AND r.kind='lens'
            GROUP BY u.record_id, u.turn_id",
        )?;
        let rows = statement
            .query_map(rusqlite::params![loupist_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    let mut by_record: BTreeMap<String, (u64, i64, BTreeMap<String, i64>)> = BTreeMap::new();
    for (record_id, chat_id, at_ms) in rows {
        let entry = by_record
            .entry(record_id)
            .or_insert((0, 0, BTreeMap::new()));
        entry.0 += 1;
        entry.1 = entry.1.max(at_ms);
        let latest = entry.2.entry(chat_id).or_insert(at_ms);
        *latest = (*latest).max(at_ms);
    }
    Ok(by_record
        .into_iter()
        .map(|(record_id, (used_count, last_used_at_ms, chats))| {
            let mut used_in = chats
                .into_iter()
                .map(|(chat_id, at_ms)| MemoryUsedIn { chat_id, at_ms })
                .collect::<Vec<_>>();
            used_in.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.chat_id.cmp(&b.chat_id)));
            used_in.truncate(MAX_USED_IN);
            MemoryUsage {
                record_id,
                used_count,
                last_used_at_ms,
                used_in,
            }
        })
        .collect())
}

/// Records delivery independently of a Journal save. No additional read or
/// revision authority is issued by these usage rows.
pub(in crate::commands) fn persist_deliveries(
    tx: &rusqlite::Transaction<'_>,
    blobs: &BlobStore,
    loupist: &str,
    chat: &str,
    turn: &str,
    receipts: &[crate::domain::LensReceipt],
    at_ms: i64,
) -> crate::knowledge::KnowledgeResult<()> {
    if receipts.is_empty() {
        return Ok(());
    }
    crate::knowledge::KnowledgeWrite::bind(tx, blobs).open_turn(crate::knowledge::OpenTurn {
        loupist_id: loupist.into(),
        chat_session_id: chat.into(),
        turn_id: turn.into(),
        at_ms,
    })?;
    for receipt in receipts {
        let json = serde_json::to_string(receipt)?;
        tx.execute("INSERT INTO knowledge_lens_deliveries(delivery_id,turn_id,loupist_id,record_id,revision_id,revision_digest,document_json,body_digest,issued_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![format!("dlv_{}", uuid::Uuid::new_v4().simple()),turn,loupist,receipt.record_id,receipt.revision_id,receipt.revision_digest,json,crate::util::sha256(json.as_bytes()),at_ms])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::{profile, seed_profile};

    #[test]
    fn usage_counts_distinct_turns_across_reads_and_applications_per_record() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("loupist");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        with_loupist_knowledge(&state, "loupist-a", |tx, _| {
            tx.execute_batch(
                "INSERT INTO knowledge_records(record_id, owner_loupist_id, kind, title, created_at_ms) VALUES ('lens-a','loupist-a','lens','Cash conversion',1);
                 INSERT INTO knowledge_turns(turn_id, loupist_id, chat_session_id, opened_at_ms) VALUES ('turn-1','loupist-a','chat-1',10),('turn-2','loupist-a','chat-2',20);
                 INSERT INTO knowledge_lens_anchors(loupist_id, record_id, role, node_id, name_fold, turn_id, created_at_ms) VALUES ('loupist-a','lens-a','applied','entity-1','','turn-1',11),('loupist-a','lens-a','applied','entity-2','','turn-1',12),('loupist-a','lens-a','applied','entity-1','','turn-2',21);",
            )?;
            Ok(())
        })
        .unwrap();
        let usage = usage(&state, "loupist-a").unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].record_id, "lens-a");
        // Two entities in one turn are one use.
        assert_eq!(usage[0].used_count, 2);
        assert_eq!(usage[0].last_used_at_ms, 21);
        assert_eq!(
            usage[0]
                .used_in
                .iter()
                .map(|u| u.chat_id.as_str())
                .collect::<Vec<_>>(),
            ["chat-2", "chat-1"]
        );
        assert!(usage_for_other_loupist(&state).is_err());
    }

    fn usage_for_other_loupist(state: &AppState) -> Result<Vec<MemoryUsage>, CommandError> {
        usage(state, "loupist-missing")
    }
}
