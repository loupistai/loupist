use super::*;

#[test]
fn external_links_allow_only_credential_free_web_urls() {
    assert!(validated_external_url("https://dart.fss.or.kr/dsab001/search.ax?q=005930").is_ok());
    assert!(validated_external_url("http://example.com/report").is_ok());
    for refused in [
        "file:///tmp/private",
        "javascript:alert(1)",
        "https://user:secret@example.com",
        "not a URL",
    ] {
        assert!(validated_external_url(refused).is_err(), "{refused}");
    }
    assert!(
        validated_external_url(&format!("https://example.com/{}", "a".repeat(8 * 1024))).is_err()
    );
}
