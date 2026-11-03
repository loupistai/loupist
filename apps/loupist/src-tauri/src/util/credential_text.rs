const ASSIGNED_NAMES: &[&str] = &[
    "access_token",
    "api_key",
    "api-key",
    "apikey",
    "app_key",
    "appkey",
    "appsecret",
    "authorization",
    "client_secret",
    "credential",
    "credentials",
    "crtfc_key",
    "passwd",
    "password",
    "private_key",
    "refresh_token",
    "secret",
    "token",
];
const QUERY_NAMES: &[&str] = &[
    "key",
    "token",
    "apikey",
    "api_key",
    "access_token",
    "secret",
    "sig",
    "signature",
    "password",
];
const AUTH_SCHEMES: &[&str] = &["bearer", "basic"];
const KEY_PREFIXES: &[(&str, usize)] = &[
    ("sk-", 6),
    ("ghp_", 20),
    ("gho_", 20),
    ("ghs_", 20),
    ("github_pat_", 20),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("akia", 16),
    ("aiza", 30),
];

/// Whether text carries a credential shape: a private key block, an auth scheme
/// followed by a token, a named secret assignment, a provider key, or a URL with
/// user information. Prose that only mentions these words stays readable.
pub(crate) fn contains_credential_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("-----begin")
        || AUTH_SCHEMES
            .iter()
            .any(|scheme| scheme_with_token(text, &lower, scheme))
        || ASSIGNED_NAMES.iter().any(|name| assigned(&lower, name))
        || QUERY_NAMES.iter().any(|name| {
            lower.contains(&format!("?{name}=")) || lower.contains(&format!("&{name}="))
        })
        || KEY_PREFIXES
            .iter()
            .any(|(prefix, minimum)| provider_key(&lower, prefix, *minimum))
        || url_userinfo(&lower)
}

fn starts_word(lower: &str, index: usize) -> bool {
    lower[..index]
        .chars()
        .next_back()
        .is_none_or(|previous| !previous.is_ascii_alphanumeric())
}

fn word_starts<'a>(lower: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    lower
        .match_indices(word)
        .map(|(index, _)| index)
        .filter(move |index| starts_word(lower, *index))
}

fn scheme_with_token(text: &str, lower: &str, scheme: &str) -> bool {
    word_starts(lower, scheme).any(|index| {
        let rest = &text[index + scheme.len()..];
        if !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let token = rest
            .trim_start()
            .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';'))
            .next()
            .unwrap_or("");
        let token_chars = |c: char| c.is_ascii_alphanumeric() || "._~+/=-".contains(c);
        token.len() >= 12
            && token.chars().all(token_chars)
            && (token
                .chars()
                .any(|c| c.is_ascii_digit() || "+/=".contains(c))
                || (token.chars().any(|c| c.is_ascii_uppercase())
                    && token.chars().any(|c| c.is_ascii_lowercase())))
    })
}

fn assigned(lower: &str, name: &str) -> bool {
    word_starts(lower, name).any(|index| {
        let rest = lower[index + name.len()..].trim_start_matches(['"', '\'']);
        let Some(value) = rest.trim_start().strip_prefix([':', '=']) else {
            return false;
        };
        value
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\''))
            .chars()
            .next()
            .is_some_and(|c| !matches!(c, ',' | '}' | ']'))
    })
}

fn provider_key(lower: &str, prefix: &str, minimum: usize) -> bool {
    word_starts(lower, prefix).any(|index| {
        lower[index + prefix.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .count()
            >= minimum
    })
}

fn url_userinfo(lower: &str) -> bool {
    lower.split("://").skip(1).any(|rest| {
        rest.split(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#'))
            .next()
            .is_some_and(|authority| authority.contains('@'))
    })
}

#[cfg(test)]
mod tests {
    use super::contains_credential_text;

    #[test]
    fn credential_shapes_are_detected() {
        for text in [
            "api_key=fixture",
            "Authorization: Bearer fixture",
            "password: fixture",
            "{\"client_secret\":\"value\"}",
            "sk-fixture",
            "call with Bearer eyJhbGciOiJIUzI1NiJ9.e30.sig",
            "header Basic dXNlcjpwYXNzd29yZA==",
            "https://example.test/data?key=AIzaFixtureValue",
            "https://user:pass@example.test/path",
            "-----BEGIN PRIVATE KEY-----",
            "ghp_abcdefghijklmnopqrstuvwx",
            "AKIAABCDEFGHIJKLMNOP",
        ] {
            assert!(contains_credential_text(text), "{text}");
        }
    }

    #[test]
    fn finance_prose_with_credential_words_stays_readable() {
        for text in [
            "Headline EPS uses basic shares, not diluted shares.",
            "Use basic earnings-per-share and the weighted-average count.",
            "Discount at the risk-free rate plus a risk-adjusted premium.",
            "Desk-2026 notes: keep the task-level summary short.",
            "Report tokens of 1,200 as usage, not revenue.",
            "The company keeps its formula secret from competitors.",
            "Key: revenue grew 12% while the token economy stalled.",
            "Bearer bonds are excluded from the cash balance.",
            "Read https://example.test/report?ticker=ACME for the filing.",
        ] {
            assert!(!contains_credential_text(text), "{text}");
        }
    }
}
