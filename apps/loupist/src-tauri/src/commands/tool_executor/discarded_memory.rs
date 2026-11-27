//! Receipt names from the broker's acknowledged staging state.
use super::*;
use crate::{
    domain::{
        DiscardedMemoryItem, DiscardedMemoryKind as Kind, MAX_DISCARDED_MEMORY_ITEMS,
        MAX_DISCARDED_MEMORY_TITLE_CHARS,
    },
    knowledge::lens_page::{LensKind, LensOp, PageKind, PageTarget},
};

#[derive(Default)]
struct Items(Vec<DiscardedMemoryItem>);

fn title_excerpt(title: &str, limit: usize) -> String {
    let mut chars = title.trim().chars();
    let mut excerpt: String = chars
        .by_ref()
        .take(limit)
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if chars.next().is_some() {
        excerpt.pop();
        excerpt.push('…');
    }
    excerpt.trim().to_owned()
}

impl Items {
    fn push(&mut self, kind: Kind, title: &str) {
        if self.0.len() == MAX_DISCARDED_MEMORY_ITEMS {
            return;
        }
        let title = title_excerpt(title, MAX_DISCARDED_MEMORY_TITLE_CHARS);
        if !title.is_empty() {
            let item = DiscardedMemoryItem {
                kind,
                title,
                reason: None,
            };
            if !self.0.contains(&item) {
                self.0.push(item);
            }
        }
    }

    fn filing(&mut self, filing: &StagedFiling, reads: &[StagedKnowledgeRead]) {
        for delta in &filing.deltas {
            let mut old_page = None;
            let (title, method) = match &delta.target {
                PageTarget::Create { title, kind, .. } => (title, *kind == PageKind::Method),
                PageTarget::Existing { record_id } => {
                    let Some(read) = reads.iter().rev().find(|read| {
                        read.record_id == *record_id
                            && matches!(read.payload, AckedPayload::Lens { .. })
                    }) else {
                        continue;
                    };
                    if let AckedPayload::Lens { document_json, .. } = &read.payload {
                        old_page =
                            serde_json::from_str::<crate::knowledge::PageRead>(document_json).ok();
                    }
                    (
                        &read.title,
                        old_page
                            .as_ref()
                            .is_some_and(|page| page.kind == LensKind::Method),
                    )
                }
            };
            for op in &delta.ops {
                let heading = match op {
                    LensOp::Put { heading, .. } => Some(heading.as_str()),
                    LensOp::Retire { key, .. } => Some(
                        old_page
                            .as_ref()
                            .and_then(|page| {
                                page.body
                                    .sections()
                                    .iter()
                                    .find(|section| section.key == *key)
                            })
                            .map_or(key.as_str(), |section| section.heading.as_str()),
                    ),
                    LensOp::Summary { .. } | LensOp::IdeaStatus { .. } => None,
                };
                let kind = match (method, heading.is_some()) {
                    (true, _) => Kind::Method,
                    (false, true) => Kind::LensSection,
                    (false, false) => Kind::LensPage,
                };
                match heading {
                    Some(heading) => self.push(
                        kind,
                        &format!(
                            "{} — {}",
                            title_excerpt(title, 95),
                            title_excerpt(heading, 62)
                        ),
                    ),
                    None => self.push(kind, title),
                }
            }
        }
    }
}

pub(in crate::commands) fn discarded_price_reads<'a>(
    titles: impl IntoIterator<Item = &'a str>,
) -> Option<Vec<DiscardedMemoryItem>> {
    let mut items = Items::default();
    for title in titles {
        items.push(Kind::PriceRead, title);
    }
    (!items.0.is_empty()).then_some(items.0)
}

impl ToolCapture {
    pub(in crate::commands) fn discarded_memory_items(
        &self,
        reads: &[StagedKnowledgeRead],
    ) -> Option<Vec<DiscardedMemoryItem>> {
        let mut items = Items::default();
        if let Ok(research) = self.staged_research.try_lock() {
            if let Some(research) = research.as_ref() {
                if let Some(note) = &research.working_note {
                    items.push(Kind::Note, &note.title);
                }
                for journal in [&research.journal, &research.thesis_pair]
                    .into_iter()
                    .flatten()
                {
                    // Journal cards and save receipts use the judgment as the title.
                    items.push(Kind::JournalEntry, &journal.judgment);
                }
            }
        }
        if let Ok(principles) = self.staged_principles.try_lock() {
            for principle in principles.iter() {
                items.push(
                    Kind::Principle,
                    &crate::knowledge::principles::principle_title(&principle.input.statement),
                );
            }
        }
        if let Ok(sources) = self.staged_sources.try_lock() {
            match crate::commands::knowledge::current_paste_saves(&sources) {
                Ok(current) => {
                    for (_, source) in current {
                        items.push(Kind::Source, &source.title);
                    }
                }
                Err(_) => {
                    for source in sources.iter() {
                        items.push(Kind::Source, &source.title);
                    }
                }
            }
        }
        if let Ok(filings) = self.staged_filings.try_lock() {
            for filing in filings.iter() {
                items.filing(filing, reads);
            }
        }
        if let Ok(vocabulary) = self.staged_vocabulary.try_lock() {
            for staged in vocabulary.iter() {
                let labels = staged
                    .types
                    .iter()
                    .map(|item| &item.labels)
                    .chain(staged.relations.iter().map(|item| &item.labels));
                for labels in labels {
                    let title = match self.interface_locale {
                        Some(crate::agent_harness::interface::InterfaceLocale::Korean) => {
                            labels.ko.as_deref().unwrap_or(&labels.en)
                        }
                        _ => &labels.en,
                    };
                    items.push(Kind::WorldVocabulary, title);
                }
            }
        }
        if let Ok(setup) = self.staged_setup.try_lock() {
            if let Some(setup) = setup.as_ref() {
                if let Some(focus) = &setup.focus {
                    items.push(Kind::ResearchFocus, focus);
                }
                if let Some(preset) = setup.preset {
                    items.push(Kind::WorldVocabulary, preset.as_str());
                }
                for asset in setup.follow.iter().chain(&setup.unfollow) {
                    items.push(Kind::Watchlist, &super::setup::watchlist_label(asset));
                }
            }
        }
        if let Ok(theses) = self.staged_theses.try_lock() {
            for (title, _) in theses.iter() {
                items.push(Kind::Thesis, title);
            }
        }
        if let Ok(prices) = self.staged_price_reads.try_lock() {
            for (asset, _) in prices.iter() {
                items.push(Kind::PriceRead, &asset.name);
            }
        }
        if let Ok(bot_memory) = self.staged_bot_memory.try_lock() {
            if bot_memory.has_delivered() {
                // The Bot's memory entries have bodies and no titles.
                items.push(Kind::BotMemory, "Bot");
            }
        }
        if let Ok(handoff) = self.staged_handoff.try_lock() {
            if let Some(handoff) = handoff.as_ref() {
                items.push(Kind::ResearchHandoff, &handoff.request.title);
            }
        }
        (!items.0.is_empty()).then_some(items.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::lens_page::{PageDelta, SectionKey};

    #[tokio::test]
    async fn only_the_latest_acknowledged_write_supplies_the_title() {
        let capture = ToolCapture::default();
        for (delivery, title) in [
            ("first", "Old statement"),
            ("replacement", "Current  statement"),
            ("unacknowledged", "Undelivered statement"),
        ] {
            capture
                .stage(delivery, |pending| {
                    pending.principles.push(StagedPrinciple {
                        record_id: "same-principle".into(),
                        expected_revision_id: None,
                        input: crate::knowledge::PrincipleInput {
                            statement: title.into(),
                            why: None,
                            example: None,
                            origin: "settings".into(),
                            origin_chat_id: None,
                            origin_message_id: None,
                        },
                    });
                })
                .await;
        }
        assert!(capture.discarded_memory_items(&[]).is_none());
        capture.commit_delivery("first").await;
        capture.commit_delivery("replacement").await;
        assert_eq!(
            capture.discarded_memory_items(&[]),
            Some(vec![DiscardedMemoryItem {
                kind: Kind::Principle,
                title: "Current statement".into(),
                reason: None,
            }])
        );
    }

    #[test]
    fn lens_section_names_include_the_page_and_keep_no_markdown() {
        let filing = StagedFiling {
            deltas: vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Method,
                    title: "Cash flow checks".into(),
                    anchors: vec![],
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("overlap").unwrap(),
                    heading: "Overlap".into(),
                    markdown: "This body stays out of the receipt.".into(),
                    basis: Default::default(),
                    expected: None,
                }],
            }],
            pending_spans: vec![],
            rationale: "This rationale stays out of the receipt.".into(),
            said_ops: Default::default(),
        };
        let mut items = Items::default();
        items.filing(&filing, &[]);
        assert_eq!(
            items.0,
            vec![DiscardedMemoryItem {
                kind: Kind::Method,
                title: "Cash flow checks — Overlap".into(),
                reason: None,
            }]
        );
    }

    #[test]
    fn receipt_titles_are_bounded_on_character_boundaries_and_deduplicated() {
        let mut items = Items::default();
        let long = format!("거래량\n{}", "가".repeat(200));
        items.push(Kind::Note, &long);
        items.push(Kind::Note, &long);
        assert_eq!(items.0.len(), 1);
        assert_eq!(
            items.0[0].title.chars().count(),
            MAX_DISCARDED_MEMORY_TITLE_CHARS
        );
        assert!(items.0[0].title.ends_with('…'));
        assert!(!items.0[0].title.contains('\n'));
        for number in 0..64 {
            items.push(Kind::Source, &format!("Source {number}"));
        }
        assert_eq!(items.0.len(), MAX_DISCARDED_MEMORY_ITEMS);
        assert!(crate::domain::validate_discarded_memory_items(Some(&items.0), true).is_ok());
        assert!(crate::domain::validate_discarded_memory_items(Some(&items.0), false).is_err());
    }
}
