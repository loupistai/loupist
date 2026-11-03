use super::{world_bank::*, *};

mod requests;

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
