//! Shared validation for references carried by notes, Journals and Lens changes.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ReceiptPurpose {
    Research,
    LensChange,
}

/// Only acknowledged identifiers are copied; discovery and pending deliveries
/// never enter this ledger. Copying identifiers avoids holding capture locks
/// during database reads or cloning retained document bodies.
pub(super) struct ReceiptLedger {
    purpose: ReceiptPurpose,
    reads: BTreeMap<String, String>,
    sources: BTreeSet<String>,
    results: BTreeSet<String>,
}

impl AppToolExecutor {
    pub(super) async fn acknowledged_receipts(&self, purpose: ReceiptPurpose) -> ReceiptLedger {
        let mut reads: BTreeMap<_, _> = self
            .capture
            .staged_reads
            .lock()
            .await
            .iter()
            .map(|read| (read.read_ref.clone(), read.scope.clone()))
            .collect();
        if matches!(purpose, ReceiptPurpose::Research) {
            reads.extend(
                self.capture
                    .context_reads
                    .lock()
                    .await
                    .iter()
                    .map(|read| (read.read_ref.clone(), read.scope.clone())),
            );
        }
        let sources = self
            .capture
            .staged_sources
            .lock()
            .await
            .iter()
            .map(|source| source.request_ref.clone())
            .collect();
        let results = self
            .capture
            .run_results
            .lock()
            .await
            .results
            .keys()
            .cloned()
            .collect();
        ReceiptLedger {
            purpose,
            reads,
            sources,
            results,
        }
    }
}

impl ReceiptLedger {
    pub(super) fn validate(
        &self,
        scope: &str,
        field: &str,
        references: &[String],
    ) -> Result<(), BrokerError> {
        for reference in references {
            if self
                .reads
                .get(reference)
                .is_some_and(|found| found == scope)
                || (scope == "source" && self.sources.contains(reference))
            {
                continue;
            }
            let source_field = self.reference_field("source", field);
            // Echo only identifiers found in the host's acknowledged ledger,
            // never arbitrary model text that happens to resemble a reference.
            let known_reference: String = reference.chars().take(128).collect();
            let expected = if let Some(found) = self.reads.get(reference) {
                let kind = if found == "research" {
                    "Journal"
                } else {
                    found
                };
                if found == "lens" && matches!(self.purpose, ReceiptPurpose::LensChange) {
                    format!("{known_reference} is a Lens read; change evidence accepts sourceReads, worldReads, or journalReads")
                } else {
                    format!(
                        "{known_reference} is a {kind} read; cite it in {}",
                        self.reference_field(found, field)
                    )
                }
            } else if self.sources.contains(reference) {
                format!("{known_reference} is a source saveRef; cite it in {source_field}")
            } else if self.results.contains(reference) {
                format!("{known_reference} is a result_ref; save it with source_save and cite the returned saveRef in {source_field}")
            } else {
                return Err(BrokerError::Execution(format!("{scope} reference must name an acknowledged readRef or source saveRef from this turn")));
            };
            return Err(BrokerError::invalid(field, expected));
        }
        Ok(())
    }

    fn reference_field(&self, scope: &str, supplied_field: &str) -> String {
        let field = match scope {
            "source" => "sourceReads",
            "world" => "worldReads",
            "lens" => "lensReads",
            "research" if matches!(self.purpose, ReceiptPurpose::LensChange) => "journalReads",
            "research" => "journal.earlier[].readRef",
            _ => "the matching reference field",
        };
        if supplied_field.starts_with("journal.") && field.ends_with("Reads") {
            format!("journal.{field}")
        } else {
            field.to_owned()
        }
    }
}
