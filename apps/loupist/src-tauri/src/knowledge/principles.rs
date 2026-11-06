//! User-authored research rules, stored as Lens records of kind `principle`
//! whose payload is the input itself.
use super::{KnowledgeError, KnowledgeResult, RecordKind};
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Principle {
    pub record_id: String,
    pub revision_id: String,
    pub loupist_id: String,
    pub statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_message_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleInput {
    pub statement: String,
    pub why: Option<String>,
    pub example: Option<String>,
    pub origin: String,
    pub origin_chat_id: Option<String>,
    pub origin_message_id: Option<String>,
}

pub fn validate_principle(input: &mut PrincipleInput) -> KnowledgeResult<()> {
    input.statement = input.statement.trim().to_owned();
    if input.statement.is_empty()
        || input.statement.chars().count() > 280
        || input.statement.chars().any(|c| c == '\0')
    {
        return Err(KnowledgeError::Invalid(
            "statement must contain 1–280 characters",
        ));
    }
    for text in [&mut input.why, &mut input.example].into_iter().flatten() {
        *text = text.trim().to_owned();
        if text.chars().count() > 500 || text.chars().any(|c| c == '\0') {
            return Err(KnowledgeError::Invalid(
                "why and example must contain at most 500 characters",
            ));
        }
    }
    // `shared` is written only when the user accepts a shared-pack suggestion;
    // the principle commands accept settings and feedback only.
    if !matches!(
        input.origin.as_str(),
        "settings" | "feedback" | "chat" | "shared"
    ) {
        return Err(KnowledgeError::Invalid(
            "principle origin must be settings, feedback, chat, or shared",
        ));
    }
    if !matches!(input.origin.as_str(), "settings" | "shared")
        && (input.origin_chat_id.as_deref().is_none_or(str::is_empty)
            || input.origin_message_id.as_deref().is_none_or(str::is_empty))
    {
        return Err(KnowledgeError::Invalid(
            "chat and feedback principles require originChatId and originMessageId",
        ));
    }
    Ok(())
}

pub(super) fn require_unique_statement(
    tx: &Transaction<'_>,
    loupist: &str,
    statement: &str,
    except: Option<&str>,
) -> KnowledgeResult<()> {
    let found:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_lens_metadata WHERE loupist_id=?1 AND kind='principle' AND archived_at_ms IS NULL AND principle_statement_fold=?2 AND record_id<>?3)",params![loupist,super::lens::fold_lens_title(statement),except.unwrap_or("")],|r|r.get(0))?;
    if found {
        Err(KnowledgeError::DuplicatePrinciple)
    } else {
        Ok(())
    }
}

pub fn principle_list(
    tx: &Transaction<'_>,
    loupist: &str,
    include_archived: bool,
) -> KnowledgeResult<Vec<Principle>> {
    super::model::validate_loupist_id(loupist)?;
    let mut query = tx.prepare("SELECT m.record_id,h.revision_id,r.payload_json,m.created_at_ms,meta.updated_at_ms,meta.archived_at_ms IS NOT NULL FROM knowledge_records m JOIN knowledge_record_heads h ON h.record_id=m.record_id JOIN knowledge_record_revisions r ON r.revision_id=h.revision_id JOIN knowledge_lens_metadata meta ON meta.record_id=m.record_id WHERE m.owner_loupist_id=?1 AND meta.kind='principle' AND (?2 OR meta.archived_at_ms IS NULL) ORDER BY meta.updated_at_ms DESC,m.record_id")?;
    let rows = query.query_map(params![loupist, include_archived], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, bool>(5)?,
        ))
    })?;
    let removed = super::memory_controls::load(tx, loupist)?.removed;
    rows.filter_map(|row| match row {
        Ok(row) if removed.contains(&row.0) => None,
        other => Some(other),
    })
    .map(|row| {
        let (record_id, revision_id, json, created_at_ms, updated_at_ms, archived) = row?;
        let input: PrincipleInput = serde_json::from_str(&json)?;
        Ok(Principle {
            record_id,
            revision_id,
            loupist_id: loupist.into(),
            statement: input.statement,
            why: input.why,
            example: input.example,
            origin: input.origin,
            origin_chat_id: input.origin_chat_id,
            origin_message_id: input.origin_message_id,
            created_at_ms,
            updated_at_ms,
            archived,
        })
    })
    .collect()
}

pub fn principle_read(tx: &Transaction<'_>, loupist: &str, id: &str) -> KnowledgeResult<Principle> {
    principle_list(tx, loupist, true)?
        .into_iter()
        .find(|p| p.record_id == id)
        .ok_or(KnowledgeError::NotFound)
}

/// Caller owns the transaction, so creation, revision, metadata, and growth publish together.
pub fn principle_save(
    tx: &Transaction<'_>,
    loupist: &str,
    id: Option<&str>,
    expected: Option<&str>,
    mut input: PrincipleInput,
    at_ms: i64,
) -> KnowledgeResult<Principle> {
    validate_principle(&mut input)?;
    super::model::validate_time(at_ms)?;
    let title: String = input
        .statement
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect();
    let creating = expected.is_none();
    require_unique_statement(tx, loupist, &input.statement, id)?;
    let record_id = if creating {
        super::catalog::create_principle_record(
            tx,
            super::CreateRecord {
                loupist_id: loupist.into(),
                kind: RecordKind::Lens,
                title: title.clone(),
                at_ms,
            },
            id.map(str::to_owned),
        )?
        .record_id
    } else {
        let id = id.ok_or(KnowledgeError::NotFound)?;
        principle_read(tx, loupist, id)?;
        id.to_owned()
    };
    let before = if creating {
        None
    } else {
        Some(principle_read(tx, loupist, &record_id)?)
    };
    if let Some(previous) = &before {
        if expected != Some(previous.revision_id.as_str()) {
            return Err(KnowledgeError::RevisionConflict {
                expected: expected.map(str::to_owned),
                actual: Some(previous.revision_id.clone()),
            });
        }
        if previous.statement == input.statement
            && previous.why == input.why
            && previous.example == input.example
            && previous.origin == input.origin
            && previous.origin_chat_id == input.origin_chat_id
            && previous.origin_message_id == input.origin_message_id
        {
            return Ok(previous.clone());
        }
    }
    let record = super::authority::require_record(tx, loupist, &record_id)?;
    super::authority::require_edit(&record, loupist)?;
    for text in [
        Some(&input.statement),
        input.why.as_ref(),
        input.example.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        super::lens::check_text(tx, super::lens_page::LensKind::Principle, text)?;
    }
    let body = super::lens_page::LensBody::Principle(input.clone());
    let payload = body.to_payload()?;
    let saved =
        super::revision::publish_revision(tx, &record, loupist, expected, &payload, &[], at_ms)?;
    tx.execute("UPDATE knowledge_lens_metadata SET title=?1,title_fold=?2,updated_at_ms=?3,principle_statement_fold=?5 WHERE record_id=?4",params![title,super::lens::fold_lens_title(&title),at_ms,record_id,super::lens::fold_lens_title(&input.statement)])?;
    super::search_index::index_payload(
        tx,
        None,
        &record_id,
        &saved.revision_id,
        RecordKind::Lens,
        &title,
        &payload,
    )?;
    let mut event = super::growth::GrowthEvent::new(
        loupist,
        &record_id,
        at_ms,
        if creating {
            "lens_created"
        } else {
            "lens_revised"
        },
        "user",
        &input.statement,
    );
    event.origin = Some(serde_json::json!(input.origin));
    event.origin_chat_id = input.origin_chat_id.clone();
    event.origin_message_id = input.origin_message_id.clone();
    event.before_summary = before
        .as_ref()
        .map(|p| super::growth::one_line(&p.statement));
    event.after_summary = Some(super::growth::one_line(&input.statement));
    event.insert(tx)?;
    super::lens::log_principle(tx, loupist, &record_id, &input, before.as_ref(), at_ms)?;
    principle_read(tx, loupist, &record_id)
}

pub fn principle_archive(
    tx: &Transaction<'_>,
    loupist: &str,
    id: &str,
    archived: bool,
    at_ms: i64,
) -> KnowledgeResult<Principle> {
    principle_read(tx, loupist, id)?;
    super::lens::set_lens_archived(
        tx,
        loupist,
        id,
        archived,
        "User edited principle visibility",
        at_ms,
        "user",
    )?;
    principle_read(tx, loupist, id)
}

/// Includes the overflow notice in the same character budget as the block.
pub fn principles_context(principles: &[Principle]) -> (String, Vec<Principle>) {
    let mut active: Vec<_> = principles.iter().filter(|p| !p.archived).collect();
    active.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then(a.record_id.cmp(&b.record_id))
    });
    let mut selected = Vec::new();
    let mut lines = Vec::new();
    let header = "User principles\n";
    for principle in active.iter().take(30) {
        let why = principle
            .why
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let line = format!(
            "- [lens:{}] {} — {}",
            principle.record_id,
            principle
                .statement
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            why
        );
        let remaining = active.len() - selected.len() - 1;
        let notice = if remaining > 0 {
            format!("\n{remaining} more principles are saved; use lens_search to find them.")
        } else {
            String::new()
        };
        if header.chars().count()
            + lines
                .iter()
                .map(|l: &String| l.chars().count() + 1)
                .sum::<usize>()
            + line.chars().count()
            + notice.chars().count()
            > 6000
        {
            break;
        }
        lines.push(line);
        selected.push((*principle).clone());
    }
    let remaining = active.len() - selected.len();
    if remaining > 0 {
        lines.push(format!(
            "{remaining} more principles are saved; use lens_search to find them."
        ));
    }
    (
        if lines.is_empty() {
            String::new()
        } else {
            format!("{header}{}", lines.join("\n"))
        },
        selected,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{GrowthEventsQuery, KnowledgeStore};
    fn input(statement: &str) -> PrincipleInput {
        PrincipleInput {
            statement: statement.into(),
            why: None,
            example: None,
            origin: "settings".into(),
            origin_chat_id: None,
            origin_message_id: None,
        }
    }
    fn save(store: &KnowledgeStore, statement: &str, at_ms: i64) -> KnowledgeResult<Principle> {
        store.transact(|w| principle_save(w.tx, "g", None, None, input(statement), at_ms))
    }
    #[test]
    fn principle_crud_validates_unicode_lengths_conflicts_origins_and_archive() {
        let temp = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(temp.path().join("memory.sqlite")).unwrap();
        let mut too_long_why = input("Check cash");
        too_long_why.why = Some("원".repeat(501));
        for invalid in [input(" "), input(&"원".repeat(281)), too_long_why] {
            assert!(store
                .transact(|w| principle_save(w.tx, "g", None, None, invalid, 1))
                .is_err());
        }
        let mut accepted = input(&format!("  {}  ", "원".repeat(280)));
        accepted.why = Some("원".repeat(500));
        let p = store
            .transact(|w| principle_save(w.tx, "g", None, None, accepted, 1))
            .unwrap();
        assert_eq!(p.statement.chars().count(), 280);
        let mut feedback = input("Check cash flow before trusting profit");
        feedback.origin = "feedback".into();
        assert!(validate_principle(&mut feedback).is_err());
        feedback.origin_chat_id = Some("chat-a".into());
        feedback.origin_message_id = Some("message-a".into());
        let edit = |expected: &str, value: PrincipleInput, at_ms| {
            store.transact(|w| {
                principle_save(w.tx, "g", Some(&p.record_id), Some(expected), value, at_ms)
            })
        };
        let p = edit(&p.revision_id, feedback, 2).unwrap();
        let newer = edit(&p.revision_id, input("Check operating cash flow"), 3).unwrap();
        assert!(matches!(
            edit(&p.revision_id, input("Old edit"), 4),
            Err(KnowledgeError::RevisionConflict { .. })
        ));
        assert!(store
            .read(|w| principle_read(w.tx, "other", &p.record_id))
            .is_err());
        let archived = store
            .transact(|w| principle_archive(w.tx, "g", &p.record_id, true, 5))
            .unwrap();
        assert!(archived.archived);
        assert!(store
            .read(|w| principle_list(w.tx, "g", false))
            .unwrap()
            .is_empty());
        let lens = store.read(|w| w.read_page("g", &p.record_id)).unwrap();
        assert_eq!(lens.body.summary(), newer.statement);
        assert_eq!(lens.kind, crate::knowledge::lens_page::LensKind::Principle);
        assert_eq!(lens.history[0].origin, "user");
        // User-authored principles keep trade-action wording.
        save(&store, "You should buy when cash flow improves", 7).unwrap();
        let growth = store
            .growth_events(&GrowthEventsQuery {
                loupist_id: "g".into(),
                since_ms: None,
                until_ms: None,
                limit: None,
                cursor: None,
            })
            .unwrap();
        assert!(growth.events.iter().all(|e| e.actor == "user"));
        assert!(growth
            .events
            .iter()
            .any(|e| e.origin_chat_id.as_deref() == Some("chat-a")
                && e.origin_message_id.as_deref() == Some("message-a")));
    }
    #[test]
    fn principle_duplicates_use_full_normalized_statement_not_title() {
        let temp = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(temp.path().join("memory.sqlite")).unwrap();
        let prefix = "a".repeat(80);
        let first = save(&store, &format!("{prefix} first condition"), 1).unwrap();
        save(&store, &format!("{prefix} second condition"), 2).unwrap();
        let duplicate = format!("{}   FIRST  CONDITION", prefix.to_uppercase());
        assert!(matches!(
            save(&store, &duplicate, 3),
            Err(KnowledgeError::DuplicatePrinciple)
        ));
        store
            .transact(|w| principle_archive(w.tx, "g", &first.record_id, true, 4))
            .unwrap();
        save(&store, &first.statement, 5).unwrap();
        assert!(matches!(
            store.transact(|w| principle_archive(w.tx, "g", &first.record_id, false, 6)),
            Err(KnowledgeError::DuplicatePrinciple)
        ));
    }

    #[test]
    fn principle_context_orders_caps_and_excludes_archived() {
        let temp = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(temp.path().join("memory.sqlite")).unwrap();
        for i in 0..35 {
            save(&store, &format!("Rule {i}: Check cash flow"), i).unwrap();
        }
        let all = store.read(|w| principle_list(w.tx, "g", false)).unwrap();
        let (text, shown) = principles_context(&all);
        assert_eq!(shown.len(), 30);
        assert_eq!(shown[0].statement, "Rule 34: Check cash flow");
        assert!(text.chars().count() <= 6000);
        let mut big = all.clone();
        for p in &mut big {
            p.why = Some("원".repeat(500));
        }
        big[0].archived = true;
        let (text, shown) = principles_context(&big);
        assert!(shown.len() < 30);
        assert!(!text.contains(&big[0].record_id));
        assert!(text.chars().count() <= 6000);
    }
    #[test]
    fn principle_rollback_and_file_copy_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.sqlite");
        let store = KnowledgeStore::open(&path).unwrap();
        let result: KnowledgeResult<()> = store.transact(|w| {
            principle_save(w.tx, "g", None, None, input("Rolled back rule"), 1)?;
            Err(KnowledgeError::Invalid("failed turn"))
        });
        assert!(result.is_err());
        assert!(store
            .read(|w| principle_list(w.tx, "g", true))
            .unwrap()
            .is_empty());
        let p = save(&store, "Check cash flow", 2).unwrap();
        drop(store);
        let copied = temp.path().join("copied.sqlite");
        std::fs::copy(path, &copied).unwrap();
        let reopened = KnowledgeStore::open(copied).unwrap();
        let restored = reopened
            .read(|w| principle_read(w.tx, "g", &p.record_id))
            .unwrap();
        assert_eq!(p, restored);
    }
}
