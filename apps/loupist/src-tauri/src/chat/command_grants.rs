//! "Allow commands in this Chat": the one blanket command grant a user can
//! give from a `command` card when only earlier replies carried third-party
//! text. It lives in memory for this app session, keyed by Loupist and Chat,
//! never applies to a turn that reads third-party text itself, and goes away
//! on `revoke` (the settled line's "Stop allowing") or app restart.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

fn registry() -> &'static Mutex<BTreeSet<(String, String)>> {
    static GRANTS: OnceLock<Mutex<BTreeSet<(String, String)>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

pub fn grant(loupist_id: &str, thread_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert((loupist_id.to_owned(), thread_id.to_owned()));
}

pub fn revoke(loupist_id: &str, thread_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&(loupist_id.to_owned(), thread_id.to_owned()));
}

pub fn granted(loupist_id: &str, thread_id: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(&(loupist_id.to_owned(), thread_id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_is_scoped_to_one_chat_and_ends_on_revoke() {
        assert!(!granted("loupist-g", "chat-g"));
        grant("loupist-g", "chat-g");
        assert!(granted("loupist-g", "chat-g"));
        assert!(!granted("loupist-g", "chat-h"));
        assert!(!granted("loupist-h", "chat-g"));
        revoke("loupist-g", "chat-g");
        revoke("loupist-g", "chat-g");
        assert!(!granted("loupist-g", "chat-g"));
    }
}
