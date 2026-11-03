use std::collections::{BTreeMap, BTreeSet};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{
    validate_text, validate_token, BindingRole, ClaimStatus, GraphClaim, GraphNode,
    WorldSnapshotBody,
};

pub(super) struct WorldFilters<'a> {
    pub search: &'a str,
    pub kinds: &'a [String],
    pub relations: &'a [String],
    pub binding_roles: &'a [BindingRole],
    pub statuses: &'a [ClaimStatus],
    pub publication_times: &'a [String],
    /// An as-of cutoff: undated relationships and ones published later are left out.
    pub published_by: Option<&'a str>,
}

pub(super) struct FilteredWorld<'a> {
    pub nodes: BTreeMap<&'a str, &'a GraphNode>,
    pub claims: Vec<&'a GraphClaim>,
}

impl WorldFilters<'_> {
    pub fn validate(&self) -> KnowledgeResult<()> {
        validate_text(self.search, 200, true, "search is invalid")?;
        if self.kinds.len() > 256
            || self.relations.len() > 256
            || self.publication_times.len() > 256
            || self.binding_roles.len() > 5
            || self.statuses.len() > 3
        {
            return Err(KnowledgeError::Invalid("world filters are invalid"));
        }
        for kind in self.kinds {
            validate_token(kind, 128, "node kind is invalid")?;
        }
        for relation in self.relations {
            validate_token(relation, 64, "relation is invalid")?;
        }
        for time in self.publication_times {
            validate_text(time, 64, false, "publication filter is invalid")?;
        }
        if self
            .published_by
            .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err())
        {
            return Err(KnowledgeError::Invalid("as-of cutoff is invalid"));
        }
        Ok(())
    }
}

pub(super) fn filter<'a>(
    body: &'a WorldSnapshotBody,
    filters: &WorldFilters<'_>,
) -> FilteredWorld<'a> {
    let search = filters.search.trim().to_lowercase();
    let mut nodes: BTreeMap<_, _> = body
        .nodes
        .iter()
        .filter(|node| filters.kinds.is_empty() || filters.kinds.contains(&node.kind))
        .map(|node| (node.id.as_str(), node))
        .collect();
    let search_nodes: BTreeSet<_> = nodes
        .values()
        .filter(|node| node_matches(node, &search))
        .map(|node| node.id.as_str())
        .collect();
    let claims: Vec<_> = body
        .claims
        .iter()
        .filter(|claim| {
            claim_matches(claim, filters)
                && claim
                    .endpoints
                    .iter()
                    .all(|endpoint| nodes.contains_key(endpoint.node_id.as_str()))
                && (search.is_empty()
                    || claim_matches_search(claim, &search)
                    || claim
                        .endpoints
                        .iter()
                        .any(|endpoint| search_nodes.contains(endpoint.node_id.as_str())))
        })
        .collect();
    let claim_filter = !filters.relations.is_empty()
        || !filters.binding_roles.is_empty()
        || !filters.statuses.is_empty()
        || !filters.publication_times.is_empty()
        || filters.published_by.is_some();
    // Only direct neighbours of actual search matches are added, independent
    // of input order. Every retained relationship keeps all of its endpoints.
    let mut visible = if claim_filter {
        BTreeSet::new()
    } else {
        search_nodes
    };
    for claim in &claims {
        visible.extend(
            claim
                .endpoints
                .iter()
                .map(|endpoint| endpoint.node_id.as_str()),
        );
    }
    nodes.retain(|id, _| visible.contains(id));
    FilteredWorld { nodes, claims }
}

fn node_matches(node: &GraphNode, search: &str) -> bool {
    search.is_empty()
        || [&node.name, &node.description, &node.kind]
            .into_iter()
            .chain(node.aliases.iter())
            .any(|text| text.to_lowercase().contains(search))
}

fn claim_matches(claim: &GraphClaim, filters: &WorldFilters<'_>) -> bool {
    (filters.relations.is_empty() || filters.relations.contains(&claim.relation))
        && (filters.statuses.is_empty() || filters.statuses.contains(&claim.status))
        && (filters.binding_roles.is_empty()
            || claim
                .evidence
                .iter()
                .any(|item| filters.binding_roles.contains(&item.role)))
        && (filters.publication_times.is_empty()
            || filters
                .publication_times
                .iter()
                .any(|value| value == publication_value(claim)))
        && filters
            .published_by
            .is_none_or(|cutoff| published_by(claim.publication_time.as_ref(), cutoff))
}

/// Whether a publication time falls on or before `cutoff` (`YYYY-MM-DD`). A
/// month or year counts at its last day; an undated value never does.
pub(crate) fn published_by(time: Option<&super::model::TimePoint>, cutoff: &str) -> bool {
    let Some(value) = time.and_then(|time| time.value.as_deref()) else {
        return false;
    };
    let Ok(cutoff) = chrono::NaiveDate::parse_from_str(cutoff, "%Y-%m-%d") else {
        return false;
    };
    last_day(value).is_some_and(|day| day <= cutoff)
}

/// The last calendar day a date-like value can mean (`2026-03-18…`, `2026-03`, `2026`).
pub(crate) fn last_day(value: &str) -> Option<chrono::NaiveDate> {
    use chrono::NaiveDate;
    let value = value.trim();
    if let Some(day) = value
        .get(..10)
        .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
    {
        return Some(day);
    }
    if let Some(month) = value
        .get(..7)
        .filter(|month| month.as_bytes().get(4) == Some(&b'-'))
    {
        let first = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok()?;
        let next = first.checked_add_months(chrono::Months::new(1))?;
        return next.pred_opt();
    }
    let year: i32 = value.get(..4)?.parse().ok()?;
    NaiveDate::from_ymd_opt(year, 12, 31)
}

fn claim_matches_search(claim: &GraphClaim, search: &str) -> bool {
    [&claim.proposition, &claim.attribution, &claim.relation]
        .into_iter()
        .chain(claim.qualifiers.iter().map(|item| &item.value))
        .any(|text| text.to_lowercase().contains(search))
}

pub(super) fn publication_value(claim: &GraphClaim) -> &str {
    claim
        .publication_time
        .as_ref()
        .and_then(|time| time.value.as_deref())
        .unwrap_or("undated")
}
