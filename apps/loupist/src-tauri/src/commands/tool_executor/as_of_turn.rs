//! An as-of turn's record of what it could not use (finance-point-in-time.md
//! §3.3): reads a tool refused under the cutoff (`as_of_unsupported`), reads
//! with nothing dated by then (`as_of_empty`) and saved Memory left out.
use super::*;
use crate::commands::knowledge::as_of::{Cutoff, LeftOut};
use crate::domain::{AsOfEmpty, AsOfRefusal, AsOfReport, ChatToolTrace, MAX_AS_OF_ITEMS};

/// Argument fields that name what a read was about, in preference order.
const TARGET_FIELDS: &[&str] = &[
    "symbol",
    "ticker",
    "stock_code",
    "corp_code",
    "corp_name",
    "company",
    "series",
    "indicator",
    "code",
    "query",
    "url",
];

fn trace_code(entry: &crate::domain::ChatToolTraceEntry) -> Option<&str> {
    entry
        .error
        .as_ref()
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
}

fn target(args: Option<&Value>) -> Option<String> {
    let args = args?;
    TARGET_FIELDS.iter().find_map(|field| {
        let value = args.get(*field)?;
        let text = match value {
            Value::String(text) => text.trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => return None,
        };
        (!text.is_empty()).then(|| crate::domain::answer_checks::clip(&text, 120))
    })
}

impl AppToolExecutor {
    /// Leaves out search hits the turn's cutoff did not know, and counts them.
    pub(super) fn as_of_hits(
        &self,
        policy: &ToolPolicy,
        hits: &mut Value,
    ) -> Result<(), BrokerError> {
        self.as_of_hits_within(policy, hits, usize::MAX)
    }

    /// As `as_of_hits`, but counts only what the cutoff removed from the first
    /// `window` hits: those the read would otherwise have shown.
    pub(super) fn as_of_hits_within(
        &self,
        policy: &ToolPolicy,
        hits: &mut Value,
        window: usize,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) = Cutoff::parse(policy.as_of.as_deref()) else {
            return Ok(());
        };
        let Some(rows) = hits.as_array_mut() else {
            return Ok(());
        };
        let filter = |rows: &mut Vec<Value>, left: &mut LeftOut| {
            crate::commands::knowledge::as_of::filter_hits(
                &self.state,
                &policy.loupist_id,
                rows,
                cutoff,
                left,
            )
            .map_err(command_error)
        };
        let mut rest = rows.split_off(window.min(rows.len()));
        let mut left = LeftOut::default();
        filter(rows, &mut left)?;
        if !rest.is_empty() {
            filter(&mut rest, &mut LeftOut::default())?;
            rows.append(&mut rest);
        }
        self.capture.note_left_out(left);
        Ok(())
    }
}

/// A saved item the cutoff leaves out: the read is refused with `as_of_later`.
pub(super) fn later_than(cutoff: Cutoff, what: &str) -> BrokerError {
    BrokerError::Execution(format!(
        "as_of_later: this {what} was saved or published after the as-of date {}, so it is left out of an answer about what was known then",
        cutoff.day()
    ))
}

impl ToolCapture {
    /// The cutoff this turn answers under, if any.
    pub(in crate::commands) fn cutoff(&self) -> Option<Cutoff> {
        Cutoff::parse(self.turn_as_of.as_deref())
    }

    /// Records Memory a cutoff filter removed.
    pub(in crate::commands) fn note_left_out(&self, left: LeftOut) {
        self.as_of_left_out
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .merge(left);
    }

    /// What this as-of turn could not use, from its tool trace and filters.
    /// `None` for a turn without a cutoff.
    pub(in crate::commands) fn as_of_report(&self, trace: &ChatToolTrace) -> Option<AsOfReport> {
        let cutoff = self.cutoff()?;
        let mut refused: Vec<AsOfRefusal> = Vec::new();
        let mut empty: Vec<AsOfEmpty> = Vec::new();
        for entry in &trace.entries {
            match trace_code(entry) {
                Some("as_of_unsupported") => {
                    let item = AsOfRefusal {
                        tool: crate::domain::answer_checks::clip(&entry.name, 128),
                        connection: entry
                            .component
                            .as_deref()
                            .map(|name| crate::domain::answer_checks::clip(name, 256)),
                    };
                    if !refused.contains(&item) && refused.len() < MAX_AS_OF_ITEMS {
                        refused.push(item);
                    }
                }
                Some("as_of_empty") => {
                    let item = AsOfEmpty {
                        tool: crate::domain::answer_checks::clip(&entry.name, 128),
                        target: target(entry.args.as_ref()),
                    };
                    if !empty.contains(&item) && empty.len() < MAX_AS_OF_ITEMS {
                        empty.push(item);
                    }
                }
                _ => {}
            }
        }
        let left_out = self
            .as_of_left_out
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .counts();
        Some(AsOfReport {
            cutoff: cutoff.day(),
            refused,
            empty,
            left_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChatToolTraceEntry, ToolTraceOutcome};

    fn entry(name: &str, code: &str, args: Value) -> ChatToolTraceEntry {
        ChatToolTraceEntry {
            call_id: Some(format!("call-{name}")),
            name: name.into(),
            component: None,
            args: Some(args),
            start_offset_ms: 0,
            duration_ms: 1,
            outcome: ToolTraceOutcome::Error,
            error: Some(json!({"code": code, "message": "m"})),
            result_bytes: None,
        }
    }

    #[test]
    fn the_report_lists_refusals_empty_reads_and_left_out_memory() {
        let mut capture = ToolCapture::for_chat("assistant".into());
        let trace = ChatToolTrace {
            entries: vec![
                entry(
                    "kis_read",
                    "as_of_unsupported",
                    json!({"operation": "quote"}),
                ),
                entry(
                    "kis_read",
                    "as_of_unsupported",
                    json!({"operation": "quote"}),
                ),
                entry(
                    "krx_daily_quote",
                    "as_of_empty",
                    json!({"symbol": "990221"}),
                ),
                entry("web_fetch", "offline", json!({})),
            ],
            truncated: false,
            untrusted_text: false,
        };
        assert!(capture.as_of_report(&trace).is_none());
        assert!(!capture.as_of_turn());
        capture.turn_as_of = Some("2026-03-03".into());
        // The turn's own date makes it an as-of turn before any tool runs.
        assert!(capture.as_of_turn());
        let mut left = LeftOut::default();
        left.sources.insert("src-1".into());
        left.facts.extend(["c1".to_owned(), "c2".to_owned()]);
        capture.note_left_out(left.clone());
        capture.note_left_out(left);
        let report = capture.as_of_report(&trace).unwrap();
        assert_eq!(report.cutoff, "2026-03-03");
        assert_eq!(report.refused.len(), 1);
        assert_eq!(report.refused[0].tool, "kis_read");
        assert_eq!(report.empty[0].target.as_deref(), Some("990221"));
        assert_eq!((report.left_out.facts, report.left_out.sources), (2, 1));
    }
}
