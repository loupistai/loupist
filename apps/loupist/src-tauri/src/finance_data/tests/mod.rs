use super::{world_bank::*, *};

mod live;
mod requests;

#[test]
fn finance_transport_errors_never_echo_a_credential_bearing_url() {
    let error = reqwest::Client::new()
        .get("http://[")
        .build()
        .unwrap_err()
        .with_url(reqwest::Url::parse("https://fixture.example/private/provider-route?api_key=private-fixture-key&authorization=private-fixture-header").unwrap());
    let error = FinanceDataError::from(error);
    assert_eq!(error.to_string(), "finance data provider request failed");
    let diagnostics = format!(
        "{error} {error:?} {}",
        std::error::Error::source(&error).unwrap()
    );
    for private in [
        "private-fixture-key",
        "private-fixture-header",
        "/private/provider-route",
    ] {
        assert!(!diagnostics.contains(private));
    }
}

#[tokio::test]
async fn a_read_that_never_reaches_its_server_is_offline_not_a_provider_failure() {
    // A closed local port refuses the connection before any HTTP exchange.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let error = reqwest::Client::new()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(
        FinanceDataError::from_send(&error, "fixture.provider"),
        FinanceDataError::Offline("fixture.provider")
    ));
}

#[tokio::test]
async fn declared_and_streamed_oversized_responses_return_the_limit() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let service = FinanceDataService::new().unwrap();
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n12345".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n123\r\n2\r\n45\r\n0\r\n\r\n",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(response).await.unwrap();
        });
        let result = service
            .send_provider_bounded(client.get(&url), "fixture.provider", url, 4)
            .await;
        server.await.unwrap();
        assert!(matches!(
            result,
            Err(FinanceDataError::ResultTooLarge { limit_bytes: 4 })
        ));
    }
}
