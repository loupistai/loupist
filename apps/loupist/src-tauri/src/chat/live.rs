use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use tokio::{
    sync::{broadcast, watch},
    task::AbortHandle,
};

use crate::{
    app::CommandError,
    commands::types::{ChatStreamEvent, ChatThreadDto, ChatWatchRequest, ChatWatchSnapshot},
};

use super::replay::Replay;

const FOLLOW_CAPACITY: usize = 64;

type Key = (String, String);
type Runs = Mutex<HashMap<Key, Arc<LiveChat>>>;
type Subscriber = (Arc<()>, AbortHandle);

#[derive(Clone, Default)]
pub(crate) struct LiveChats(Arc<Runs>);

pub(crate) struct LiveChat {
    run_id: String,
    ready: watch::Sender<bool>,
    stream: Mutex<Stream>,
}

struct Stream {
    baseline: Option<ChatThreadDto>,
    replay: Replay,
    sender: Option<broadcast::Sender<Arc<ChatStreamEvent>>>,
    subscribers: HashMap<String, Subscriber>,
}

pub(crate) struct LiveLease {
    pub chat: Arc<LiveChat>,
    registry: Weak<Runs>,
    key: Key,
}

impl LiveChats {
    pub(crate) fn start(&self, loupist: &str, thread: &str, run_id: &str) -> LiveLease {
        let key = (loupist.to_owned(), thread.to_owned());
        let chat = Arc::new(LiveChat {
            run_id: run_id.to_owned(),
            ready: watch::channel(false).0,
            stream: Mutex::new(Stream {
                baseline: None,
                replay: Replay::default(),
                sender: Some(broadcast::channel(FOLLOW_CAPACITY).0),
                subscribers: HashMap::new(),
            }),
        });
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key.clone(), chat.clone());
        LiveLease {
            chat,
            registry: Arc::downgrade(&self.0),
            key,
        }
    }

    fn get(&self, loupist: &str, thread: &str) -> Option<Arc<LiveChat>> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(loupist.to_owned(), thread.to_owned()))
            .cloned()
    }

    pub(crate) async fn attach(
        &self,
        request: ChatWatchRequest,
        observe: impl Fn(ChatStreamEvent) -> Result<(), ()> + Send + 'static,
    ) -> Result<Option<ChatWatchSnapshot>, CommandError> {
        let Some(chat) = self.get(&request.loupist_id, &request.thread_id) else {
            return Ok(None);
        };
        let _ = chat.ready.subscribe().wait_for(|ready| *ready).await;
        chat.attach(request.subscription_id, observe)
    }

    pub(crate) fn detach(&self, request: &ChatWatchRequest) {
        if let Some(chat) = self.get(&request.loupist_id, &request.thread_id) {
            if let Some((_, task)) = chat.lock().subscribers.remove(&request.subscription_id) {
                task.abort();
            }
        }
    }

    /// Whether this run's lease is held: its sink can still record how it ends.
    pub(crate) fn holds(&self, loupist: &str, thread: &str, run_id: &str) -> bool {
        self.get(loupist, thread)
            .is_some_and(|chat| chat.run_id == run_id)
    }
}

impl Drop for LiveLease {
    fn drop(&mut self) {
        self.chat.close();
        if let Some(registry) = self.registry.upgrade() {
            let mut runs = registry.lock().unwrap_or_else(|error| error.into_inner());
            if runs
                .get(&self.key)
                .is_some_and(|chat| Arc::ptr_eq(chat, &self.chat))
            {
                runs.remove(&self.key);
            }
        }
    }
}

impl LiveChat {
    fn lock(&self) -> std::sync::MutexGuard<'_, Stream> {
        self.stream
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn set_baseline(&self, thread: ChatThreadDto) {
        self.lock().baseline = Some(thread);
        self.ready.send_replace(true);
    }

    fn close(&self) {
        if let Some(sender) = self.lock().sender.take() {
            let _ = sender.send(Arc::new(ChatStreamEvent::StreamReset {
                run_id: self.run_id.clone(),
            }));
        }
        self.ready.send_replace(true);
    }

    pub(crate) fn publish(&self, event: ChatStreamEvent) {
        let mut stream = self.lock();
        let terminal = matches!(
            event,
            ChatStreamEvent::Completed { .. }
                | ChatStreamEvent::Error { .. }
                | ChatStreamEvent::Aborted { .. }
        );
        stream.replay.observe(&event);
        if let Some(sender) = &stream.sender {
            let _ = sender.send(Arc::new(event));
        }
        if terminal {
            stream.sender.take();
            self.ready.send_replace(true);
        }
    }

    fn attach(
        self: &Arc<Self>,
        id: String,
        observe: impl Fn(ChatStreamEvent) -> Result<(), ()> + Send + 'static,
    ) -> Result<Option<ChatWatchSnapshot>, CommandError> {
        let mut stream = self.lock();
        let (Some(sender), Some(thread)) = (&stream.sender, &stream.baseline) else {
            return Ok(None);
        };
        let thread = thread.clone();
        let replay = stream.replay.events().ok_or_else(|| {
            CommandError::new(
                "item_too_large",
                "The live Chat snapshot exceeded its limit",
            )
        })?;
        let mut events = sender.subscribe();
        let chat = Arc::downgrade(self);
        let run_id = self.run_id.clone();
        let subscription = id.clone();
        let identity = Arc::new(());
        let task_identity = identity.clone();
        let task = tauri::async_runtime::spawn(async move {
            if replay.into_iter().all(|event| observe(event).is_ok()) {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            if observe(Arc::unwrap_or_clone(event)).is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let _ = observe(ChatStreamEvent::StreamReset { run_id });
                            break;
                        }
                        _ => break,
                    }
                }
            }
            if let Some(chat) = chat.upgrade() {
                let mut stream = chat.lock();
                if stream
                    .subscribers
                    .get(&subscription)
                    .is_some_and(|(current, _)| Arc::ptr_eq(current, &task_identity))
                {
                    stream.subscribers.remove(&subscription);
                }
            }
        });
        if let Some((_, old)) = stream
            .subscribers
            .insert(id, (identity, task.inner().abort_handle()))
        {
            old.abort();
        }
        Ok(Some(ChatWatchSnapshot {
            run_id: self.run_id.clone(),
            thread,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commands::types::chat_dto, test_fixtures::chat_session};
    use std::sync::mpsc;
    use tokio::sync::oneshot;

    fn request(id: &str) -> ChatWatchRequest {
        ChatWatchRequest {
            loupist_id: "loupist".into(),
            thread_id: "thread".into(),
            subscription_id: id.into(),
        }
    }

    fn delta(text: &str) -> ChatStreamEvent {
        ChatStreamEvent::Delta {
            run_id: "run".into(),
            text: text.into(),
            settles: None,
        }
    }

    #[test]
    fn a_lease_holds_its_run_until_it_drops_and_only_that_run() {
        let chats = LiveChats::default();
        let lease = chats.start("loupist", "thread", "run");
        assert!(chats.holds("loupist", "thread", "run"));
        assert!(!chats.holds("loupist", "thread", "other-run"));
        assert!(!chats.holds("loupist", "other-thread", "run"));
        // A terminal event ends the stream, not the lease: the sink still holds it.
        lease.chat.publish(ChatStreamEvent::Aborted {
            run_id: "run".into(),
        });
        assert!(chats.holds("loupist", "thread", "run"));
        drop(lease);
        assert!(!chats.holds("loupist", "thread", "run"));
    }

    #[tokio::test]
    async fn live_observer_overflow_signals_reset_and_fresh_attach_replays_everything() {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let chats = LiveChats::default();
            let lease = chats.start("loupist", "thread", "run");
            lease.chat.set_baseline(chat_dto(&chat_session("thread", "loupist")).unwrap());
            lease.chat.publish(delta("snapshot "));
            let (entered, waiting) = oneshot::channel();
            let entered = Mutex::new(Some(entered));
            let (resume, blocked) = mpsc::channel();
            let (event, mut observed) = tokio::sync::mpsc::unbounded_channel();
            let snapshot = chats.attach(request("slow"), move |item| {
                if let Some(entered) = entered.lock().unwrap().take() {
                    entered.send(()).unwrap();
                    blocked.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                }
                event.send(item).map_err(|_| ())
            }).await.unwrap().unwrap();
            assert_eq!(snapshot.thread.id, "thread");
            waiting.await.unwrap();
            for _ in 0..=FOLLOW_CAPACITY {
                lease.chat.publish(delta("+"));
            }
            resume.send(()).unwrap();
            assert!(matches!(observed.recv().await, Some(ChatStreamEvent::Delta { text, .. }) if text == "snapshot "));
            assert!(matches!(observed.recv().await, Some(ChatStreamEvent::StreamReset { run_id }) if run_id == "run"));
            let (event, mut observed) = tokio::sync::mpsc::unbounded_channel();
            chats.attach(request("fresh"), move |item| event.send(item).map_err(|_| ()))
                .await.unwrap().unwrap();
            let expected = format!("snapshot {}", "+".repeat(FOLLOW_CAPACITY + 1));
            assert!(matches!(observed.recv().await, Some(ChatStreamEvent::Delta { text, .. }) if text == expected));
            lease.chat.publish(delta("following"));
            assert!(matches!(observed.recv().await, Some(ChatStreamEvent::Delta { text, .. }) if text == "following"));
        }).await.unwrap();
    }
}
