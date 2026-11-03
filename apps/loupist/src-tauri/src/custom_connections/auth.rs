//! MCP Authorization 2026-07-28. Never propagate remote errors or secret values.
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

use super::{
    http::{canonical_resource, checked_url, client, json_response, Challenge},
    ConnectionError, CLIENT_METADATA_URL,
};

#[derive(Default)]
pub struct BearerChallenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub insufficient_scope: bool,
}

/// Parse quoted commas and backslash escapes, and ignore other auth schemes.
pub fn parse_challenge(headers: &[String]) -> Result<BearerChallenge, ConnectionError> {
    let mut values = BTreeMap::new();
    for header in headers {
        if header.len() > 16 * 1024 {
            return Err(ConnectionError::Protocol);
        }
        let mut fields = Vec::new();
        let mut start = 0;
        let mut quoted = false;
        let mut escaped = false;
        for (index, ch) in header.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && quoted {
                escaped = true;
            } else if ch == '"' {
                quoted = !quoted;
            } else if ch == ',' && !quoted {
                fields.push(&header[start..index]);
                start = index + 1;
            }
        }
        if quoted || escaped {
            return Err(ConnectionError::Protocol);
        }
        fields.push(&header[start..]);
        let mut bearer = false;
        for field in fields {
            let mut field = field.trim();
            if let Some((scheme, rest)) = field.split_once(char::is_whitespace) {
                if !scheme.contains('=') {
                    bearer = scheme.eq_ignore_ascii_case("bearer");
                    field = rest.trim();
                }
            }
            if !bearer {
                continue;
            }
            let (key, raw) = field.split_once('=').ok_or(ConnectionError::Protocol)?;
            let raw = raw.trim();
            let value = if raw.starts_with('"') {
                let inner = raw
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .ok_or(ConnectionError::Protocol)?;
                let mut result = String::new();
                let mut chars = inner.chars();
                while let Some(ch) = chars.next() {
                    result.push(if ch == '\\' {
                        chars.next().ok_or(ConnectionError::Protocol)?
                    } else {
                        ch
                    });
                }
                result
            } else {
                raw.to_owned()
            };
            if values
                .insert(key.trim().to_ascii_lowercase(), value)
                .is_some()
            {
                return Err(ConnectionError::Protocol);
            }
        }
    }
    Ok(BearerChallenge {
        resource_metadata: values.remove("resource_metadata"),
        scope: values.remove("scope"),
        insufficient_scope: values
            .get("error")
            .is_some_and(|v| v == "insufficient_scope"),
    })
}

pub fn resource_metadata_urls(
    resource: &str,
    challenge: &BearerChallenge,
) -> Result<Vec<Url>, ConnectionError> {
    if let Some(url) = &challenge.resource_metadata {
        return Ok(vec![checked_url(url)?]);
    }
    let url = checked_url(resource)?;
    let base = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    let mut urls = vec![checked_url(&format!(
        "{base}/.well-known/oauth-protected-resource{path}"
    ))?];
    if !path.is_empty() {
        urls.push(checked_url(&format!(
            "{base}/.well-known/oauth-protected-resource"
        ))?);
    }
    Ok(urls)
}

pub fn authorization_metadata_urls(issuer: &str) -> Result<Vec<Url>, ConnectionError> {
    let url = checked_url(issuer)?;
    if url.query().is_some() {
        return Err(ConnectionError::InvalidAuthorization);
    }
    let base = url.origin().ascii_serialization();
    let path = url.path().trim_end_matches('/');
    let mut urls = vec![
        checked_url(&format!(
            "{base}/.well-known/oauth-authorization-server{path}"
        ))?,
        checked_url(&format!("{base}/.well-known/openid-configuration{path}"))?,
    ];
    if !path.is_empty() {
        urls.push(checked_url(&format!(
            "{base}{path}/.well-known/openid-configuration"
        ))?);
    }
    Ok(urls)
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AuthorizationMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub authorization_response_iss_parameter_supported: bool,
    #[serde(default)]
    pub client_id_metadata_document_supported: bool,
    pub code_challenge_methods_supported: Vec<String>,
}

pub struct Discovery {
    pub metadata: AuthorizationMetadata,
    pub scopes: BTreeSet<String>,
}

pub async fn discover(
    resource: &str,
    challenge: &BearerChallenge,
) -> Result<Discovery, ConnectionError> {
    let http = client()?;
    let mut protected = None;
    for url in resource_metadata_urls(resource, challenge)? {
        let response = http
            .get(url)
            .send()
            .await
            .map_err(|_| ConnectionError::Network)?;
        if response.status().is_redirection() {
            return Err(ConnectionError::Redirect);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        let value = json_response(response).await?;
        let named_resource = value
            .get("resource")
            .and_then(Value::as_str)
            .ok_or(ConnectionError::InvalidAuthorization)?;
        if canonical_resource(named_resource)? != canonical_resource(resource)? {
            return Err(ConnectionError::InvalidAuthorization);
        }
        protected = Some(value);
        break;
    }
    let protected = protected.ok_or(ConnectionError::Authorization)?;
    let servers = protected
        .get("authorization_servers")
        .and_then(Value::as_array)
        .ok_or(ConnectionError::InvalidAuthorization)?;
    let issuer = servers
        .first()
        .and_then(Value::as_str)
        .ok_or(ConnectionError::InvalidAuthorization)?;
    let mut metadata = None;
    for url in authorization_metadata_urls(issuer)? {
        let response = http
            .get(url)
            .send()
            .await
            .map_err(|_| ConnectionError::Network)?;
        if response.status().is_redirection() {
            return Err(ConnectionError::Redirect);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        let candidate: AuthorizationMetadata =
            serde_json::from_value(json_response(response).await?)
                .map_err(|_| ConnectionError::InvalidAuthorization)?;
        // Exact string comparison, with no URL normalization, is intentional.
        if candidate.issuer != issuer
            || !candidate
                .code_challenge_methods_supported
                .iter()
                .any(|s| s == "S256")
        {
            return Err(ConnectionError::InvalidAuthorization);
        }
        checked_url(&candidate.authorization_endpoint)?;
        checked_url(&candidate.token_endpoint)?;
        if let Some(endpoint) = &candidate.registration_endpoint {
            checked_url(endpoint)?;
        }
        metadata = Some(candidate);
        break;
    }
    let scopes = match &challenge.scope {
        Some(scope) => scope.split_whitespace().map(str::to_owned).collect(),
        None => protected
            .get("scopes_supported")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    };
    Ok(Discovery {
        metadata: metadata.ok_or(ConnectionError::Authorization)?,
        scopes,
    })
}

// Only serialized into the credential store. No Debug implementation.
#[derive(Clone, Deserialize, Serialize)]
pub struct Registration {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub token_endpoint_auth_method: String,
    #[serde(default)]
    pub dynamically_registered: bool,
}

pub async fn register(
    metadata: &AuthorizationMetadata,
    redirect: &str,
    supplied: Option<&Registration>,
) -> Result<Registration, ConnectionError> {
    register_with_metadata_url(metadata, redirect, supplied, CLIENT_METADATA_URL).await
}

async fn register_with_metadata_url(
    metadata: &AuthorizationMetadata,
    redirect: &str,
    supplied: Option<&Registration>,
    metadata_url: &str,
) -> Result<Registration, ConnectionError> {
    if !metadata_url.is_empty() && metadata.client_id_metadata_document_supported {
        let url = checked_url(metadata_url)?;
        if url.scheme() != "https" || url.path() == "/" {
            return Err(ConnectionError::Configuration);
        }
        return Ok(Registration {
            issuer: metadata.issuer.clone(),
            client_id: metadata_url.into(),
            client_secret: None,
            token_endpoint_auth_method: "none".into(),
            dynamically_registered: false,
        });
    }
    // Reuse only registration bound to this exact issuer. A changed issuer must
    // never receive the previous client's credentials.
    if let Some(registration) = supplied {
        if registration.issuer != metadata.issuer {
            return Err(ConnectionError::InvalidAuthorization);
        }
        if registration.dynamically_registered || metadata.registration_endpoint.is_none() {
            return Ok(registration.clone());
        }
    }
    let endpoint = metadata
        .registration_endpoint
        .as_ref()
        .ok_or(ConnectionError::ClientIdRequired)?;
    let value = json_response(client()?.post(checked_url(endpoint)?).json(&json!({
        "client_name":"Loupist", "redirect_uris":[redirect], "application_type":"native",
        "grant_types":["authorization_code","refresh_token"], "response_types":["code"], "token_endpoint_auth_method":"none"
    })).send().await.map_err(|_| ConnectionError::Network)?).await?;
    if value
        .get("redirect_uris")
        .and_then(Value::as_array)
        .is_some_and(|uris| !uris.iter().any(|uri| uri.as_str() == Some(redirect)))
    {
        return Err(ConnectionError::InvalidAuthorization);
    }
    let client_id = value
        .get("client_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 2048)
        .ok_or(ConnectionError::InvalidAuthorization)?;
    let auth_method = value
        .get("token_endpoint_auth_method")
        .and_then(Value::as_str)
        .unwrap_or("none");
    if !["none", "client_secret_post", "client_secret_basic"].contains(&auth_method) {
        return Err(ConnectionError::InvalidAuthorization);
    }
    let secret = value
        .get("client_secret")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if auth_method != "none" && secret.is_none() {
        return Err(ConnectionError::InvalidAuthorization);
    }
    Ok(Registration {
        issuer: metadata.issuer.clone(),
        client_id: client_id.into(),
        client_secret: secret,
        token_endpoint_auth_method: auth_method.into(),
        dynamically_registered: true,
    })
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Tokens {
    pub resource: String,
    pub issuer: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub scopes: BTreeSet<String>,
}

pub struct AuthorizationAttempt {
    pub issuer: String,
    pub require_issuer: bool,
    pub state: String,
    pub verifier: String,
}

impl AuthorizationAttempt {
    pub fn new(metadata: &AuthorizationMetadata) -> Self {
        Self {
            issuer: metadata.issuer.clone(),
            require_issuer: metadata.authorization_response_iss_parameter_supported,
            state: uuid::Uuid::new_v4().simple().to_string(),
            verifier: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        }
    }

    pub fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }

    pub fn validate_callback(&self, callback: &Url) -> Result<String, ConnectionError> {
        let mut query = BTreeMap::new();
        for (key, value) in callback.query_pairs() {
            if query.insert(key.into_owned(), value.into_owned()).is_some() {
                return Err(ConnectionError::InvalidAuthorization);
            }
        }
        match query.get("iss") {
            Some(issuer) if issuer != &self.issuer => {
                return Err(ConnectionError::InvalidAuthorization)
            }
            None if self.require_issuer => return Err(ConnectionError::InvalidAuthorization),
            _ => {}
        }
        if query.get("state") != Some(&self.state) || query.contains_key("error") {
            return Err(ConnectionError::InvalidAuthorization);
        }
        query
            .remove("code")
            .filter(|c| !c.is_empty() && c.len() <= 4096)
            .ok_or(ConnectionError::InvalidAuthorization)
    }
}

pub async fn authorize(
    metadata: &AuthorizationMetadata,
    resource: &str,
    scopes: &BTreeSet<String>,
    supplied: Option<&Registration>,
) -> Result<(Registration, Tokens), ConnectionError> {
    authorize_with_opener(metadata, resource, scopes, supplied, |url| {
        crate::web::external_browser::open(url).map_err(|_| ConnectionError::Authorization)
    })
    .await
}

async fn authorize_with_opener(
    metadata: &AuthorizationMetadata,
    resource: &str,
    scopes: &BTreeSet<String>,
    supplied: Option<&Registration>,
    open: impl FnOnce(&str) -> Result<(), ConnectionError>,
) -> Result<(Registration, Tokens), ConnectionError> {
    timeout(Duration::from_secs(300), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| ConnectionError::Authorization)?;
        let redirect = format!("http://127.0.0.1:{}/callback", listener.local_addr().map_err(|_| ConnectionError::Authorization)?.port());
        let registration = register(metadata, &redirect, supplied).await?;
        let attempt = AuthorizationAttempt::new(metadata);
        let mut url = checked_url(&metadata.authorization_endpoint)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code").append_pair("client_id", &registration.client_id)
                .append_pair("redirect_uri", &redirect).append_pair("state", &attempt.state)
                .append_pair("code_challenge", &attempt.challenge()).append_pair("code_challenge_method", "S256")
                .append_pair("resource", resource);
            if !scopes.is_empty() { query.append_pair("scope", &scopes.iter().cloned().collect::<Vec<_>>().join(" ")); }
        }
        open(url.as_str())?;
        let (mut socket, peer) = listener.accept().await.map_err(|_| ConnectionError::Authorization)?;
        if !peer.ip().is_loopback() { return Err(ConnectionError::InvalidAuthorization); }
        let mut bytes = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            let count = timeout(Duration::from_secs(5), socket.read(&mut buf)).await.map_err(|_| ConnectionError::Authorization)?.map_err(|_| ConnectionError::Authorization)?;
            if count == 0 || bytes.len() + count > 16 * 1024 { return Err(ConnectionError::InvalidAuthorization); }
            bytes.extend_from_slice(&buf[..count]);
            if bytes.windows(4).any(|w| w == b"\r\n\r\n") { break; }
        }
        let request = std::str::from_utf8(&bytes).map_err(|_| ConnectionError::InvalidAuthorization)?;
        let target = request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|line| line.strip_suffix(" HTTP/1.1")).ok_or(ConnectionError::InvalidAuthorization)?;
        if !target.starts_with("/callback?") { return Err(ConnectionError::InvalidAuthorization); }
        let callback = Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| ConnectionError::InvalidAuthorization)?;
        let result = attempt.validate_callback(&callback);
        let response = if result.is_ok() { "HTTP/1.1 200 OK\r\nContent-Length: 34\r\nConnection: close\r\n\r\nYou can return to Loupist now" }
            else { "HTTP/1.1 400 Bad Request\r\nContent-Length: 20\r\nConnection: close\r\n\r\nAuthorization failed" };
        let _ = socket.write_all(response.as_bytes()).await;
        let code = result?;
        let form = vec![("grant_type", "authorization_code".into()), ("code", code), ("redirect_uri", redirect), ("code_verifier", attempt.verifier)];
        let tokens = token_request(&client()?, metadata, &registration, resource, scopes, form, None).await?;
        Ok((registration, tokens))
    }).await.map_err(|_| ConnectionError::Authorization)?
}

async fn token_request(
    http: &Client,
    metadata: &AuthorizationMetadata,
    registration: &Registration,
    resource: &str,
    requested_scopes: &BTreeSet<String>,
    mut form: Vec<(&str, String)>,
    previous_refresh: Option<&str>,
) -> Result<Tokens, ConnectionError> {
    if registration.issuer != metadata.issuer {
        return Err(ConnectionError::InvalidAuthorization);
    }
    form.push(("resource", resource.into()));
    form.push(("client_id", registration.client_id.clone()));
    let mut request = http.post(checked_url(&metadata.token_endpoint)?);
    if let Some(secret) = &registration.client_secret {
        match registration.token_endpoint_auth_method.as_str() {
            "client_secret_post" => form.push(("client_secret", secret.clone())),
            "client_secret_basic" => {
                request = request.basic_auth(&registration.client_id, Some(secret))
            }
            "none" => {}
            _ => return Err(ConnectionError::InvalidAuthorization),
        }
    }
    let value = json_response(
        request
            .form(&form)
            .send()
            .await
            .map_err(|_| ConnectionError::Network)?,
    )
    .await?;
    if !value
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("bearer"))
    {
        return Err(ConnectionError::InvalidAuthorization);
    }
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 16 * 1024)
        .ok_or(ConnectionError::InvalidAuthorization)?;
    Ok(Tokens {
        resource: resource.into(),
        issuer: metadata.issuer.clone(),
        access_token: token.into(),
        refresh_token: value
            .get("refresh_token")
            .and_then(Value::as_str)
            .or(previous_refresh)
            .map(str::to_owned),
        scopes: value
            .get("scope")
            .and_then(Value::as_str)
            .map(|s| s.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_else(|| requested_scopes.clone()),
    })
}

pub async fn refresh(
    metadata: &AuthorizationMetadata,
    registration: &Registration,
    tokens: &Tokens,
) -> Result<Tokens, ConnectionError> {
    if tokens.issuer != metadata.issuer {
        return Err(ConnectionError::InvalidAuthorization);
    }
    let refresh = tokens
        .refresh_token
        .as_deref()
        .ok_or(ConnectionError::Authorization)?;
    token_request(
        &client()?,
        metadata,
        registration,
        &tokens.resource,
        &tokens.scopes,
        vec![
            ("grant_type", "refresh_token".into()),
            ("refresh_token", refresh.into()),
        ],
        Some(refresh),
    )
    .await
}

pub fn scope_upgrade(
    tokens: &Tokens,
    challenge: &Challenge,
    attempts: &mut u8,
) -> Result<BTreeSet<String>, ConnectionError> {
    let challenge = parse_challenge(&challenge.headers)?;
    if !challenge.insufficient_scope || *attempts >= 2 {
        return Err(ConnectionError::Authorization);
    }
    let scope = challenge.scope.ok_or(ConnectionError::Authorization)?;
    *attempts += 1;
    Ok(tokens
        .scopes
        .iter()
        .cloned()
        .chain(scope.split_whitespace().map(str::to_owned))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_connections::fixture::{Response, Server};
    use std::sync::{Arc, Mutex};

    fn metadata(base: &str) -> AuthorizationMetadata {
        serde_json::from_value(json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),
            "token_endpoint":format!("{base}/token"),"registration_endpoint":format!("{base}/register"),
            "authorization_response_iss_parameter_supported":true,"client_id_metadata_document_supported":true,
            "code_challenge_methods_supported":["S256"]})).unwrap()
    }

    #[test]
    fn issuer_table_and_state_apply_to_success_and_error_callbacks() {
        for require in [true, false] {
            let mut meta = metadata("https://issuer.example/Tenant");
            meta.authorization_response_iss_parameter_supported = require;
            let attempt = AuthorizationAttempt::new(&meta);
            for issuer in [
                None,
                Some("https://issuer.example/Tenant"),
                Some("https://issuer.example/tenant"),
                Some("https://issuer.example/Tenant/"),
            ] {
                let mut callback = Url::parse("http://127.0.0.1/callback").unwrap();
                callback
                    .query_pairs_mut()
                    .append_pair("state", &attempt.state)
                    .append_pair("code", "fixture-code");
                if let Some(issuer) = issuer {
                    callback.query_pairs_mut().append_pair("iss", issuer);
                }
                let allowed =
                    issuer == Some(meta.issuer.as_str()) || (issuer.is_none() && !require);
                assert_eq!(attempt.validate_callback(&callback).is_ok(), allowed);
                callback
                    .query_pairs_mut()
                    .append_pair("error", "remote untrusted instructions");
                assert!(attempt.validate_callback(&callback).is_err());
            }
            let mut wrong_state =
                Url::parse("http://127.0.0.1/callback?state=wrong&code=fixture").unwrap();
            wrong_state
                .query_pairs_mut()
                .append_pair("iss", &meta.issuer);
            assert!(attempt.validate_callback(&wrong_state).is_err());
        }
    }

    #[tokio::test]
    async fn local_authorization_server_covers_dcr_pkce_resource_refresh_and_step_up() {
        let challenge_seen = Arc::new(Mutex::new(String::new()));
        let captured = challenge_seen.clone();
        let server = Server::start(move |base, request| {
            if request.target.starts_with("/.well-known/oauth-protected-resource") {
                return Response::json(json!({"resource":format!("{base}/mcp"),"authorization_servers":[base],"scopes_supported":["prices:read"]}));
            }
            if request.target == "/.well-known/oauth-authorization-server" { return Response::json(serde_json::to_value(metadata(base)).unwrap()); }
            if request.target == "/register" {
                let registration = request.json();
                assert_eq!(registration["application_type"], "native");
                assert_eq!(registration["grant_types"], json!(["authorization_code", "refresh_token"]));
                return Response::json(json!({"client_id":"fixture-client","token_endpoint_auth_method":"none", "redirect_uris":registration["redirect_uris"]}));
            }
            if request.target.starts_with("/authorize?") {
                let url = Url::parse(&format!("{base}{}", request.target)).unwrap();
                let query: BTreeMap<_, _> = url.query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect();
                assert_eq!(query["resource"], format!("{base}/mcp"));
                assert_eq!(query["code_challenge_method"], "S256");
                *captured.lock().unwrap() = query["code_challenge"].clone();
                let mut callback = Url::parse(&query["redirect_uri"]).unwrap();
                callback.query_pairs_mut().append_pair("code", "fixture-code").append_pair("state", &query["state"]).append_pair("iss", base);
                return Response::status(302).header("Location", callback.as_str());
            }
            if request.target == "/token" {
                let form = request.form();
                assert_eq!(form["resource"], format!("{base}/mcp"));
                if form["grant_type"] == "authorization_code" {
                    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())), *captured.lock().unwrap());
                } else { assert_eq!(form["grant_type"], "refresh_token"); assert_eq!(form["refresh_token"], "fixture-refresh"); }
                return Response::json(json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","token_type":"Bearer"}));
            }
            Response::status(404)
        }).await;
        let resource = format!("{}/mcp", server.url);
        let found = discover(&resource, &BearerChallenge::default())
            .await
            .unwrap();
        assert!(found.scopes.contains("prices:read"));
        let open = |url: &str| {
            let url = url.to_owned();
            tokio::spawn(async move {
                reqwest::get(url).await.unwrap();
            });
            Ok(())
        };
        let (registration, tokens) =
            authorize_with_opener(&found.metadata, &resource, &found.scopes, None, open)
                .await
                .unwrap();
        assert_eq!(registration.client_id, "fixture-client");
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.target == "/register"));
        let refreshed = refresh(&found.metadata, &registration, &tokens)
            .await
            .unwrap();
        assert_eq!(refreshed.resource, resource);
        let challenge = Challenge {
            status: reqwest::StatusCode::FORBIDDEN,
            headers: vec!["Bearer error=\"insufficient_scope\", scope=\"news:read\"".into()],
        };
        let mut attempts = 0;
        for _ in 0..2 {
            let scopes = scope_upgrade(&tokens, &challenge, &mut attempts).unwrap();
            assert_eq!(
                scopes,
                BTreeSet::from(["prices:read".into(), "news:read".into()])
            );
            authorize_with_opener(
                &found.metadata,
                &resource,
                &scopes,
                Some(&registration),
                open,
            )
            .await
            .unwrap();
        }
        assert!(scope_upgrade(&tokens, &challenge, &mut attempts).is_err());
    }

    #[tokio::test]
    async fn metadata_from_another_host_is_uncredentialed_and_resource_bound() {
        let resource_server = Server::start(|_, _| Response::status(401)).await;
        let resource = format!("{}/mcp", resource_server.url);
        let named_resource = resource.clone();
        let metadata_server = Server::start(move |base, request| {
            assert!(!request.headers.contains_key("authorization"));
            if request.target == "/prm" {
                Response::json(json!({"resource":named_resource,"authorization_servers":[base]}))
            } else {
                Response::json(serde_json::to_value(metadata(base)).unwrap())
            }
        })
        .await;
        let challenge = BearerChallenge {
            resource_metadata: Some(format!("{}/prm", metadata_server.url)),
            ..Default::default()
        };
        assert!(discover(&resource, &challenge).await.is_ok());
        assert!(
            discover(&format!("{}/other-server", resource_server.url), &challenge)
                .await
                .is_err()
        );
        let bad_issuer = Server::start(|base, request| {
            if request.target.contains("oauth-protected-resource") {
                Response::json(
                    json!({"resource":format!("{base}/mcp"),"authorization_servers":[base]}),
                )
            } else {
                Response::json(serde_json::to_value(metadata("https://other.example")).unwrap())
            }
        })
        .await;
        assert!(matches!(
            discover(
                &format!("{}/mcp", bad_issuer.url),
                &BearerChallenge::default()
            )
            .await,
            Err(ConnectionError::InvalidAuthorization)
        ));
    }
}
