//! "Allow commands in this Chat": the one blanket command grant a user can
//! give from a `command` card when only earlier replies carried third-party
//! text. It lives in memory for this app session, keyed by Loupist and Chat,
//! never applies to a turn that reads third-party text itself, and goes away
//! on `revoke` (the settled line's "Stop allowing", or the Chat's deletion) or
//! app restart. It remembers the Chat's newest reply that had recorded
//! third-party text when it was given: once a later reply records more, the
//! user allowed commands without having seen that text, so the grant is void.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// The id of the newest reply with third-party text, or None for a Chat that
/// never read any.
type Boundary = Option<String>;

fn registry() -> &'static Mutex<BTreeMap<(String, String), Boundary>> {
    static GRANTS: OnceLock<Mutex<BTreeMap<(String, String), Boundary>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

pub fn grant(loupist_id: &str, thread_id: &str, newest_untrusted: Option<&str>) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            (loupist_id.to_owned(), thread_id.to_owned()),
            newest_untrusted.map(str::to_owned),
        );
}

pub fn revoke(loupist_id: &str, thread_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&(loupist_id.to_owned(), thread_id.to_owned()));
}

/// Whether the Chat's grant stands for a turn whose newest reply with
/// third-party text is `newest_untrusted`.
pub fn granted(loupist_id: &str, thread_id: &str, newest_untrusted: Option<&str>) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(loupist_id.to_owned(), thread_id.to_owned()))
        .is_some_and(|boundary| boundary.as_deref() == newest_untrusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_is_scoped_to_one_chat_and_ends_on_revoke_or_newer_third_party_text() {
        assert!(!granted("loupist-g", "chat-g", Some("reply-1")));
        grant("loupist-g", "chat-g", Some("reply-1"));
        assert!(granted("loupist-g", "chat-g", Some("reply-1")));
        assert!(!granted("loupist-g", "chat-h", Some("reply-1")));
        assert!(!granted("loupist-h", "chat-g", Some("reply-1")));
        // A reply after the grant read third-party text the user never saw.
        assert!(!granted("loupist-g", "chat-g", Some("reply-2")));
        grant("loupist-g", "chat-g", Some("reply-2"));
        assert!(granted("loupist-g", "chat-g", Some("reply-2")));
        revoke("loupist-g", "chat-g");
        revoke("loupist-g", "chat-g");
        assert!(!granted("loupist-g", "chat-g", Some("reply-2")));
    }
}
