use super::*;
use crate::custom_connections::fixture::{Response, Server};

#[test]
fn only_https_and_named_loopback_http_are_allowed() {
    for url in [
        "https://server.example/mcp",
        "http://localhost:1234/mcp",
        "http://127.0.0.1:1234/mcp",
    ] {
        assert!(HttpTransport::new(url).is_ok());
    }
    for url in [
        "http://server.example/mcp",
        "http://192.168.1.2/mcp",
        "http://[::1]/mcp",
        "https://user:pass@server.example/mcp",
        "https://server.example/mcp#fragment",
        "https://server.example/mcp?token=private",
        "file:///tmp/mcp",
    ] {
        assert!(HttpTransport::new(url).is_err(), "{url}");
    }
}

#[tokio::test]
async fn streamable_http_uses_json_sse_session_and_protocol_headers() {
    let server = Server::start(|_, request| {
        let body = request.json();
        assert_eq!(request.headers["accept"], "application/json, text/event-stream");
        assert_eq!(request.headers["mcp-protocol-version"], MCP_PROTOCOL_VERSION);
        match body["method"].as_str().unwrap() {
            "initialize" => Response::json(json!({"jsonrpc":"2.0","id":body["id"],"result":{"protocolVersion":MCP_PROTOCOL_VERSION,"serverInfo":{"name":"fixture"},"capabilities":{"tools":{"listChanged":true}}}})).header("Mcp-Session-Id", "fixture-session"),
            "notifications/initialized" => { assert_eq!(request.headers["mcp-session-id"], "fixture-session"); Response::status(202) },
            "tools/list" => {
                assert_eq!(request.headers["mcp-session-id"], "fixture-session");
                Response { status: 200, headers: vec![("Content-Type".into(), "text/event-stream".into())], body: format!("data: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}}\r\n\r\ndata: {}\n\n", json!({"jsonrpc":"2.0","id":body["id"],"result":{"tools":[]}})) }
            }
            _ => Response::status(400),
        }
    }).await;
    let mut transport = HttpTransport::new(&format!("{}/mcp", server.url)).unwrap();
    assert!(matches!(
        transport
            .request("initialize", json!({}), None)
            .await
            .unwrap(),
        Reply::Result(_)
    ));
    transport
        .request("notifications/initialized", json!({}), None)
        .await
        .unwrap();
    assert!(
        matches!(transport.request("tools/list", json!({}), None).await.unwrap(), Reply::Result(value) if value["tools"] == json!([]))
    );
    assert!(transport.tools_changed);
}

#[tokio::test]
async fn cross_origin_redirects_are_refused_before_token_can_reach_second_server() {
    let second = Server::start(|_, _| Response::status(200)).await;
    let destination = format!("{}/mcp", second.url);
    let first =
        Server::start(move |_, _| Response::status(307).header("Location", &destination)).await;
    let resource = format!("{}/mcp", first.url);
    let mut transport = HttpTransport::new(&resource).unwrap();
    assert!(matches!(
        transport
            .request("initialize", json!({}), Some((&resource, "fixture-bearer")))
            .await,
        Err(ConnectionError::Redirect)
    ));
    assert!(second.requests.lock().unwrap().is_empty());
    let mut second_transport = HttpTransport::new(&format!("{}/mcp", second.url)).unwrap();
    assert!(matches!(
        second_transport
            .request("initialize", json!({}), Some((&resource, "fixture-bearer")))
            .await,
        Err(ConnectionError::InvalidAuthorization)
    ));
    assert!(second.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn malformed_ids_and_oversized_responses_fail_closed() {
    let server =
        Server::start(|_, _| Response::json(json!({"jsonrpc":"2.0","id":"wrong-id","result":{}})))
            .await;
    let mut transport = HttpTransport::new(&server.url).unwrap();
    assert!(matches!(
        transport.request("tools/list", json!({}), None).await,
        Err(ConnectionError::Protocol)
    ));
}

#[test]
fn remote_discovery_cannot_target_loopback_services() {
    for endpoint in [
        "http://localhost:1234/token",
        "http://127.0.0.1:1234/token",
        "https://127.1.2.3/token",
        "https://[::1]/token",
        "https://0.0.0.0/token",
        "https://[::]/token",
        "https://[::ffff:127.0.0.1]/token",
        "https://LOCALHOST./authorize",
        "https://auth.localhost/register",
    ] {
        assert!(
            checked_discovered_url(endpoint, "https://source.example/mcp").is_err(),
            "{endpoint}"
        );
    }
    assert!(
        checked_discovered_url("https://auth.example/token", "https://source.example/mcp").is_ok()
    );
    assert!(
        checked_discovered_url("http://localhost:1234/token", "http://127.0.0.1:5678/mcp").is_ok()
    );
}
