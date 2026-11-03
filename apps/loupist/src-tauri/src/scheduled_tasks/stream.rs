//! A bounded projection of an app-owned Chat stream for reconnecting the UI.
//! Progress upserts and text deltas are compacted; tool payloads remain in the
//! ordinary activity store rather than being copied into scheduler history.
use crate::{
    chat::progress::{ChatProgress, ChatProgressItem},
    commands::types::ChatStreamEvent,
};

const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLAY_EVENTS: usize = 4096;

#[derive(Default)]
pub(super) struct Replay {
    events: Vec<ChatStreamEvent>,
    progress: Option<(String, u64, ChatProgress)>,
    bytes: usize,
    overflowed: bool,
}

fn item_id(item: &ChatProgressItem) -> &str {
    match item {
        ChatProgressItem::Commentary { id, .. }
        | ChatProgressItem::Tool { id, .. }
        | ChatProgressItem::System { id, .. } => id,
    }
}

impl Replay {
    pub fn observe(&mut self, event: &ChatStreamEvent) {
        if self.overflowed {
            return;
        }
        match event {
            ChatStreamEvent::Progress {
                run_id,
                sequence,
                progress,
            } => {
                self.progress = Some((run_id.clone(), *sequence, progress.clone()));
            }
            ChatStreamEvent::ProgressPatch { patch, .. } => {
                if let Some((_, sequence, progress)) = &mut self.progress {
                    if patch.sequence > *sequence {
                        progress.items.retain(|item| {
                            !patch.remove_item_ids.iter().any(|id| id == item_id(item))
                        });
                        for item in &patch.upsert_items {
                            if let Some(existing) = progress
                                .items
                                .iter_mut()
                                .find(|existing| item_id(existing) == item_id(item))
                            {
                                *existing = item.clone();
                            } else {
                                progress.items.push(item.clone());
                            }
                        }
                        if patch.finished_at_ms.is_some() {
                            progress.finished_at_ms = patch.finished_at_ms;
                        }
                        if patch.plan.is_some() {
                            progress.plan = patch.plan.clone();
                        }
                        *sequence = patch.sequence;
                    }
                }
            }
            ChatStreamEvent::Delta { text, .. } => {
                self.bytes = self.bytes.saturating_add(text.len());
                if let Some(ChatStreamEvent::Delta { text: current, .. }) = self.events.last_mut() {
                    current.push_str(text);
                } else {
                    self.events.push(event.clone());
                }
            }
            _ => {
                self.bytes = self.bytes.saturating_add(
                    serde_json::to_vec(event).map_or(MAX_REPLAY_BYTES, |value| value.len()),
                );
                self.events.push(event.clone());
            }
        }
        // Progress is bounded separately because repeated patches replace data.
        let progress_bytes = self.progress.as_ref().map_or(0, |(_, _, progress)| {
            progress
                .items
                .iter()
                .map(|item| match item {
                    ChatProgressItem::Commentary { text, .. } => text.len() + 128,
                    ChatProgressItem::Tool {
                        action,
                        target,
                        href,
                        ..
                    }
                    | ChatProgressItem::System {
                        action,
                        target,
                        href,
                        ..
                    } => {
                        action.len()
                            + target.as_ref().map_or(0, String::len)
                            + href.as_ref().map_or(0, String::len)
                            + 512
                    }
                })
                .sum::<usize>()
        });
        if self.bytes.saturating_add(progress_bytes) > MAX_REPLAY_BYTES
            || self.events.len() > MAX_REPLAY_EVENTS
        {
            // Execution continues and canonical Chat remains available. Never
            // replay a silently truncated transcript as though it were complete.
            self.overflowed = true;
            self.events.clear();
            self.progress = None;
        }
    }

    pub fn events(&self) -> Option<Vec<ChatStreamEvent>> {
        if self.overflowed {
            return None;
        }
        let mut events = self.events.clone();
        if let Some((run_id, sequence, progress)) = &self.progress {
            events.push(ChatStreamEvent::Progress {
                run_id: run_id.clone(),
                sequence: *sequence,
                progress: progress.clone(),
            });
        }
        Some(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::progress::ChatProgressPatch;

    #[test]
    fn reconnect_folds_patches_and_contiguous_deltas_without_losing_text() {
        let mut replay = Replay::default();
        replay.observe(&ChatStreamEvent::Started {
            run_id: "run".into(),
        });
        replay.observe(&ChatStreamEvent::Progress {
            run_id: "run".into(),
            sequence: 0,
            progress: ChatProgress {
                started_at_ms: 1,
                finished_at_ms: None,
                items: vec![],
                plan: None,
                approvals: Vec::new(),
            },
        });
        for index in 1..=100 {
            replay.observe(&ChatStreamEvent::Delta {
                run_id: "run".into(),
                text: "a".into(),
            });
            replay.observe(&ChatStreamEvent::ProgressPatch {
                run_id: "run".into(),
                patch: ChatProgressPatch {
                    sequence: index,
                    upsert_items: vec![ChatProgressItem::Commentary {
                        id: "comment".into(),
                        text: format!("step {index}"),
                    }],
                    remove_item_ids: vec![],
                    finished_at_ms: None,
                    plan: None,
                    approvals: None,
                },
            });
        }
        let events = replay.events().unwrap();
        assert_eq!(events.len(), 3);
        assert!(
            matches!(&events[1], ChatStreamEvent::Delta { text, .. } if text == &"a".repeat(100))
        );
        assert!(
            matches!(&events[2], ChatStreamEvent::Progress { sequence: 100, progress, .. } if progress.items == [ChatProgressItem::Commentary { id: "comment".into(), text: "step 100".into() }])
        );
    }

    #[test]
    fn oversized_replay_is_explicitly_unavailable_and_releases_its_buffer() {
        let mut replay = Replay::default();
        replay.observe(&ChatStreamEvent::Delta {
            run_id: "run".into(),
            text: "a".repeat(MAX_REPLAY_BYTES + 1),
        });
        assert!(replay.events().is_none());
        assert!(replay.events.is_empty());
    }

    #[test]
    fn reconnect_applies_a_newer_patch_and_ignores_an_older_one() {
        let mut replay = Replay::default();
        replay.observe(&ChatStreamEvent::Progress {
            run_id: "run".into(),
            sequence: 0,
            progress: ChatProgress {
                started_at_ms: 1,
                finished_at_ms: None,
                items: vec![ChatProgressItem::Commentary {
                    id: "commentary-1".into(),
                    text: "Draft".into(),
                }],
                plan: None,
                approvals: Vec::new(),
            },
        });
        replay.observe(&ChatStreamEvent::ProgressPatch {
            run_id: "run".into(),
            patch: ChatProgressPatch {
                sequence: 2,
                upsert_items: vec![],
                remove_item_ids: vec!["commentary-1".into()],
                finished_at_ms: Some(20),
                plan: None,
                approvals: None,
            },
        });
        replay.observe(&ChatStreamEvent::ProgressPatch {
            run_id: "run".into(),
            patch: ChatProgressPatch {
                sequence: 1,
                upsert_items: vec![ChatProgressItem::Commentary {
                    id: "commentary-1".into(),
                    text: "Stale".into(),
                }],
                remove_item_ids: vec![],
                finished_at_ms: None,
                plan: None,
                approvals: None,
            },
        });
        let events = replay.events().unwrap();
        assert!(matches!(
            &events[0],
            ChatStreamEvent::Progress { sequence: 2, progress, .. }
                if progress.items.is_empty() && progress.finished_at_ms == Some(20)
        ));
    }
}
