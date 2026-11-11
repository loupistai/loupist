use std::{collections::BTreeSet, time::Duration};

use reqwest::{header, Client, Response, StatusCode, Url};
use serde_json::{json, Value};

use super::ConnectionError;
use crate::mcp::{McpTool, MCP_PROTOCOL_VERSION, MCP_SUPPORTED_PROTOCOL_VERSIONS};

// The session transport negotiates the host-wide MCP revision; OAuth
// independently follows the 2026-07-28 authorization specification.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub fn checked_url(value: &str) -> Result<Url, ConnectionError> {
    let url = Url::parse(value).map_err(|_| ConnectionError::InvalidUrl)?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
    {
        return Err(ConnectionError::InvalidUrl);
    }
    Ok(url)
}

/// Remote discovery cannot turn a user-approved remote connection into a
/// request to a local service. Explicit local connections retain local OAuth.
pub fn checked_discovered_url(value: &str, resource: &str) -> Result<Url, ConnectionError> {
    fn loopback(url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let host = host.trim_end_matches('.');
        host == "localhost"
            || host.ends_with(".localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| match ip {
                    std::net::IpAddr::V4(ip) => ip.is_loopback() || ip.is_unspecified(),
                    std::net::IpAddr::V6(ip) => {
                        ip.is_loopback()
                            || ip.is_unspecified()
                            || ip
                                .to_ipv4()
                                .is_some_and(|ip| ip.is_loopback() || ip.is_unspecified())
                    }
                })
    }
    let url = checked_url(value)?;
    if loopback(&url) && !loopback(&checked_url(resource)?) {
        return Err(ConnectionError::InvalidAuthorization);
    }
    Ok(url)
}

pub fn canonical_resource(value: &str) -> Result<String, ConnectionError> {
    let url = checked_url(value)?;
    // Query parameters can contain credentials and are not resource identifiers.
    if url.query().is_some() {
        return Err(ConnectionError::InvalidUrl);
    }
    Ok(if url.path() == "/" {
        url.as_str().trim_end_matches('/').to_owned()
    } else {
        url.to_string()
    })
}

pub fn client() -> Result<Client, ConnectionError> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| ConnectionError::Network)
}

pub async fn bounded_body(mut response: Response) -> Result<Vec<u8>, ConnectionError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ConnectionError::Protocol);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ConnectionError::Network)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(ConnectionError::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub async fn json_response(response: Response) -> Result<Value, ConnectionError> {
    if response.status().is_redirection() {
        return Err(ConnectionError::Redirect);
    }
    if !response.status().is_success() {
        return Err(ConnectionError::Authorization);
    }
    serde_json::from_slice(&bounded_body(response).await?).map_err(|_| ConnectionError::Protocol)
}

// Deliberately no Debug or Serialize: challenge values and token-bound sessions
// never enter renderer errors or traces.
pub struct Challenge {
    pub status: StatusCode,
    pub headers: Vec<String>,
}

pub enum Reply {
    Result(Value),
    Challenge(Challenge),
}

pub struct HttpTransport {
    client: Client,
    resource: String,
    session_id: Option<header::HeaderValue>,
    protocol_version: String,
    headers: header::HeaderMap,
    pub tools_changed: bool,
}

impl HttpTransport {
    pub fn new(url: &str) -> Result<Self, ConnectionError> {
        Ok(Self {
            client: client()?,
            resource: canonical_resource(url)?,
            session_id: None,
            protocol_version: MCP_PROTOCOL_VERSION.into(),
            headers: header::HeaderMap::new(),
            tools_changed: false,
        })
    }

    pub fn resource(&self) -> &str {
        &self.resource
    }

    pub fn set_headers(
        &mut self,
        values: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), ConnectionError> {
        let mut headers = header::HeaderMap::new();
        for (name, value) in values {
            let name = header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ConnectionError::Configuration)?;
            let mut value =
                header::HeaderValue::from_str(value).map_err(|_| ConnectionError::Configuration)?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        self.headers = headers;
        Ok(())
    }

    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
        bearer: Option<(&str, &str)>,
    ) -> Result<Reply, ConnectionError> {
        let id = uuid::Uuid::new_v4().to_string();
        let notification = method == "notifications/initialized";
        let mut body = json!({"jsonrpc":"2.0","method":method,"params":params});
        if !notification {
            body["id"] = json!(id);
        }
        let mut request = self
            .client
            .post(&self.resource)
            .headers(self.headers.clone())
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &self.protocol_version)
            .json(&body);
        if let Some(session_id) = &self.session_id {
            request = request.header("Mcp-Session-Id", session_id);
        }
        if let Some((resource, token)) = bearer {
            if resource != self.resource {
                return Err(ConnectionError::InvalidAuthorization);
            }
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        if response.status().is_redirection() {
            return Err(ConnectionError::Redirect);
        }
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Ok(Reply::Challenge(Challenge {
                status: response.status(),
                headers: response
                    .headers()
                    .get_all(header::WWW_AUTHENTICATE)
                    .iter()
                    .filter_map(|v| v.to_str().ok().map(str::to_owned))
                    .collect(),
            }));
        }
        if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
            // MCP uses 404 for a session that the server no longer recognizes.
            return Err(ConnectionError::Transport);
        }
        if !response.status().is_success() {
            return Err(ConnectionError::Protocol);
        }
        if let Some(session) = response.headers().get("Mcp-Session-Id") {
            if session.as_bytes().is_empty()
                || session.as_bytes().len() > 1024
                || !session.as_bytes().iter().all(|b| (0x21..=0x7e).contains(b))
            {
                return Err(ConnectionError::Protocol);
            }
            if method == "initialize" {
                self.session_id = Some(session.clone());
            } else if self.session_id.as_ref() != Some(session) {
                return Err(ConnectionError::Protocol);
            }
        }
        if notification {
            return Ok(Reply::Result(Value::Null));
        }
        let sse = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next() == Some("text/event-stream"));
        let value = if sse {
            self.read_sse(response, &id).await?
        } else {
            let bytes = bounded_body(response).await.map_err(|error| match error {
                ConnectionError::Network => ConnectionError::Transport,
                error => error,
            })?;
            serde_json::from_slice(&bytes).map_err(|_| ConnectionError::Protocol)?
        };
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || value.get("id").and_then(Value::as_str) != Some(id.as_str())
        {
            return Err(ConnectionError::Protocol);
        }
        let result = value
            .get("result")
            .cloned()
            .ok_or(ConnectionError::Protocol)?;
        if method == "initialize" {
            if !result.get("capabilities").is_some_and(Value::is_object)
                || !result
                    .get("serverInfo")
                    .and_then(|info| info.get("name"))
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty() && name.len() <= 128)
            {
                return Err(ConnectionError::Protocol);
            }
            let version = result
                .get("protocolVersion")
                .and_then(Value::as_str)
                .ok_or(ConnectionError::Protocol)?;
            if !MCP_SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
                return Err(ConnectionError::Protocol);
            }
            self.protocol_version = version.into();
        }
        Ok(Reply::Result(result))
    }

    async fn read_sse(
        &mut self,
        mut response: Response,
        id: &str,
    ) -> Result<Value, ConnectionError> {
        let mut buffer = Vec::new();
        let mut total = 0usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ConnectionError::Transport)?
        {
            total = total.saturating_add(chunk.len());
            if total > MAX_RESPONSE_BYTES {
                return Err(ConnectionError::Protocol);
            }
            buffer.extend_from_slice(&chunk);
            while let Some((offset, delimiter)) = [
                buffer.windows(2).position(|w| w == b"\n\n").map(|p| (p, 2)),
                buffer
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| (p, 4)),
            ]
            .into_iter()
            .flatten()
            .min_by_key(|(offset, _)| *offset)
            {
                let event = String::from_utf8(buffer.drain(..offset + delimiter).collect())
                    .map_err(|_| ConnectionError::Protocol)?;
                let data = event
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("data:")
                            .map(|s| s.strip_prefix(' ').unwrap_or(s))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                let message: Value =
                    serde_json::from_str(&data).map_err(|_| ConnectionError::Protocol)?;
                if message.get("method").and_then(Value::as_str)
                    == Some("notifications/tools/list_changed")
                {
                    self.tools_changed = true;
                }
                if message.get("id").and_then(Value::as_str) == Some(id) {
                    return Ok(message);
                }
                // Server-originated requests (sampling, elicitation, roots) are never serviced.
                if message.get("id").is_some() {
                    return Err(ConnectionError::Protocol);
                }
            }
        }
        Err(ConnectionError::Transport)
    }
}

#[cfg(test)]
mod tests;

pub fn parse_inventory_page(
    value: &Value,
    tools: &mut Vec<McpTool>,
    cursors: &mut BTreeSet<String>,
) -> Result<Option<String>, ConnectionError> {
    let page = value
        .get("tools")
        .and_then(Value::as_array)
        .ok_or(ConnectionError::Protocol)?;
    for value in page {
        if serde_json::to_vec(value)
            .map_err(|_| ConnectionError::Protocol)?
            .len()
            > 128 * 1024
        {
            return Err(ConnectionError::Protocol);
        }
        let tool: McpTool =
            serde_json::from_value(value.clone()).map_err(|_| ConnectionError::Protocol)?;
        crate::mcp::validate_tool(&tool).map_err(|_| ConnectionError::Protocol)?;
        if tools.len() >= 512 || tools.iter().any(|previous| previous.name == tool.name) {
            return Err(ConnectionError::Protocol);
        }
        tools.push(tool);
    }
    match value.get("nextCursor") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(cursor))
            if !page.is_empty()
                && !cursor.is_empty()
                && cursor.len() <= 512
                && cursors.insert(cursor.clone()) =>
        {
            Ok(Some(cursor.clone()))
        }
        _ => Err(ConnectionError::Protocol),
    }
}
