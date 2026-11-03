//! Live work-trace flush. Narration accumulates on `ChatProgressProjection`;
//! this module decides when that projection becomes a stream patch.
use std::time::Duration;

use tokio::time::Instant;

use super::{ChatEventSink, ChatStreamEvent};
use crate::chat::progress::{ChatProgress, ChatProgressProjection};

pub(super) const COMMENTARY_FLUSH_WINDOW: Duration = Duration::from_millis(32);

/// One narration window. The first delta arms it; later deltas do not extend it.
pub(super) struct CommentaryCoalescer {
    pending: bool,
    deadline: Option<Instant>,
}

impl CommentaryCoalescer {
    pub(super) fn new() -> Self {
        Self {
            pending: false,
            deadline: None,
        }
    }

    /// Returns whether this delta armed a new window.
    pub(super) fn note_delta(&mut self, now: Instant) -> bool {
        if self.pending {
            return false;
        }
        self.pending = true;
        self.deadline = Some(now + COMMENTARY_FLUSH_WINDOW);
        true
    }

    pub(super) fn is_pending(&self) -> bool {
        self.pending
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        self.pending && self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.pending.then_some(self.deadline).flatten()
    }

    pub(super) fn clear(&mut self) {
        self.pending = false;
        self.deadline = None;
    }
}

pub(super) fn publish_progress(
    on_event: &ChatEventSink,
    run_id: &str,
    projection: &mut ChatProgressProjection,
    commentary: &mut CommentaryCoalescer,
) {
    emit_chat_progress_patch(on_event, run_id, projection);
    commentary.clear();
}

/// The settled answer is visible before its live narration copy is removed.
pub(super) fn publish_settled_answer(
    on_event: &ChatEventSink,
    run_id: &str,
    projection: &mut ChatProgressProjection,
    commentary: &mut CommentaryCoalescer,
    text: &str,
) {
    let _ = on_event.send(ChatStreamEvent::Delta {
        run_id: run_id.to_owned(),
        text: text.to_owned(),
    });
    publish_progress(on_event, run_id, projection, commentary);
}

pub(super) fn emit_chat_progress_snapshot(
    on_event: &ChatEventSink,
    run_id: &str,
    projection: &ChatProgressProjection,
    sequence: u64,
) {
    let _ = on_event.send(ChatStreamEvent::Progress {
        run_id: run_id.to_owned(),
        sequence,
        progress: projection.snapshot(),
    });
}

pub(super) fn emit_chat_phase(
    on_event: &ChatEventSink,
    run_id: &str,
    announced: &mut Option<&'static str>,
    phase: &'static str,
) {
    if *announced == Some(phase) {
        return;
    }
    *announced = Some(phase);
    let _ = on_event.send(ChatStreamEvent::Phase {
        run_id: run_id.to_owned(),
        phase: phase.to_owned(),
    });
}

pub(super) fn emit_chat_progress_patch(
    on_event: &ChatEventSink,
    run_id: &str,
    projection: &mut ChatProgressProjection,
) {
    if let Some(patch) = projection.take_patch() {
        let _ = on_event.send(ChatStreamEvent::ProgressPatch {
            run_id: run_id.to_owned(),
            patch,
        });
    }
}

pub(super) fn finish_chat_progress(
    on_event: &ChatEventSink,
    run_id: &str,
    projection: &mut ChatProgressProjection,
    finished_at_ms: i64,
    stopped: bool,
) -> Option<ChatProgress> {
    let snapshot = projection.finish(finished_at_ms, stopped);
    emit_chat_progress_patch(on_event, run_id, projection);
    // A single terminal snapshot lets a renderer that missed a sequenced patch
    // resynchronize. Renderers already at this sequence ignore the duplicate.
    emit_chat_progress_snapshot(
        on_event,
        run_id,
        projection,
        projection.latest_patch_sequence(),
    );
    snapshot
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::chat::progress::ChatProgressItem;

    fn capture_sink(events: &Arc<Mutex<Vec<ChatStreamEvent>>>) -> ChatEventSink {
        let events = Arc::clone(events);
        ChatEventSink::new(move |event| {
            events.lock().expect("event lock").push(event);
            Ok(())
        })
    }

    #[test]
    fn settled_answer_delta_precedes_commentary_removal() {
        let mut progress = ChatProgressProjection::new(10);
        progress.start_assistant_turn();
        progress.append_commentary("Answer text");
        let published = progress.take_patch().expect("commentary patch");
        let commentary_id = match &published.upsert_items[0] {
            ChatProgressItem::Commentary { id, .. } => id.clone(),
            other => panic!("expected commentary, got {other:?}"),
        };
        progress.finish_assistant_turn(true);
        let mut window = CommentaryCoalescer::new();
        assert!(window.note_delta(Instant::now()));

        let events = Arc::new(Mutex::new(Vec::new()));
        publish_settled_answer(
            &capture_sink(&events),
            "run",
            &mut progress,
            &mut window,
            "Answer text",
        );

        let events = events.lock().expect("event lock");
        assert!(matches!(
            &events[0],
            ChatStreamEvent::Delta { text, .. } if text == "Answer text"
        ));
        assert!(matches!(
            &events[1],
            ChatStreamEvent::ProgressPatch { patch, .. }
                if patch.upsert_items.is_empty()
                    && patch.remove_item_ids == [commentary_id]
        ));
        assert!(!window.is_pending());
    }

    #[test]
    fn finishing_a_turn_publishes_commentary_still_inside_the_window() {
        let mut progress = ChatProgressProjection::new(10);
        progress.start_assistant_turn();
        progress.append_commentary("Still writing");
        let events = Arc::new(Mutex::new(Vec::new()));
        let snapshot = finish_chat_progress(&capture_sink(&events), "run", &mut progress, 20, true);
        let text = match &snapshot.expect("progress").items[0] {
            ChatProgressItem::Commentary { text, .. } => text.clone(),
            other => panic!("expected commentary, got {other:?}"),
        };
        assert_eq!(text, "Still writing");
        let events = events.lock().expect("event lock");
        assert!(matches!(
            &events[0],
            ChatStreamEvent::ProgressPatch { patch, .. }
                if patch.sequence == 1
                    && patch.finished_at_ms == Some(20)
                    && matches!(
                        patch.upsert_items.first(),
                        Some(ChatProgressItem::Commentary { text, .. }) if text == "Still writing"
                    )
        ));
        assert!(matches!(
            &events[1],
            ChatStreamEvent::Progress { sequence: 1, progress, .. }
                if progress.finished_at_ms == Some(20)
        ));
    }
}
