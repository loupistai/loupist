//! GitHub for sharing: OAuth device flow (scopes `public_repo gist`), the token
//! in the OS keyring, public repository and gist publishing, and public reads
//! for links. Every call runs here; the renderer sees the login only.
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

use super::pack::{Files, MAX_BYTES};

pub(crate) const SCOPES: &str = "public_repo gist";
pub(crate) const TOPIC: &str = "loupist-pack";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// The OAuth App client id, compiled in. Missing or empty hides publishing.
pub(crate) fn client_id() -> Option<&'static str> {
    option_env!("LOUPIST_GITHUB_CLIENT_ID").filter(|id| !id.trim().is_empty())
}

/// The community index URL, compiled in. Missing hides the Gallery and Report.
pub(crate) fn index_url() -> Option<&'static str> {
    option_env!("LOUPIST_COMMUNITY_INDEX_URL").filter(|url| url.starts_with("https://"))
}

pub(crate) fn gallery_terms_url() -> Option<&'static str> {
    option_env!("LOUPIST_GALLERY_TERMS_URL").filter(|url| super::pack::valid_terms_url(url))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApiError {
    Offline,
    LicenseNotShareable,
    NameTaken,
    RateLimited,
    /// 401: the token was revoked or never valid.
    Denied,
    NotFound,
    TooLarge,
    Other(String),
}

/// The final network boundary also refuses non-redistributable packs.
fn shareable_files(files: &Files) -> Result<(), ApiError> {
    let manifest: Value = files
        .get(super::pack::MANIFEST)
        .and_then(|bytes| serde_json::from_slice(bytes).ok())
        .ok_or_else(|| ApiError::Other("manifest".into()))?;
    let license = manifest["license"]
        .as_str()
        .and_then(super::pack::License::from_id)
        .ok_or_else(|| ApiError::Other("license".into()))?;
    if license.redistributable() {
        Ok(())
    } else {
        Err(ApiError::LicenseNotShareable)
    }
}

/// Endpoints. Production is github.com; e2e builds may point every call at a
/// local fixture server, and tests construct their own.
#[derive(Debug, Clone)]
pub(crate) struct Github {
    web: String,
    api: String,
    codeload: String,
    client: reqwest::Client,
}

impl Github {
    pub(crate) fn new(web: &str, api: &str, codeload: &str) -> Self {
        Self {
            web: web.trim_end_matches('/').into(),
            api: api.trim_end_matches('/').into(),
            codeload: codeload.trim_end_matches('/').into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Loupist-Sharing")
                .build()
                .expect("HTTP client"),
        }
    }

    pub(crate) fn production() -> Self {
        #[cfg(feature = "e2e")]
        if let Ok(base) = std::env::var("LOUPIST_E2E_GITHUB_BASE") {
            if base.starts_with("http://127.0.0.1:") || base.starts_with("http://localhost:") {
                return Self::new(&base, &format!("{base}/api"), &format!("{base}/codeload"));
            }
        }
        Self::new(
            "https://github.com",
            "https://api.github.com",
            "https://codeload.github.com",
        )
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: String,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let builder = self
            .client
            .request(method, url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        match token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    async fn send(&self, builder: reqwest::RequestBuilder) -> Result<(u16, Value), ApiError> {
        let response = builder.send().await.map_err(|_| ApiError::Offline)?;
        let status = response.status().as_u16();
        let limited = response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|value| value == "0");
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ApiError::TooLarge);
        }
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ApiError::Offline)?;
            if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
                return Err(ApiError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        match status {
            200..=299 => Ok((status, body)),
            401 => Err(ApiError::Denied),
            403 if limited => Err(ApiError::RateLimited),
            429 => Err(ApiError::RateLimited),
            404 => Err(ApiError::NotFound),
            422 if body.to_string().contains("already exists") => Err(ApiError::NameTaken),
            _ => Err(ApiError::Other(format!("GitHub returned {status}"))),
        }
    }

    pub(crate) async fn device_code(&self, client_id: &str) -> Result<DeviceCode, ApiError> {
        let builder = self
            .client
            .post(format!("{}/login/device/code", self.web))
            .header("Accept", "application/json")
            .form(&[("client_id", client_id), ("scope", SCOPES)]);
        let (_, body) = self.send(builder).await?;
        serde_json::from_value(body).map_err(|_| ApiError::Other("device code".into()))
    }

    pub(crate) async fn poll_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<Poll, ApiError> {
        let builder = self
            .client
            .post(format!("{}/login/oauth/access_token", self.web))
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ]);
        let (_, body) = self.send(builder).await?;
        if let Some(token) = body["access_token"].as_str().filter(|t| !t.is_empty()) {
            return Ok(Poll::Token(token.to_owned()));
        }
        Ok(match body["error"].as_str().unwrap_or_default() {
            "authorization_pending" => Poll::Pending,
            "slow_down" => Poll::SlowDown(body["interval"].as_u64()),
            "expired_token" => Poll::Expired,
            "access_denied" => Poll::Denied,
            _ => Poll::Failed,
        })
    }

    pub(crate) async fn login(&self, token: &str) -> Result<String, ApiError> {
        let (_, body) = self
            .send(self.request(
                reqwest::Method::GET,
                format!("{}/user", self.api),
                Some(token),
            ))
            .await?;
        body["login"]
            .as_str()
            .filter(|login| valid_login(login))
            .map(str::to_owned)
            .ok_or_else(|| ApiError::Other("user".into()))
    }

    pub(crate) async fn create_repo(
        &self,
        token: &str,
        name: &str,
        description: &str,
    ) -> Result<RepoInfo, ApiError> {
        let builder = self
            .request(
                reqwest::Method::POST,
                format!("{}/user/repos", self.api),
                Some(token),
            )
            .json(&json!({
                "name": name,
                "description": description,
                "private": false,
                "auto_init": true,
                "has_wiki": false,
            }));
        let (_, body) = self.send(builder).await?;
        repo_info(&body)
    }

    pub(crate) async fn repo(
        &self,
        token: Option<&str>,
        full_name: &str,
    ) -> Result<RepoInfo, ApiError> {
        let (_, body) = self
            .send(self.request(
                reqwest::Method::GET,
                format!("{}/repos/{full_name}", self.api),
                token,
            ))
            .await?;
        repo_info(&body)
    }

    /// The pack id in the repository's `loupist-pack.json` on `branch`, if any.
    pub(crate) async fn pack_id(
        &self,
        token: &str,
        full_name: &str,
        branch: &str,
    ) -> Result<Option<String>, ApiError> {
        let builder = self
            .request(
                reqwest::Method::GET,
                format!(
                    "{}/repos/{full_name}/contents/{}",
                    self.api,
                    super::pack::MANIFEST
                ),
                Some(token),
            )
            .query(&[("ref", branch)]);
        let body = match self.send(builder).await {
            Ok((_, body)) => body,
            Err(ApiError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let encoded: String = body["content"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            return Ok(None);
        };
        let manifest: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(manifest["id"].as_str().map(str::to_owned))
    }

    pub(crate) async fn pack_manifest(
        &self,
        token: &str,
        full_name: &str,
        commit: &str,
    ) -> Result<super::pack::Manifest, ApiError> {
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::GET,
                    format!(
                        "{}/repos/{full_name}/contents/{}",
                        self.api,
                        super::pack::MANIFEST
                    ),
                    Some(token),
                )
                .query(&[("ref", commit)]),
            )
            .await?;
        let encoded: String = body["content"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| ApiError::Other("manifest".into()))?;
        let manifest =
            super::pack::manifest_from(&bytes).map_err(|_| ApiError::Other("manifest".into()))?;
        super::pack::validate_manifest(&manifest)
            .map_err(|_| ApiError::Other("manifest".into()))?;
        Ok(manifest)
    }

    /// The recorded repository, only while it still holds this Loupist's own
    /// pack: Update never overwrites a repository that now holds something else.
    pub(crate) async fn recorded_repo(
        &self,
        token: &str,
        recorded: &str,
        pack_id: &str,
    ) -> Result<RepoInfo, RecordedError> {
        let repo = self
            .repo(Some(token), recorded)
            .await
            .map_err(RecordedError::Api)?;
        let found = self
            .pack_id(token, &repo.full_name, &repo.default_branch)
            .await
            .map_err(RecordedError::Api)?;
        if found.as_deref() != Some(pack_id) {
            return Err(RecordedError::Mismatch);
        }
        Ok(repo)
    }

    pub(crate) async fn set_topic(&self, token: &str, full_name: &str) -> Result<(), ApiError> {
        let builder = self
            .request(
                reqwest::Method::PUT,
                format!("{}/repos/{full_name}/topics", self.api),
                Some(token),
            )
            .json(&json!({"names": [TOPIC]}));
        self.send(builder).await.map(|_| ())
    }

    /// One commit that replaces the whole tree with `files`.
    pub(crate) async fn commit_files(
        &self,
        token: &str,
        full_name: &str,
        branch: &str,
        files: &Files,
        message: &str,
    ) -> Result<String, ApiError> {
        shareable_files(files)?;
        let mut head = None;
        for attempt in 0..5 {
            match self
                .send(self.request(
                    reqwest::Method::GET,
                    format!("{}/repos/{full_name}/git/ref/heads/{branch}", self.api),
                    Some(token),
                ))
                .await
            {
                Ok((_, body)) => {
                    head = body["object"]["sha"].as_str().map(str::to_owned);
                    break;
                }
                // A just-created repository can take a moment to get its first branch.
                Err(ApiError::NotFound | ApiError::Other(_)) if attempt < 4 => {
                    tokio::time::sleep(Duration::from_millis(400 * (attempt + 1))).await;
                }
                Err(error) => return Err(error),
            }
        }
        let head = head.ok_or(ApiError::NotFound)?;
        let mut tree = Vec::new();
        for (path, bytes) in files {
            let builder = self
                .request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/blobs", self.api),
                    Some(token),
                )
                .json(&json!({
                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "encoding": "base64",
                }));
            let (_, body) = self.send(builder).await?;
            let sha = body["sha"].as_str().ok_or(ApiError::Other("blob".into()))?;
            tree.push(json!({"path": path, "mode": "100644", "type": "blob", "sha": sha}));
        }
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/trees", self.api),
                    Some(token),
                )
                .json(&json!({"tree": tree})),
            )
            .await?;
        let tree_sha = body["sha"].as_str().ok_or(ApiError::Other("tree".into()))?;
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/commits", self.api),
                    Some(token),
                )
                .json(&json!({"message": message, "tree": tree_sha, "parents": [head]})),
            )
            .await?;
        let commit = body["sha"]
            .as_str()
            .ok_or(ApiError::Other("commit".into()))?
            .to_owned();
        self.send(
            self.request(
                reqwest::Method::PATCH,
                format!("{}/repos/{full_name}/git/refs/heads/{branch}", self.api),
                Some(token),
            )
            .json(&json!({"sha": commit, "force": false})),
        )
        .await?;
        Ok(commit)
    }

    pub(crate) async fn create_gist(
        &self,
        token: &str,
        files: &Files,
        description: &str,
        public: bool,
    ) -> Result<String, ApiError> {
        shareable_files(files)?;
        let mut map = serde_json::Map::new();
        for (path, bytes) in files {
            let content = String::from_utf8(bytes.clone())
                .map_err(|_| ApiError::Other("gist file".into()))?;
            map.insert(path.clone(), json!({ "content": content }));
        }
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/gists", self.api),
                    Some(token),
                )
                .json(&json!({"description": description, "public": public, "files": map})),
            )
            .await?;
        body["html_url"]
            .as_str()
            .map(str::to_owned)
            .ok_or(ApiError::Other("gist".into()))
    }

    /// The signed-in account's fork of `full_name` (created when missing).
    pub(crate) async fn fork(&self, token: &str, full_name: &str) -> Result<RepoInfo, ApiError> {
        let builder = self
            .request(
                reqwest::Method::POST,
                format!("{}/repos/{full_name}/forks", self.api),
                Some(token),
            )
            .json(&json!({"default_branch_only": true}));
        let (_, body) = self.send(builder).await?;
        repo_info(&body)
    }

    /// A branch head, waiting a moment for a repository GitHub is still creating.
    async fn head(&self, token: &str, full_name: &str, branch: &str) -> Result<String, ApiError> {
        for attempt in 0..5 {
            match self
                .send(self.request(
                    reqwest::Method::GET,
                    format!("{}/repos/{full_name}/git/ref/heads/{branch}", self.api),
                    Some(token),
                ))
                .await
            {
                Ok((_, body)) => {
                    return body["object"]["sha"]
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(ApiError::NotFound)
                }
                Err(ApiError::NotFound | ApiError::Other(_)) if attempt < 4 => {
                    tokio::time::sleep(Duration::from_millis(400 * (attempt + 1))).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(ApiError::NotFound)
    }

    /// One commit on `branch` (made from `base` when missing) that adds or
    /// replaces a single file and keeps the rest of the tree.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn commit_file(
        &self,
        token: &str,
        full_name: &str,
        base: &str,
        branch: &str,
        path: &str,
        bytes: &[u8],
        message: &str,
    ) -> Result<String, ApiError> {
        let head = self.head(token, full_name, base).await?;
        let (_, body) = self
            .send(self.request(
                reqwest::Method::GET,
                format!("{}/repos/{full_name}/git/commits/{head}", self.api),
                Some(token),
            ))
            .await?;
        let base_tree = body["tree"]["sha"]
            .as_str()
            .ok_or(ApiError::Other("tree".into()))?
            .to_owned();
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/blobs", self.api),
                    Some(token),
                )
                .json(&json!({
                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                    "encoding": "base64",
                })),
            )
            .await?;
        let blob = body["sha"].as_str().ok_or(ApiError::Other("blob".into()))?;
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/trees", self.api),
                    Some(token),
                )
                .json(&json!({
                    "base_tree": base_tree,
                    "tree": [{"path": path, "mode": "100644", "type": "blob", "sha": blob}],
                })),
            )
            .await?;
        let tree = body["sha"].as_str().ok_or(ApiError::Other("tree".into()))?;
        let (_, body) = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/commits", self.api),
                    Some(token),
                )
                .json(&json!({"message": message, "tree": tree, "parents": [head]})),
            )
            .await?;
        let commit = body["sha"]
            .as_str()
            .ok_or(ApiError::Other("commit".into()))?
            .to_owned();
        let created = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/git/refs", self.api),
                    Some(token),
                )
                .json(&json!({"ref": format!("refs/heads/{branch}"), "sha": commit})),
            )
            .await;
        match created {
            Ok(_) => {}
            // The branch is ours from an earlier request: move it to the new commit.
            Err(ApiError::NameTaken) => {
                self.send(
                    self.request(
                        reqwest::Method::PATCH,
                        format!("{}/repos/{full_name}/git/refs/heads/{branch}", self.api),
                        Some(token),
                    )
                    .json(&json!({"sha": commit, "force": true})),
                )
                .await?;
            }
            Err(error) => return Err(error),
        }
        Ok(commit)
    }

    /// Opens a pull request (or finds the open one for the same branch); its web address.
    pub(crate) async fn open_pull(
        &self,
        token: &str,
        full_name: &str,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<String, ApiError> {
        let opened = self
            .send(
                self.request(
                    reqwest::Method::POST,
                    format!("{}/repos/{full_name}/pulls", self.api),
                    Some(token),
                )
                .json(&json!({"title": title, "head": head, "base": base, "body": body, "maintainer_can_modify": false})),
            )
            .await;
        let url = match opened {
            Ok((_, body)) => body["html_url"].as_str().map(str::to_owned),
            Err(ApiError::NameTaken) => {
                let (_, existing) = self
                    .send(
                        self.request(
                            reqwest::Method::GET,
                            format!("{}/repos/{full_name}/pulls", self.api),
                            Some(token),
                        )
                        .query(&[("head", head), ("state", "open")]),
                    )
                    .await?;
                let number = existing[0]["number"]
                    .as_u64()
                    .ok_or_else(|| ApiError::Other("pull request".into()))?;
                self.send(
                    self.request(
                        reqwest::Method::PATCH,
                        format!("{}/repos/{full_name}/pulls/{number}", self.api),
                        Some(token),
                    )
                    .json(&json!({"title": title, "body": body})),
                )
                .await?;
                existing[0]["html_url"].as_str().map(str::to_owned)
            }
            Err(error) => return Err(error),
        };
        url.filter(|url| url.starts_with(&format!("https://github.com/{full_name}/pull/")))
            .ok_or(ApiError::Other("pull request".into()))
    }

    /// The commit a public reference points at.
    pub(crate) async fn resolve_commit(
        &self,
        token: Option<&str>,
        owner: &str,
        repo: &str,
        reference: Option<&str>,
    ) -> Result<String, ApiError> {
        let reference = match reference {
            Some(reference) => reference.to_owned(),
            None => {
                self.repo(token, &format!("{owner}/{repo}"))
                    .await?
                    .default_branch
            }
        };
        let (_, body) = self
            .send(self.request(
                reqwest::Method::GET,
                format!("{}/repos/{owner}/{repo}/commits/{reference}", self.api),
                token,
            ))
            .await?;
        body["sha"]
            .as_str()
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned)
            .ok_or(ApiError::NotFound)
    }

    /// The repository archive at one commit, bounded to 16 MiB, no redirects.
    pub(crate) async fn archive(
        &self,
        owner: &str,
        repo: &str,
        commit: &str,
    ) -> Result<Vec<u8>, ApiError> {
        use futures_util::StreamExt;
        let response = self
            .client
            .get(format!("{}/{owner}/{repo}/zip/{commit}", self.codeload))
            .send()
            .await
            .map_err(|_| ApiError::Offline)?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(ApiError::NotFound),
            429 | 403 => return Err(ApiError::RateLimited),
            status => return Err(ApiError::Other(format!("archive returned {status}"))),
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err(ApiError::TooLarge);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ApiError::Offline)?;
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err(ApiError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    /// A public gist's files, flat. The owner must match the link.
    pub(crate) async fn gist(
        &self,
        token: Option<&str>,
        user: &str,
        id: &str,
    ) -> Result<Files, ApiError> {
        let (_, body) = self
            .send(self.request(
                reqwest::Method::GET,
                format!("{}/gists/{id}", self.api),
                token,
            ))
            .await?;
        if !body["owner"]["login"]
            .as_str()
            .is_some_and(|login| login.eq_ignore_ascii_case(user))
        {
            return Err(ApiError::NotFound);
        }
        let mut files = Files::new();
        let mut total = 0;
        for (name, file) in body["files"].as_object().into_iter().flatten() {
            if file["truncated"].as_bool() == Some(true) {
                return Err(ApiError::TooLarge);
            }
            let content = file["content"].as_str().ok_or(ApiError::NotFound)?;
            total += content.len();
            if total > MAX_BYTES || files.len() >= super::pack::MAX_FILES {
                return Err(ApiError::TooLarge);
            }
            files.insert(name.clone(), content.as_bytes().to_vec());
        }
        Ok(files)
    }
}

/// Why the recorded repository cannot be updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordedError {
    Api(ApiError),
    /// It no longer holds this Loupist's pack (another id, or none).
    Mismatch,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Poll {
    Token(String),
    Pending,
    SlowDown(Option<u64>),
    Expired,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoInfo {
    pub full_name: String,
    pub html_url: String,
    pub default_branch: String,
}

fn repo_info(body: &Value) -> Result<RepoInfo, ApiError> {
    let full_name = body["full_name"].as_str().unwrap_or_default();
    let valid = full_name.split_once('/').is_some_and(|(owner, repo)| {
        valid_login(owner)
            && !repo.is_empty()
            && repo.len() <= 100
            && repo
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    });
    if !valid {
        return Err(ApiError::Other("repository".into()));
    }
    Ok(RepoInfo {
        full_name: full_name.to_owned(),
        html_url: format!("https://github.com/{full_name}"),
        default_branch: body["default_branch"]
            .as_str()
            .filter(|branch| !branch.is_empty() && branch.len() <= 100)
            .unwrap_or("main")
            .to_owned(),
    })
}

pub(crate) fn valid_login(login: &str) -> bool {
    (1..=39).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !login.starts_with('-')
}

/// A repository name GitHub accepts and we will create.
pub(crate) fn valid_repo_name(name: &str) -> bool {
    (1..=100).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && !matches!(name, "." | "..")
}

/// An unavailable credential store is different from an absent token.
/// Never retain native error text, which can include credential data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenStoreError;

#[cfg(all(
    any(target_os = "macos", windows),
    any(test, not(any(debug_assertions, feature = "webdriver")))
))]
fn token_from_password(result: keyring::Result<String>) -> Result<Option<String>, TokenStoreError> {
    match result {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(TokenStoreError),
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    any(target_os = "macos", windows)
))]
mod token_store {
    use super::TokenStoreError;

    fn entry(environment: &str) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(
            "com.monarchjuno.loupist.github-sharing",
            &format!("{environment}:github-sharing"),
        )
        .map_err(|_| TokenStoreError)
    }
    pub(super) fn load(environment: &str) -> Result<Option<String>, TokenStoreError> {
        super::token_from_password(entry(environment)?.get_password())
    }
    pub(super) fn save(environment: &str, token: &str) -> bool {
        entry(environment).is_ok_and(|entry| entry.set_password(token).is_ok())
    }
    pub(super) fn delete(environment: &str) {
        if let Ok(entry) = entry(environment) {
            let _ = entry.delete_credential();
        }
    }
}

/// Debug and test builds keep the token in process memory, like connector keys.
#[cfg(any(debug_assertions, test, feature = "webdriver"))]
mod token_store {
    use super::TokenStoreError;
    use std::{collections::HashMap, sync::Mutex};
    fn tokens() -> &'static Mutex<HashMap<String, String>> {
        static TOKENS: std::sync::OnceLock<Mutex<HashMap<String, String>>> =
            std::sync::OnceLock::new();
        TOKENS.get_or_init(Default::default)
    }
    pub(super) fn load(environment: &str) -> Result<Option<String>, TokenStoreError> {
        Ok(tokens()
            .lock()
            .map_err(|_| TokenStoreError)?
            .get(environment)
            .cloned())
    }
    pub(super) fn save(environment: &str, token: &str) -> bool {
        tokens()
            .lock()
            .map(|mut tokens| tokens.insert(environment.into(), token.into()))
            .is_ok()
    }
    pub(super) fn delete(environment: &str) {
        if let Ok(mut tokens) = tokens().lock() {
            tokens.remove(environment);
        }
    }
}

#[cfg(all(
    not(any(debug_assertions, test, feature = "webdriver")),
    not(any(target_os = "macos", windows))
))]
mod token_store {
    use super::TokenStoreError;

    pub(super) fn load(_: &str) -> Result<Option<String>, TokenStoreError> {
        Err(TokenStoreError)
    }
    pub(super) fn save(_: &str, _: &str) -> bool {
        false
    }
    pub(super) fn delete(_: &str) {}
}

pub(crate) fn load_token(environment: &str) -> Option<String> {
    // Public reads and account display may proceed signed out. The export
    // private-string scan must use the fallible entry point below.
    try_load_token(environment).ok().flatten()
}

pub(crate) fn try_load_token(environment: &str) -> Result<Option<String>, TokenStoreError> {
    token_store::load(environment)
}
pub(crate) fn save_token(environment: &str, token: &str) -> bool {
    token_store::save(environment, token)
}
pub(crate) fn delete_token(environment: &str) {
    token_store::delete(environment)
}

/// The signed-in login lives in the global catalog; the token never does.
pub(crate) fn stored_login(connection: &rusqlite::Connection) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_sharing_account (
            id INTEGER PRIMARY KEY CHECK (id = 1), login TEXT NOT NULL
         ) STRICT;",
    )?;
    connection
        .query_row(
            "SELECT login FROM github_sharing_account WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
}

pub(crate) fn store_login(
    connection: &rusqlite::Connection,
    login: Option<&str>,
) -> rusqlite::Result<()> {
    stored_login(connection)?;
    match login {
        Some(login) => connection.execute(
            "INSERT INTO github_sharing_account (id, login) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET login = excluded.login",
            [login],
        )?,
        None => connection.execute("DELETE FROM github_sharing_account", [])?,
    };
    Ok(())
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A tiny HTTP server that answers GitHub routes from a closure.
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    pub(crate) type Route = Arc<dyn Fn(&str, &str, &str) -> (u16, String) + Send + Sync>;
    pub(crate) type BytesRoute = Arc<dyn Fn(&str, &str, &str) -> (u16, Vec<u8>) + Send + Sync>;

    pub(crate) struct Fixture {
        pub base: String,
        pub requests: Arc<Mutex<Vec<(String, String, String)>>>,
    }

    pub(crate) async fn serve(route: Route) -> Fixture {
        serve_bytes(Arc::new(move |method, path, body| {
            let (status, reply) = route(method, path, body);
            (status, reply.into_bytes())
        }))
        .await
    }

    /// The same server for binary replies (archives, images).
    pub(crate) async fn serve_bytes(route: BytesRoute) -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let route = route.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 8192];
                    loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buffer.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buffer).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let length = text[..end]
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if buffer.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    let text = String::from_utf8_lossy(&buffer).to_string();
                    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                    let mut words = head.split_whitespace();
                    let method = words.next().unwrap_or_default().to_owned();
                    let path = words.next().unwrap_or_default().to_owned();
                    seen.lock()
                        .unwrap()
                        .push((method.clone(), path.clone(), body.to_owned()));
                    let (status, reply) = route(&method, &path, body);
                    let head = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        reply.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&reply).await;
                });
            }
        });
        Fixture { base, requests }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn keychain_token_reads_distinguish_absence_from_unavailable_and_malformed_credentials() {
        assert_eq!(token_from_password(Err(keyring::Error::NoEntry)), Ok(None));
        assert_eq!(
            token_from_password(Ok("pine".into())),
            Ok(Some("pine".into()))
        );
        for error in [
            keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("locked"))),
            keyring::Error::BadEncoding(b"private-token-fixture".to_vec()),
        ] {
            let result = token_from_password(Err(error));
            assert_eq!(result, Err(TokenStoreError));
            assert!(!format!("{result:?}").contains("private-token-fixture"));
        }
    }

    #[test]
    fn fallible_token_reads_preserve_the_optional_public_read_api() {
        let environment = format!("token-test-{}", uuid::Uuid::new_v4());
        assert_eq!(try_load_token(&environment), Ok(None));
        assert!(save_token(&environment, "pine"));
        assert_eq!(try_load_token(&environment), Ok(Some("pine".into())));
        assert_eq!(load_token(&environment), Some("pine".into()));
        delete_token(&environment);
        assert_eq!(try_load_token(&environment), Ok(None));
    }
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn publishing_never_touches_an_existing_repository_even_if_it_is_empty() {
        for name in ["preexisting", "empty", "content", "unknown", "denied"] {
            let server = fixture::serve(Arc::new(|method, path, _| {
                assert_eq!((method, path), ("POST", "/api/user/repos"));
                (422, r#"{"message":"already exists"}"#.into())
            }))
            .await;
            let api = Github::new(&server.base, &format!("{}/api", server.base), &server.base);
            assert_eq!(
                api.create_repo("t", name, "d").await,
                Err(ApiError::NameTaken)
            );
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0, "POST");
            assert_eq!(requests[0].1, "/api/user/repos");
        }
    }

    #[tokio::test]
    async fn github_rejects_oversized_responses_before_the_body_finishes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for chunked in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).await.unwrap() > 0);
                if chunked {
                    let _ = socket
                        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                        .await;
                    let _ = socket
                        .write_all(format!("{:x}\r\n", MAX_RESPONSE_BYTES + 1).as_bytes())
                        .await;
                    let _ = socket.write_all(&vec![b' '; MAX_RESPONSE_BYTES + 1]).await;
                    let _ = socket.write_all(b"\r\n").await;
                } else {
                    let _ = socket
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                MAX_RESPONSE_BYTES + 1
                            )
                            .as_bytes(),
                        )
                        .await;
                }
                // An unbounded reader would wait for the rest of this response.
                std::future::pending::<()>().await;
            });
            let api = Github::new(&base, &base, &base);
            let result =
                tokio::time::timeout(Duration::from_secs(5), api.login("fixture-token")).await;
            server.abort();
            assert_eq!(
                result.unwrap(),
                Err(ApiError::TooLarge),
                "chunked={chunked}"
            );
        }
    }

    #[tokio::test]
    async fn device_flow_waits_slows_down_and_returns_the_login() {
        let polls = Arc::new(AtomicUsize::new(0));
        let count = polls.clone();
        let server = fixture::serve(Arc::new(move |method, path, body| match (method, path) {
            ("POST", "/login/device/code") => {
                let form: std::collections::HashMap<String, String> =
                    reqwest::Url::parse(&format!("https://example.test/?{body}"))
                        .unwrap().query_pairs().into_owned().collect();
                assert_eq!(form.get("scope").map(String::as_str), Some("public_repo gist"));
                (200, r#"{"device_code":"dev-1","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":0}"#.into())
            }
            ("POST", "/login/oauth/access_token") => match count.fetch_add(1, Ordering::SeqCst) {
                0 => (200, r#"{"error":"authorization_pending"}"#.into()),
                1 => (200, r#"{"error":"slow_down","interval":10}"#.into()),
                _ => (200, r#"{"access_token":"gho_fixturetoken","token_type":"bearer"}"#.into()),
            },
            ("GET", "/api/user") => (200, r#"{"login":"kimj"}"#.into()),
            _ => (404, "{}".into()),
        }))
        .await;
        let github = Github::new(&server.base, &format!("{}/api", server.base), &server.base);
        let code = github.device_code("client-fixture").await.unwrap();
        assert_eq!(code.user_code, "WDJB-MJHT");
        assert_eq!(
            github.poll_token("client-fixture", "dev-1").await.unwrap(),
            Poll::Pending
        );
        assert_eq!(
            github.poll_token("client-fixture", "dev-1").await.unwrap(),
            Poll::SlowDown(Some(10))
        );
        let Poll::Token(token) = github.poll_token("client-fixture", "dev-1").await.unwrap() else {
            panic!("no token");
        };
        assert_eq!(github.login(&token).await.unwrap(), "kimj");
        assert_eq!(polls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn publishing_creates_a_public_repo_with_the_topic_and_one_commit() {
        let server = fixture::serve(Arc::new(|method, path, body| match (method, path) {
            ("POST", "/api/user/repos") => {
                let request: Value = serde_json::from_str(body).unwrap();
                assert_eq!(request["private"], false);
                assert_eq!(request["auto_init"], true);
                if request["name"] == "taken" {
                    return (422, r#"{"message":"Repository creation failed.","errors":[{"message":"name already exists on this account"}]}"#.into());
                }
                (201, r#"{"full_name":"kimj/loupist-desk","default_branch":"main"}"#.into())
            }
            ("PUT", "/api/repos/kimj/loupist-desk/topics") => (200, r#"{"names":["loupist-pack"]}"#.into()),
            ("GET", "/api/repos/kimj/loupist-desk/git/ref/heads/main") => (200, r#"{"object":{"sha":"base"}}"#.into()),
            ("POST", "/api/repos/kimj/loupist-desk/git/blobs") => (201, r#"{"sha":"blob"}"#.into()),
            ("POST", "/api/repos/kimj/loupist-desk/git/trees") => (201, r#"{"sha":"tree"}"#.into()),
            ("POST", "/api/repos/kimj/loupist-desk/git/commits") => {
                assert!(body.contains("Loupist pack v1"));
                (201, r#"{"sha":"commit-1"}"#.into())
            }
            ("PATCH", "/api/repos/kimj/loupist-desk/git/refs/heads/main") => (200, "{}".into()),
            ("GET", "/api/repos/kimj/gone") => (404, "{}".into()),
            ("POST", "/api/gists") => (401, "{}".into()),
            _ => (500, "{}".into()),
        }))
        .await;
        let github = Github::new(&server.base, &format!("{}/api", server.base), &server.base);
        assert_eq!(
            github.create_repo("t", "taken", "d").await.unwrap_err(),
            ApiError::NameTaken
        );
        let repo = github.create_repo("t", "loupist-desk", "d").await.unwrap();
        assert_eq!(repo.html_url, "https://github.com/kimj/loupist-desk");
        github.set_topic("t", &repo.full_name).await.unwrap();
        let files = Files::from([
            ("README.md".to_owned(), b"# Desk\n".to_vec()),
            (
                "loupist-pack.json".to_owned(),
                br#"{"license":"CC-BY-4.0"}"#.to_vec(),
            ),
        ]);
        let commit = github
            .commit_files("t", &repo.full_name, "main", &files, "Loupist pack v1")
            .await
            .unwrap();
        assert_eq!(commit, "commit-1");
        assert_eq!(
            github.repo(Some("t"), "kimj/gone").await.unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            github
                .create_gist("t", &files, "d", false)
                .await
                .unwrap_err(),
            ApiError::Denied
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(method, path, _)| method == "POST" && path.ends_with("/git/blobs"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn update_refuses_a_recorded_repository_that_holds_another_pack() {
        let manifest = |id: &str| {
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{{\"format\":\"loupist-pack/1\",\"id\":\"{id}\"}}"));
            format!(r#"{{"content":"{}","encoding":"base64"}}"#, encoded)
        };
        let ours = manifest("7f1d1c3e-0000-4000-8000-000000000001");
        let theirs = manifest("7f1d1c3e-0000-4000-8000-000000000002");
        let server = fixture::serve(Arc::new(move |method, path, _| {
            let repo = |name: &str| {
                (
                    200,
                    format!(r#"{{"full_name":"kimj/{name}","default_branch":"main"}}"#),
                )
            };
            match (method, path) {
                ("GET", "/api/repos/kimj/ours") => repo("ours"),
                ("GET", "/api/repos/kimj/replaced") => repo("replaced"),
                ("GET", "/api/repos/kimj/emptied") => repo("emptied"),
                ("GET", "/api/repos/kimj/ours/contents/loupist-pack.json?ref=main") => {
                    (200, ours.clone())
                }
                ("GET", "/api/repos/kimj/replaced/contents/loupist-pack.json?ref=main") => {
                    (200, theirs.clone())
                }
                _ => (404, "{}".into()),
            }
        }))
        .await;
        let github = Github::new(&server.base, &format!("{}/api", server.base), &server.base);
        let pack = "7f1d1c3e-0000-4000-8000-000000000001";
        assert_eq!(
            github
                .recorded_repo("t", "kimj/ours", pack)
                .await
                .unwrap()
                .full_name,
            "kimj/ours"
        );
        for other in ["kimj/replaced", "kimj/emptied"] {
            assert_eq!(
                github.recorded_repo("t", other, pack).await.unwrap_err(),
                RecordedError::Mismatch,
                "{other}"
            );
        }
        assert_eq!(
            github
                .recorded_repo("t", "kimj/gone", pack)
                .await
                .unwrap_err(),
            RecordedError::Api(ApiError::NotFound)
        );
    }

    #[tokio::test]
    async fn a_gallery_listing_writes_one_entry_file_on_its_fork_and_opens_a_pull_request() {
        let branch = "loupist-kimj__loupist-desk";
        let server = fixture::serve(Arc::new(move |method, path, body| {
            let reply = |status: u16, body: &str| (status, body.to_owned());
            match (method, path) {
                ("POST", "/api/repos/loupist/community/forks") => {
                    reply(202, r#"{"full_name":"kimj/community","default_branch":"main"}"#)
                }
                ("GET", "/api/repos/kimj/community/git/ref/heads/main") => {
                    reply(200, r#"{"object":{"sha":"base"}}"#)
                }
                ("GET", "/api/repos/kimj/community/git/commits/base") => {
                    reply(200, r#"{"tree":{"sha":"base-tree"}}"#)
                }
                ("POST", "/api/repos/kimj/community/git/blobs") => reply(201, r#"{"sha":"blob"}"#),
                ("POST", "/api/repos/kimj/community/git/trees") => {
                    let request: Value = serde_json::from_str(body).unwrap();
                    // The rest of the index stays: one file on top of the fork's tree.
                    assert_eq!(request["base_tree"], "base-tree");
                    assert_eq!(request["tree"].as_array().unwrap().len(), 1);
                    assert_eq!(request["tree"][0]["path"], "entries/kimj__loupist-desk.json");
                    reply(201, r#"{"sha":"tree"}"#)
                }
                ("POST", "/api/repos/kimj/community/git/commits") => reply(201, r#"{"sha":"commit-1"}"#),
                ("POST", "/api/repos/kimj/community/git/refs") => {
                    reply(422, r#"{"message":"Reference already exists"}"#)
                }
                ("PATCH", "/api/repos/kimj/community/git/refs/heads/loupist-kimj__loupist-desk") => {
                    reply(200, "{}")
                }
                ("POST", "/api/repos/loupist/community/pulls") => reply(
                    422,
                    r#"{"message":"Validation Failed","errors":[{"message":"A pull request already exists for kimj:loupist-kimj__loupist-desk."}]}"#,
                ),
                ("GET", path) if path.starts_with("/api/repos/loupist/community/pulls?") => {
                    reply(200, r#"[{"number":7,"html_url":"https://github.com/loupist/community/pull/7"}]"#)
                }
                ("PATCH", "/api/repos/loupist/community/pulls/7") => {
                    let request: Value = serde_json::from_str(body).unwrap();
                    let body = request["body"].as_str().unwrap();
                    assert!(body.contains("https://example.org/gallery-terms"));
                    assert!(body.contains("2026-11-01T10:11:12Z"));
                    reply(200, "{}")
                }
                _ => reply(500, "{}"),
            }
        }))
        .await;
        let github = Github::new(&server.base, &format!("{}/api", server.base), &server.base);
        let fork = github.fork("t", "loupist/community").await.unwrap();
        assert_eq!(fork.full_name, "kimj/community");
        let (path, bytes) = crate::sharing::gallery::listing_entry(
            "kimj/loupist-desk",
            "",
            "0123456789abcdef0123456789abcdef01234567",
        )
        .unwrap();
        let commit = github
            .commit_file(
                "t",
                &fork.full_name,
                "main",
                branch,
                &path,
                &bytes,
                "List kimj/loupist-desk",
            )
            .await
            .unwrap();
        assert_eq!(commit, "commit-1");
        let url = github
            .open_pull(
                "t",
                "loupist/community",
                &format!("kimj:{branch}"),
                "main",
                "List",
                &crate::sharing::commands::gallery::listing_body(
                    "https://example.org/gallery-terms",
                    "2026-11-01T10:11:12Z",
                ),
            )
            .await
            .unwrap();
        assert_eq!(url, "https://github.com/loupist/community/pull/7");
        // Writes go to its own fork, plus the fork and pull request on the index.
        let requests = server.requests.lock().unwrap();
        for (method, path, _) in requests.iter().filter(|(method, _, _)| method != "GET") {
            assert!(
                path.starts_with("/api/repos/kimj/community/")
                    || path == "/api/repos/loupist/community/forks"
                    || path == "/api/repos/loupist/community/pulls"
                    || path == "/api/repos/loupist/community/pulls/7",
                "{method} {path}"
            );
        }
    }

    #[test]
    fn names_and_logins_are_bounded() {
        assert!(valid_repo_name("loupist-quality-compounder"));
        assert!(!valid_repo_name("../x"));
        assert!(!valid_repo_name(""));
        assert!(valid_login("kimj"));
        assert!(!valid_login("kim/j"));
    }
}
