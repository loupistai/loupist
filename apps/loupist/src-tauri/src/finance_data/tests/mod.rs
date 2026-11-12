use super::{world_bank::*, *};

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
