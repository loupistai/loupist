//! Sharing Loupists (W19): the pack format, exporter, private-string scan,
//! GitHub publishing, importer with review and quarantine, update checks for
//! packs already added, the community Gallery, and deep links. Rust owns every
//! byte, scan, network call and credential; the renderer submits typed intent
//! and receives counts, findings and validated text.
pub(crate) mod commands;
pub(crate) mod deep_link;
pub(crate) mod delivery;
pub(crate) mod export;
pub(crate) mod gallery;
pub(crate) mod github;
pub(crate) mod import;
pub(crate) mod pack;
pub(crate) mod provider_policy;
pub(crate) mod scan;
pub(crate) mod store;
#[cfg(test)]
mod tests;
pub(crate) mod update;

/// The README notice, in the sharer's language.
pub(crate) fn readme_notice(language: &str) -> &'static str {
    if language == "ko" {
        "AI 리서치 동료 루피스트로 만들었어요. 방법과 지도는 AI의 도움으로 작성되어 틀릴 수 있어요. 투자 조언이 아니에요."
    } else {
        "Made with Loupist, an AI research companion. Methods and the map were written with AI help and can be wrong. This is not financial advice."
    }
}

/// The title a pack carries when its Profile is not shared.
pub(crate) fn neutral_title(language: &str) -> &'static str {
    if language == "ko" {
        "공유된 루피스트"
    } else {
        "Shared Loupist"
    }
}
