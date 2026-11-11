//! `loupist://import?url=…` links and the GitHub link allowlist. Rust holds at
//! most one pending link (newer replaces older); nothing is fetched until the
//! person presses Preview.
use std::sync::Mutex;

/// A public GitHub location a pack may come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GithubLocation {
    Repo {
        owner: String,
        repo: String,
        reference: Option<String>,
        path: Option<String>,
    },
    Gist {
        user: String,
        id: String,
    },
}

impl GithubLocation {
    /// The canonical https URL.
    pub(crate) fn url(&self) -> String {
        match self {
            Self::Repo {
                owner,
                repo,
                reference: Some(reference),
                path,
            } => {
                let mut url = format!("https://github.com/{owner}/{repo}/tree/{reference}");
                if let Some(path) = path {
                    url.push('/');
                    url.push_str(path);
                }
                url
            }
            Self::Repo { owner, repo, .. } => format!("https://github.com/{owner}/{repo}"),
            Self::Gist { user, id } => format!("https://gist.github.com/{user}/{id}"),
        }
    }

    /// "github.com/owner/repo", shown in the confirm dialog and review.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Repo { owner, repo, .. } => format!("github.com/{owner}/{repo}"),
            Self::Gist { user, id } => format!("gist.github.com/{user}/{id}"),
        }
    }
}

pub(super) fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Accepts only `https://github.com/{o}/{r}[/tree/{ref}/{path}]` and
/// `https://gist.github.com/{u}/{id}`, without credentials, ports, queries or fragments.
pub(crate) fn parse_github(value: &str) -> Option<GithubLocation> {
    if value.len() > 2_000 {
        return None;
    }
    let url = reqwest::Url::parse(value.trim()).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let parts: Vec<&str> = url
        .path()
        .trim_end_matches('/')
        .trim_start_matches('/')
        .split('/')
        .collect();
    if !parts.iter().all(|part| segment(part)) {
        return None;
    }
    match url.host_str()? {
        "github.com" => match parts.as_slice() {
            [owner, repo] => {
                let repo = repo.strip_suffix(".git").unwrap_or(repo);
                segment(repo).then(|| GithubLocation::Repo {
                    owner: (*owner).into(),
                    repo: repo.into(),
                    reference: None,
                    path: None,
                })
            }
            [owner, repo, "tree", reference, rest @ ..] if rest.len() <= 6 => {
                Some(GithubLocation::Repo {
                    owner: (*owner).into(),
                    repo: (*repo).into(),
                    reference: Some((*reference).into()),
                    path: (!rest.is_empty()).then(|| rest.join("/")),
                })
            }
            _ => None,
        },
        "gist.github.com" => match parts.as_slice() {
            [user, id] if id.bytes().all(|b| b.is_ascii_hexdigit()) => Some(GithubLocation::Gist {
                user: (*user).into(),
                id: (*id).into(),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// `loupist://import?url=<allowlisted GitHub link>`; anything else is a bad link.
// Only supported desktop builds register the deep-link plugin. Keep its pure
// parser and receiver available to tests on every platform.
#[cfg(any(test, all(any(target_os = "macos", windows), not(feature = "e2e"))))]
pub(crate) fn parse_import_link(value: &str) -> Option<GithubLocation> {
    if value.len() > 4_000 {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "loupist"
        || url.host_str() != Some("import")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(url.path(), "" | "/")
        || url.fragment().is_some()
    {
        return None;
    }
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    match pairs.as_slice() {
        [(key, target)] if key == "url" => parse_github(target),
        _ => None,
    }
}

fn pending() -> &'static Mutex<Option<GithubLocation>> {
    static PENDING: Mutex<Option<GithubLocation>> = Mutex::new(None);
    &PENDING
}

/// What the renderer hears about an arriving link.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "lowercase")]
pub(crate) enum DeepLinkEvent {
    Ok {
        source: String,
    },
    #[cfg(any(test, all(any(target_os = "macos", windows), not(feature = "e2e"))))]
    Bad,
}

/// Validates an arriving link; a good one replaces any held link.
#[cfg(any(test, all(any(target_os = "macos", windows), not(feature = "e2e"))))]
pub(crate) fn receive(value: &str) -> DeepLinkEvent {
    match parse_import_link(value) {
        Some(location) => {
            let source = location.label();
            *pending().lock().unwrap_or_else(|e| e.into_inner()) = Some(location);
            DeepLinkEvent::Ok { source }
        }
        None => DeepLinkEvent::Bad,
    }
}

pub(crate) fn held() -> Option<GithubLocation> {
    pending().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn dismiss() {
    pending().lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_links_accept_only_allowlisted_github_locations() {
        let repo = parse_import_link(
            "loupist://import?url=https%3A%2F%2Fgithub.com%2Fkimj%2Floupist-quality-compounder",
        )
        .unwrap();
        assert_eq!(repo.label(), "github.com/kimj/loupist-quality-compounder");
        assert_eq!(
            parse_github("https://github.com/kimj/desk.git.git")
                .unwrap()
                .url(),
            "https://github.com/kimj/desk.git"
        );
        assert_eq!(
            parse_github("https://github.com/kimj/desk.git")
                .unwrap()
                .url(),
            "https://github.com/kimj/desk"
        );
        assert!(parse_github("https://github.com/kimj/.git").is_none());
        let tree = parse_import_link(
            "loupist://import?url=https://github.com/kimj/packs/tree/main/semis/chips",
        )
        .unwrap();
        assert_eq!(
            tree,
            GithubLocation::Repo {
                owner: "kimj".into(),
                repo: "packs".into(),
                reference: Some("main".into()),
                path: Some("semis/chips".into())
            }
        );
        assert_eq!(
            tree.url(),
            "https://github.com/kimj/packs/tree/main/semis/chips"
        );
        assert!(parse_import_link(
            "loupist://import?url=https://gist.github.com/kimj/0123456789abcdef"
        )
        .is_some());
        for bad in [
            "loupist://import",
            "loupist://import?url=https://evil.example/kimj/repo",
            "loupist://import?url=http://github.com/kimj/repo",
            "loupist://import?url=https://github.com/kimj/repo?x=1",
            "loupist://import?url=https://user:pass@github.com/kimj/repo",
            "loupist://import?url=https://github.com:444/kimj/repo",
            "loupist://import?url=https://github.com/kimj/repo&url=https://github.com/a/b",
            "loupist://import?url=https://github.com/kimj/repo&extra=1",
            "loupist://import?url=https://github.com/kimj/../repo",
            "loupist://import?url=https://github.com/kimj/repo/blob/main/x",
            "loupist://export?url=https://github.com/kimj/repo",
            "https://github.com/kimj/repo",
            "loupist://import/path?url=https://github.com/kimj/repo",
            "loupist://import?url=https://gist.github.com/kimj/not-hex!",
            "loupist://import?url=file:///etc/passwd",
        ] {
            assert!(parse_import_link(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_newer_link_replaces_the_held_one_and_bad_links_hold_nothing() {
        dismiss();
        assert_eq!(
            receive("loupist://import?url=https://example.com/x"),
            DeepLinkEvent::Bad
        );
        assert_eq!(held(), None);
        receive("loupist://import?url=https://github.com/a/one");
        receive("loupist://import?url=https://github.com/b/two");
        assert_eq!(held().unwrap().label(), "github.com/b/two");
        dismiss();
        assert_eq!(held(), None);
    }
}
