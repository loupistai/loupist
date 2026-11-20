//! Fixed-address, bounded OpenFIGI transport and process-wide quota accounting.
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

use reqwest::{header, Client, ClientBuilder, Request};
use tokio::time::Instant;

use super::LookupStatus;

pub(super) const ROOT: &str = "https://api.openfigi.com";
const REQUEST_BYTES: usize = 8 * 1024;
const RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const WINDOW: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(120);
pub(super) const DEADLINE: Duration = Duration::from_secs(25);

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    Search,
}

impl Endpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Search => "/v3/search",
        }
    }

    fn allowance(self, keyed: bool) -> u32 {
        match (self, keyed) {
            (Self::Search, false) => 5,
            (Self::Search, true) => 20,
        }
    }
}

pub(super) struct Reply {
    pub status: u16,
    pub headers: header::HeaderMap,
    pub body: Vec<u8>,
}

#[async_trait::async_trait]
pub(super) trait Transport: Send + Sync {
    async fn send(&self, request: Request) -> Result<Reply, LookupStatus>;
}

pub(super) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub(super) struct RuntimeClock;

impl Clock for RuntimeClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub(super) struct Https;

fn client() -> Result<&'static Client, LookupStatus> {
    static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| client_builder().build().ok())
        .as_ref()
        .ok_or(LookupStatus::Unavailable)
}

fn client_builder() -> ClientBuilder {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .user_agent("Loupist")
        .connect_timeout(Duration::from_secs(10))
        .timeout(DEADLINE)
}

#[async_trait::async_trait]
impl Transport for Https {
    async fn send(&self, request: Request) -> Result<Reply, LookupStatus> {
        send(client()?, request).await
    }
}

async fn send(client: &Client, request: Request) -> Result<Reply, LookupStatus> {
    let mut response = client.execute(request).await.map_err(transport_error)?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut body = Vec::new();
    if status.is_success() {
        if response
            .content_length()
            .is_some_and(|size| size > RESPONSE_BYTES as u64)
        {
            return Err(LookupStatus::Partial);
        }
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len() + chunk.len() > RESPONSE_BYTES {
                return Err(LookupStatus::Partial);
            }
            body.extend_from_slice(&chunk);
        }
    }
    Ok(Reply {
        status: status.as_u16(),
        headers,
        body,
    })
}

fn transport_error(error: reqwest::Error) -> LookupStatus {
    if error.is_timeout() {
        LookupStatus::Timeout
    } else if error.is_connect() {
        LookupStatus::Offline
    } else {
        LookupStatus::Unavailable
    }
}

pub(super) fn request(
    endpoint: Endpoint,
    payload: &impl serde::Serialize,
    key: Option<&str>,
) -> Result<Request, LookupStatus> {
    let body = serde_json::to_vec(payload).map_err(|_| LookupStatus::InvalidQuery)?;
    if body.len() > REQUEST_BYTES {
        return Err(LookupStatus::InvalidQuery);
    }
    let mut builder = client()?
        .post(format!("{ROOT}{}", endpoint.path()))
        .body(body)
        .header(header::USER_AGENT, "Loupist")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        let mut value = header::HeaderValue::from_str(key).map_err(|_| LookupStatus::InvalidKey)?;
        value.set_sensitive(true);
        builder = builder.header("x-openfigi-apikey", value);
    }
    builder.build().map_err(|_| LookupStatus::Unavailable)
}

#[derive(Default)]
pub(super) struct Limiter {
    buckets: Mutex<[Bucket; 2]>,
}

#[derive(Default)]
struct Bucket {
    start: Option<Instant>,
    dispatched: u32,
    upstream: Option<(Instant, u32)>,
    blocked_until: Option<Instant>,
}

pub(super) struct Dispatch {
    keyed: bool,
    start: Instant,
}

impl Limiter {
    pub fn dispatch(
        &self,
        endpoint: Endpoint,
        keyed: bool,
        at: Instant,
    ) -> Result<Dispatch, LookupStatus> {
        let mut buckets = self.buckets.lock().expect("OpenFIGI limiter");
        let bucket = &mut buckets[usize::from(keyed)];
        if let Some(until) = bucket.blocked_until.filter(|until| *until > at) {
            return Err(busy(at, until));
        }
        if let Some((until, 0)) = bucket.upstream.filter(|(until, _)| *until > at) {
            return Err(busy(at, until));
        }
        let start = *bucket.start.get_or_insert(at);
        if at >= start + WINDOW {
            bucket.start = Some(at);
            bucket.dispatched = 0;
        }
        let start = bucket.start.expect("set above");
        if bucket.dispatched >= endpoint.allowance(keyed) {
            return Err(busy(at, start + WINDOW));
        }
        if let Some((until, remaining)) = &mut bucket.upstream {
            if *until > at {
                *remaining = remaining.saturating_sub(1);
            }
        }
        bucket.dispatched += 1;
        Ok(Dispatch { keyed, start })
    }

    pub fn observe(
        &self,
        dispatch: Dispatch,
        reply: &Reply,
        at: Instant,
    ) -> Result<(), LookupStatus> {
        let mut buckets = self.buckets.lock().expect("OpenFIGI limiter");
        let bucket = &mut buckets[usize::from(dispatch.keyed)];
        let seconds = |name| {
            reply
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        };
        let delay = |name| seconds(name).map(|value| Duration::from_secs(value).min(MAX_BACKOFF));
        let reset = delay("ratelimit-reset").filter(|delay| !delay.is_zero());
        let remaining = seconds("ratelimit-remaining").and_then(|value| u32::try_from(value).ok());
        if let (Some(remaining), Some(reset)) = (remaining, reset) {
            if bucket.start == Some(dispatch.start) {
                let until = at + reset;
                bucket.upstream = Some(match bucket.upstream {
                    Some((prior, quota)) if prior > at => (prior.max(until), quota.min(remaining)),
                    _ => (until, remaining),
                });
            }
        }
        if reply.status == 429 {
            let duration = delay("retry-after")
                .or(reset)
                .or_else(|| {
                    bucket
                        .upstream
                        .and_then(|(until, _)| until.checked_duration_since(at))
                        .filter(|duration| !duration.is_zero())
                })
                .or_else(|| {
                    (dispatch.start + WINDOW)
                        .checked_duration_since(at)
                        .filter(|duration| !duration.is_zero())
                })
                .unwrap_or(WINDOW);
            let until =
                (at + duration.max(Duration::from_secs(1))).max(bucket.blocked_until.unwrap_or(at));
            bucket.blocked_until = Some(until);
            return Err(busy(at, until));
        }
        match reply.status {
            200 => Ok(()),
            401 | 403 if dispatch.keyed => Err(LookupStatus::InvalidKey),
            _ => Err(LookupStatus::Unavailable),
        }
    }
}

fn busy(now: Instant, until: Instant) -> LookupStatus {
    let millis = (until - now).as_millis();
    LookupStatus::Busy {
        retry_after_seconds: millis.div_ceil(1000).max(1) as u64,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    #[test]
    fn search_limits_keyed_and_keyless_traffic_and_obeys_retry_after() {
        let limiter = Limiter::default();
        let now = Instant::now();
        for _ in 0..5 {
            assert!(limiter.dispatch(Endpoint::Search, false, now).is_ok());
        }
        assert_eq!(
            limiter.dispatch(Endpoint::Search, false, now).err(),
            Some(LookupStatus::Busy {
                retry_after_seconds: 60
            })
        );
        let lease = limiter.dispatch(Endpoint::Search, true, now).unwrap();
        let mut headers = header::HeaderMap::new();
        headers.insert(header::RETRY_AFTER, header::HeaderValue::from_static("90"));
        let reply = Reply {
            status: 429,
            headers,
            body: Vec::new(),
        };
        assert_eq!(
            limiter.observe(lease, &reply, now),
            Err(LookupStatus::Busy {
                retry_after_seconds: 90
            })
        );
        let next_window = now + WINDOW;
        assert!(limiter
            .dispatch(Endpoint::Search, false, next_window)
            .is_ok());
        assert_eq!(
            limiter.dispatch(Endpoint::Search, true, next_window).err(),
            Some(LookupStatus::Busy {
                retry_after_seconds: 30
            })
        );
        assert!(limiter
            .dispatch(Endpoint::Search, true, now + Duration::from_secs(90))
            .is_ok());
    }

    async fn serve(response: Vec<u8>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut headers = [0; 1024];
            if socket.read(&mut headers).await.is_ok_and(|n| n > 0) {
                // The consumer can close as soon as it detects a cap breach.
                let _ = socket.write_all(&response).await;
            }
        });
        address
    }

    fn loopback_client() -> Client {
        // Exercise the production transport policy over disposable local HTTP
        // sockets. The separate HTTPS-only check uses the real client.
        client_builder().https_only(false).build().unwrap()
    }

    async fn get(client: &Client, address: SocketAddr) -> Result<Reply, LookupStatus> {
        send(
            client,
            client.get(format!("http://{address}/")).build().unwrap(),
        )
        .await
    }

    #[test]
    fn request_cap_checks_serialized_bytes() {
        let exact = "a".repeat(REQUEST_BYTES - 2);
        let built = request(Endpoint::Search, &exact, None).unwrap();
        assert_eq!(
            built.body().unwrap().as_bytes().unwrap().len(),
            REQUEST_BYTES
        );
        assert_eq!(
            request(Endpoint::Search, &(exact + "a"), None).err(),
            Some(LookupStatus::InvalidQuery),
        );
    }

    #[tokio::test]
    async fn real_response_cap_checks_content_length_and_streamed_bytes() {
        let client = loopback_client();
        let declared = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            RESPONSE_BYTES + 1
        );
        let address = serve(declared.into_bytes()).await;
        assert_eq!(
            get(&client, address).await.err(),
            Some(LookupStatus::Partial)
        );

        for size in [RESPONSE_BYTES, RESPONSE_BYTES + 1] {
            let mut response =
                format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{size:x}\r\n")
                    .into_bytes();
            response.resize(response.len() + size, b'a');
            response.extend_from_slice(b"\r\n0\r\n\r\n");
            let address = serve(response).await;
            match get(&client, address).await {
                Ok(reply) => {
                    assert_eq!(size, RESPONSE_BYTES);
                    assert_eq!(reply.body.len(), RESPONSE_BYTES);
                }
                Err(status) => {
                    assert_eq!(size, RESPONSE_BYTES + 1);
                    assert_eq!(status, LookupStatus::Partial);
                }
            }
        }
    }

    #[tokio::test]
    async fn real_transport_refuses_redirects_and_cleartext() {
        let response = b"HTTP/1.1 302 Found\r\nLocation: /other\r\nContent-Length: 6\r\n\r\nsecret";
        let address = serve(response.to_vec()).await;
        let reply = get(&loopback_client(), address).await.unwrap();
        assert_eq!(reply.status, 302);
        assert!(reply.body.is_empty());

        let address = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
        assert_eq!(
            get(client().unwrap(), address).await.err(),
            Some(LookupStatus::Unavailable)
        );
    }

    #[tokio::test]
    async fn real_transport_times_out_mid_response_within_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request = tokio::spawn(async move { get(&loopback_client(), address).await });
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut headers = [0; 1024];
        assert!(socket.read(&mut headers).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n")
            .await
            .unwrap();

        tokio::time::pause();
        tokio::time::advance(DEADLINE + Duration::from_secs(1)).await;
        tokio::time::resume();
        let result = tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .expect("the transport deadline bounds the body read")
            .unwrap();
        assert_eq!(result.err(), Some(LookupStatus::Timeout));
    }

    #[tokio::test]
    async fn real_transport_bypasses_an_environment_proxy() {
        const PROXY_PROBE: &str = "LOUPIST_TEST_OPENFIGI_PROXY_CHILD";
        const PROBE_HOST: &str = "openfigi-proxy-test.invalid";
        if std::env::var_os(PROXY_PROBE).is_some() {
            let address =
                serve(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\ndirect".to_vec()).await;
            let client = client_builder()
                .https_only(false)
                .resolve(PROBE_HOST, address)
                .build()
                .unwrap();
            let request = client
                .get(format!("http://{PROBE_HOST}:{}/", address.port()))
                .build()
                .unwrap();
            assert_eq!(send(&client, request).await.unwrap().body, b"direct");
            return;
        }

        let address =
            serve(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n".to_vec()).await;
        let proxy = format!("http://{address}");
        // Isolate environment changes so parallel reqwest or unrelated tests
        // cannot observe the synthetic proxy.
        let child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "finance_data::openfigi::http::tests::real_transport_bypasses_an_environment_proxy",
            ])
            .env(PROXY_PROBE, "1")
            .env("HTTP_PROXY", &proxy)
            .env("http_proxy", &proxy)
            .env("ALL_PROXY", &proxy)
            .env("all_proxy", &proxy)
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output()
            .await
            .unwrap();
        let output = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && output.contains("1 passed"),
            "{output}"
        );
    }
}
