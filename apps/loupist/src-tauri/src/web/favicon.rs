use super::{request_bounded, validate_url_shape, Method, Url};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::time::Duration;
use tokio::sync::Semaphore;

const MAX_BYTES: usize = 128 * 1024;
static REQUESTS: Semaphore = Semaphore::const_new(4);

/// Decorative site identity only. Never fetch a caller-selected path or forward
/// cookies; reuse the public-address and redirect checks of ordinary web reads.
#[tauri::command(rename_all = "snake_case")]
pub async fn site_favicon(url: String) -> Option<String> {
    if url.len() > 8192 {
        return None;
    }
    let mut url = Url::parse(&url).ok()?;
    validate_url_shape(&url).ok()?;
    url.set_path("/favicon.ico");
    url.set_query(None);
    url.set_fragment(None);
    tokio::time::timeout(Duration::from_secs(10), async {
        let _permit = REQUESTS.acquire().await.ok()?;
        let response = request_bounded(
            Method::GET,
            url,
            None,
            Duration::from_secs(5),
            MAX_BYTES,
            "image/x-icon, image/vnd.microsoft.icon, image/png, image/webp, image/jpeg, image/gif",
        )
        .await
        .ok()?;
        if !response.status.is_success() || response.bytes.len() > MAX_BYTES {
            return None;
        }
        let mime = raster_type(&response.bytes)?;
        Some(format!(
            "data:{mime};base64,{}",
            STANDARD.encode(response.bytes)
        ))
    })
    .await
    .ok()
    .flatten()
}

fn raster_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::script;

    #[tokio::test]
    async fn favicon_uses_only_the_origin_and_returns_inline_raster_bytes() {
        let bytes = b"\x89PNG\r\n\x1a\nicon";
        let _guard = script::install(vec![(
            "https://example.com/favicon.ico",
            vec![script::ok("application/octet-stream", bytes.to_vec())],
        )]);
        assert_eq!(
            site_favicon("https://example.com/private/report?token=secret#section".into()).await,
            Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
        );
    }

    #[tokio::test]
    async fn favicon_refuses_non_rasters_oversized_bodies_and_private_redirects() {
        assert_eq!(
            site_favicon("https://user:password@example.com".into()).await,
            None
        );
        assert_eq!(site_favicon("file:///etc/passwd".into()).await, None);
        let _guard = script::install(vec![(
            "https://example.com/favicon.ico",
            vec![
                script::ok("image/png", b"<svg onload='alert(1)'/>".to_vec()),
                script::ok("image/png", vec![0; MAX_BYTES + 1]),
                script::ScriptedResponse {
                    challenge: false,
                    status: 302,
                    content_type: String::new(),
                    body: Vec::new(),
                    retry_after: None,
                    location: Some("http://127.0.0.1/private".into()),
                },
            ],
        )]);
        for _ in 0..3 {
            assert_eq!(site_favicon("https://example.com".into()).await, None);
        }
    }
}
