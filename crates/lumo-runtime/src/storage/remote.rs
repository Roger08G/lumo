use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
    thread,
    time::Duration,
};

use lumo_core::{
    domain::RuntimeState,
    ports::StateRepository,
    security::{ReplayGuard, SessionCipher},
    LumoError, LumoResult,
};
use lumo_protocol::{
    derive_state_key, ApiErrorBody, CompactPutStateRequest, CompactRemoteStateRecord,
    PutStateRequest, RemoteStateRecord, RequestAuthenticator, COMPACT_STATE_PATH, STATE_PATH,
};
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
    redirect::Policy,
    tls::Version,
    Method, StatusCode, Url,
};

use crate::config::RuntimeConfig;

const TIMESTAMP_HEADER: &str = "x-lumo-timestamp";
const NONCE_HEADER: &str = "x-lumo-nonce";
const SIGNATURE_HEADER: &str = "x-lumo-signature";
const MAX_TRANSPORT_ATTEMPTS: usize = 3;
const CAPABILITY_UNKNOWN: u8 = 0;
const CAPABILITY_LEGACY: u8 = 1;
const CAPABILITY_COMPACT: u8 = 2;

type RemoteStateKey = (String, u64);
type SharedRemoteStates = Mutex<HashMap<RemoteStateKey, Arc<SharedRemoteState>>>;

static HTTPS_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static TEST_HTTP_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static SHARED_REMOTE_STATES: OnceLock<SharedRemoteStates> = OnceLock::new();

#[derive(Debug, Default)]
struct SharedRemoteState {
    capability: AtomicU8,
    cache: Mutex<Option<CachedRecord>>,
}

#[derive(Debug, Clone)]
struct CachedRecord {
    path: &'static str,
    etag: Option<String>,
    record: RemoteStateRecord,
}

#[derive(Clone)]
pub struct RemoteRepository {
    base_url: String,
    client: Client,
    authenticator: RequestAuthenticator,
    cipher: SessionCipher,
    shared: Arc<SharedRemoteState>,
}

impl fmt::Debug for RemoteRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteRepository")
            .field("base_url", &self.base_url)
            .field("authenticator", &"[REDACTED]")
            .field("cipher", &"[REDACTED]")
            .finish()
    }
}

impl RemoteRepository {
    pub fn from_config(config: &RuntimeConfig) -> LumoResult<Self> {
        let url = config
            .api_url
            .as_deref()
            .ok_or_else(|| LumoError::Configuration("LUMO_API_URL is required".to_owned()))?;
        let password = config
            .api_password()
            .ok_or_else(|| LumoError::Configuration("LUMO_API_PASSWORD is required".to_owned()))?;
        Self::new(url, password, false)
    }

    pub fn new(base_url: &str, password: &str, allow_insecure_http: bool) -> LumoResult<Self> {
        let base_url = validate_base_url(base_url, allow_insecure_http)?;
        let state_key = derive_state_key(password)?;
        Ok(Self {
            client: shared_client(allow_insecure_http)?,
            authenticator: RequestAuthenticator::new(password.to_owned())?,
            cipher: SessionCipher::from_key(state_key),
            shared: shared_remote_state(&base_url, &state_key),
            base_url,
        })
    }

    fn fetch_record(&self) -> LumoResult<Option<RemoteStateRecord>> {
        match self.shared.capability.load(Ordering::Acquire) {
            CAPABILITY_LEGACY => self.fetch_version(STATE_PATH, false),
            CAPABILITY_UNKNOWN | CAPABILITY_COMPACT => {
                match self.fetch_version(COMPACT_STATE_PATH, true) {
                    Err(LumoError::NotFound(_)) => {
                        self.shared
                            .capability
                            .store(CAPABILITY_LEGACY, Ordering::Release);
                        self.fetch_version(STATE_PATH, false)
                    }
                    result => result,
                }
            }
            _ => Err(LumoError::Storage(
                "invalid remote API capability state".to_owned(),
            )),
        }
    }

    fn fetch_version(
        &self,
        path: &'static str,
        compact: bool,
    ) -> LumoResult<Option<RemoteStateRecord>> {
        let conditional_etag = self.cached_record(path).and_then(|cached| cached.etag);
        let response = self.send(Method::GET, path, Vec::new(), conditional_etag.as_deref())?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(LumoError::NotFound(path.to_owned()));
        }
        if response.status() == StatusCode::NO_CONTENT {
            self.clear_cache();
            self.set_capability(compact);
            return Ok(None);
        }
        if response.status() == StatusCode::NOT_MODIFIED {
            self.set_capability(compact);
            return self
                .cached_record(path)
                .map(|cached| Some(cached.record))
                .ok_or_else(|| {
                    LumoError::Storage(
                        "remote API returned 304 without a matching cached state".to_owned(),
                    )
                });
        }

        let response = parse_success(response)?;
        let etag = response_etag(&response);
        let record = if compact {
            let wire = response
                .json::<CompactRemoteStateRecord>()
                .map_err(response_decode_error)?;
            RemoteStateRecord::try_from(wire)?
        } else {
            let record = response
                .json::<RemoteStateRecord>()
                .map_err(response_decode_error)?;
            record.validate()?;
            record
        };
        self.set_capability(compact);
        self.cache_record(path, etag, record.clone());
        Ok(Some(record))
    }

    fn put_record(&self, request: &PutStateRequest) -> LumoResult<()> {
        let compact = self.shared.capability.load(Ordering::Acquire) != CAPABILITY_LEGACY;
        let path = if compact {
            COMPACT_STATE_PATH
        } else {
            STATE_PATH
        };
        let body = if compact {
            serde_json::to_vec(&CompactPutStateRequest::from(request))
        } else {
            serde_json::to_vec(request)
        }
        .map_err(|error| LumoError::Serialization(error.to_string()))?;

        match self.send(Method::PUT, path, body, None) {
            Ok(response) if response.status() == StatusCode::CONFLICT => {
                self.confirm_committed(request, LumoError::RevisionConflict)
            }
            Ok(response) => {
                let response = parse_success(response)?;
                self.cache_record(path, response_etag(&response), request.record.clone());
                Ok(())
            }
            Err(error) => self.confirm_committed(request, error),
        }
    }

    fn confirm_committed(&self, request: &PutStateRequest, original: LumoError) -> LumoResult<()> {
        match self.fetch_record() {
            Ok(Some(current)) if current == request.record => Ok(()),
            _ => Err(original),
        }
    }

    fn send(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        conditional_etag: Option<&str>,
    ) -> LumoResult<Response> {
        for attempt in 0..MAX_TRANSPORT_ATTEMPTS {
            match self.send_once(method.clone(), path, body.clone(), conditional_etag) {
                Ok(response)
                    if is_transient_status(response.status())
                        && attempt + 1 < MAX_TRANSPORT_ATTEMPTS =>
                {
                    retry_delay(attempt);
                }
                Ok(response) => return Ok(response),
                Err(error)
                    if is_transient_transport_error(&error)
                        && attempt + 1 < MAX_TRANSPORT_ATTEMPTS =>
                {
                    retry_delay(attempt);
                }
                Err(error) => return Err(transport_error(error)),
            }
        }
        Err(LumoError::RemoteUnavailable)
    }

    fn send_once(
        &self,
        method: Method,
        path: &str,
        body: Vec<u8>,
        conditional_etag: Option<&str>,
    ) -> Result<Response, reqwest::Error> {
        let now_ms = system_now_ms();
        let signed = self
            .authenticator
            .sign(method.as_str(), path, &body, now_ms);
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header(ACCEPT, "application/json")
            .header(TIMESTAMP_HEADER, signed.timestamp_ms)
            .header(NONCE_HEADER, signed.nonce)
            .header(SIGNATURE_HEADER, signed.signature);
        if let Some(etag) = conditional_etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if !body.is_empty() {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        request.send()
    }

    fn decode(&self, record: RemoteStateRecord) -> LumoResult<RuntimeState> {
        record.validate()?;
        self.cipher.open(
            &record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )
    }

    fn encode(&self, state: &RuntimeState) -> LumoResult<RemoteStateRecord> {
        let now_ms = system_now_ms();
        Ok(RemoteStateRecord {
            revision: state.revision,
            envelope: self
                .cipher
                .seal(state, now_ms, i64::MAX.saturating_sub(now_ms))?,
        })
    }

    fn cached_record(&self, path: &'static str) -> Option<CachedRecord> {
        lock_cache(&self.shared.cache)
            .as_ref()
            .filter(|cached| cached.path == path)
            .cloned()
    }

    fn cache_record(&self, path: &'static str, etag: Option<String>, record: RemoteStateRecord) {
        *lock_cache(&self.shared.cache) = Some(CachedRecord { path, etag, record });
    }

    fn clear_cache(&self) {
        *lock_cache(&self.shared.cache) = None;
    }

    fn set_capability(&self, compact: bool) {
        self.shared.capability.store(
            if compact {
                CAPABILITY_COMPACT
            } else {
                CAPABILITY_LEGACY
            },
            Ordering::Release,
        );
    }
}

impl StateRepository for RemoteRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        self.fetch_record()?
            .map(|record| self.decode(record))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>,
    {
        let current = self.fetch_record()?;
        let expected_revision = current.as_ref().map(|record| record.revision);
        let mut state = current
            .map(|record| self.decode(record))
            .transpose()?
            .unwrap_or_default();
        let original = state.clone();
        let outcome = operation(&mut state);
        if state != original {
            self.put_record(&PutStateRequest {
                expected_revision,
                record: self.encode(&state)?,
            })?;
        }
        outcome
    }
}

fn shared_client(allow_insecure_http: bool) -> LumoResult<Client> {
    let cell = if allow_insecure_http {
        &TEST_HTTP_CLIENT
    } else {
        &HTTPS_CLIENT
    };
    match cell.get_or_init(|| build_client(allow_insecure_http).map_err(|error| error.to_string()))
    {
        Ok(client) => Ok(client.clone()),
        Err(error) => Err(LumoError::Configuration(format!(
            "unable to initialize HTTP client: {error}"
        ))),
    }
}

fn build_client(allow_insecure_http: bool) -> Result<Client, reqwest::Error> {
    Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .pool_idle_timeout(Duration::from_secs(45))
        .pool_max_idle_per_host(2)
        .tcp_keepalive(Duration::from_secs(30))
        .tcp_nodelay(true)
        .min_tls_version(Version::TLS_1_2)
        .https_only(!allow_insecure_http)
        .redirect(Policy::none())
        .user_agent(concat!("Lumo/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn shared_remote_state(base_url: &str, state_key: &[u8; 32]) -> Arc<SharedRemoteState> {
    let states = SHARED_REMOTE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut states = states
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let key_fingerprint = u64::from_le_bytes(
        state_key[..8]
            .try_into()
            .expect("the state key always contains eight prefix bytes"),
    );
    states
        .entry((base_url.to_owned(), key_fingerprint))
        .or_insert_with(|| Arc::new(SharedRemoteState::default()))
        .clone()
}

fn lock_cache(cache: &Mutex<Option<CachedRecord>>) -> MutexGuard<'_, Option<CachedRecord>> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn validate_base_url(base_url: &str, allow_insecure_http: bool) -> LumoResult<String> {
    let url = Url::parse(base_url.trim())
        .map_err(|error| LumoError::Configuration(format!("LUMO_API_URL is invalid: {error}")))?;
    let secure = url.scheme() == "https";
    if !secure && !(allow_insecure_http && url.scheme() == "http") {
        return Err(LumoError::Configuration(
            "remote API requires HTTPS".to_owned(),
        ));
    }
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(LumoError::Configuration(
            "LUMO_API_URL must be an origin URL without credentials, path, query, or fragment"
                .to_owned(),
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn parse_success(response: Response) -> LumoResult<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.json::<ApiErrorBody>().ok();
    let code = body.as_ref().map(|body| body.code.as_str());
    Err(match status {
        StatusCode::UNAUTHORIZED if code == Some("clock_skew") => LumoError::ExpiredMessage,
        StatusCode::UNAUTHORIZED => LumoError::AuthenticationFailed,
        StatusCode::CONFLICT if code == Some("replay_detected") => LumoError::ReplayDetected,
        StatusCode::CONFLICT => LumoError::RevisionConflict,
        StatusCode::TOO_MANY_REQUESTS => LumoError::RateLimited,
        StatusCode::PAYLOAD_TOO_LARGE => {
            LumoError::InvalidInput("remote state exceeds the API size limit".to_owned())
        }
        status if status.is_server_error() || is_transient_status(status) => {
            LumoError::RemoteUnavailable
        }
        _ => LumoError::Storage(
            body.map(|body| body.message)
                .unwrap_or_else(|| format!("remote API returned {status}")),
        ),
    })
}

fn response_etag(response: &Response) -> Option<String> {
    response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn response_decode_error(error: reqwest::Error) -> LumoError {
    LumoError::Serialization(format!("invalid remote API response: {error}"))
}

fn is_transient_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
}

fn is_transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn retry_delay(attempt: usize) {
    let delay_ms = if attempt == 0 { 100 } else { 250 };
    thread::sleep(Duration::from_millis(delay_ms));
}

fn transport_error(error: reqwest::Error) -> LumoError {
    if is_transient_transport_error(&error) {
        LumoError::RemoteUnavailable
    } else {
        LumoError::Storage("remote API transport failed".to_owned())
    }
}

fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::get,
        Json, Router,
    };
    use lumo_core::ports::StateRepository;
    use tokio::net::TcpListener;

    use super::*;

    const PASSWORD: &str = "remote-client-test-password-with-entropy";
    const TEST_ETAG: &str = "\"lumo-compact-1\"";

    #[test]
    fn shared_caches_are_isolated_by_endpoint_and_credential() {
        let first = shared_remote_state("https://example.test", &[1; 32]);
        let same = shared_remote_state("https://example.test", &[1; 32]);
        let rotated = shared_remote_state("https://example.test", &[2; 32]);
        let other_endpoint = shared_remote_state("https://other.test", &[1; 32]);

        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &rotated));
        assert!(!Arc::ptr_eq(&first, &other_endpoint));
    }

    #[derive(Clone)]
    struct RecordServer {
        calls: Arc<AtomicUsize>,
        record: CompactRemoteStateRecord,
    }

    #[derive(Clone, Default)]
    struct NegotiationCalls {
        compact: Arc<AtomicUsize>,
        legacy: Arc<AtomicUsize>,
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn conditional_get_reuses_verified_cached_state_after_304() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let (repository, expected, encoded) = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new(&base_url, PASSWORD, true).expect("repository");
            let expected = RuntimeState {
                revision: 1,
                ..RuntimeState::default()
            };
            let encoded = repository.encode(&expected).expect("encoded state");
            (repository, expected, encoded)
        })
        .await
        .expect("repository task");
        let server_state = RecordServer {
            calls: Arc::new(AtomicUsize::new(0)),
            record: CompactRemoteStateRecord::from(&encoded),
        };
        let calls = server_state.calls.clone();
        let app = Router::new()
            .route(COMPACT_STATE_PATH, get(conditional_state))
            .with_state(server_state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let loaded = tokio::task::spawn_blocking(move || {
            let first = repository.load().expect("first load");
            let second = repository.load().expect("cached load");
            (first, second)
        })
        .await
        .expect("client task");
        assert_eq!(loaded.0, expected);
        assert_eq!(loaded.1, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn transient_server_failures_are_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(COMPACT_STATE_PATH, get(transient_state))
            .with_state(calls.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let repository = tokio::task::spawn_blocking(move || {
            RemoteRepository::new(&base_url, PASSWORD, true).expect("repository")
        })
        .await
        .expect("repository task");

        let loaded = tokio::task::spawn_blocking(move || repository.load())
            .await
            .expect("client task")
            .expect("state");
        assert_eq!(loaded, RuntimeState::default());
        assert_eq!(calls.load(Ordering::SeqCst), MAX_TRANSPORT_ATTEMPTS);
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn compact_endpoint_negotiation_falls_back_to_legacy_v1() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let calls = NegotiationCalls::default();
        let observed = calls.clone();
        let app = Router::new()
            .route(COMPACT_STATE_PATH, get(compact_not_found))
            .route(STATE_PATH, get(legacy_empty))
            .with_state(calls);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let repository = tokio::task::spawn_blocking(move || {
            RemoteRepository::new(&base_url, PASSWORD, true).expect("repository")
        })
        .await
        .expect("repository task");

        tokio::task::spawn_blocking(move || {
            repository.load().expect("negotiated load");
            repository.load().expect("legacy load");
        })
        .await
        .expect("client task");
        assert_eq!(observed.compact.load(Ordering::SeqCst), 1);
        assert_eq!(observed.legacy.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ambiguous_put_is_confirmed_without_duplicating_the_mutation() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let (repository, record) = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new(&base_url, PASSWORD, true).expect("repository");
            let state = RuntimeState {
                revision: 1,
                ..RuntimeState::default()
            };
            let record = repository.encode(&state).expect("record");
            (repository, record)
        })
        .await
        .expect("repository task");
        let server_state = RecordServer {
            calls: Arc::new(AtomicUsize::new(0)),
            record: CompactRemoteStateRecord::from(&record),
        };
        let calls = server_state.calls.clone();
        let app = Router::new()
            .route(COMPACT_STATE_PATH, get(current_state).put(conflicting_put))
            .with_state(server_state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let request = PutStateRequest {
            expected_revision: None,
            record,
        };

        tokio::task::spawn_blocking(move || repository.put_record(&request))
            .await
            .expect("client task")
            .expect("confirmed write");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.abort();
    }

    async fn conditional_state(State(state): State<RecordServer>, headers: HeaderMap) -> Response {
        let call = state.calls.fetch_add(1, Ordering::SeqCst);
        if call > 0 {
            assert_eq!(
                headers
                    .get(IF_NONE_MATCH)
                    .and_then(|value| value.to_str().ok()),
                Some(TEST_ETAG)
            );
            return (StatusCode::NOT_MODIFIED, [(ETAG, TEST_ETAG)]).into_response();
        }
        ([(ETAG, TEST_ETAG)], Json(state.record)).into_response()
    }

    async fn transient_state(State(calls): State<Arc<AtomicUsize>>) -> StatusCode {
        let call = calls.fetch_add(1, Ordering::SeqCst);
        if call + 1 < MAX_TRANSPORT_ATTEMPTS {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::NO_CONTENT
        }
    }

    async fn current_state(State(state): State<RecordServer>) -> impl IntoResponse {
        state.calls.fetch_add(1, Ordering::SeqCst);
        ([(ETAG, TEST_ETAG)], Json(state.record))
    }

    async fn conflicting_put() -> StatusCode {
        StatusCode::CONFLICT
    }

    async fn compact_not_found(State(calls): State<NegotiationCalls>) -> StatusCode {
        calls.compact.fetch_add(1, Ordering::SeqCst);
        StatusCode::NOT_FOUND
    }

    async fn legacy_empty(State(calls): State<NegotiationCalls>) -> StatusCode {
        calls.legacy.fetch_add(1, Ordering::SeqCst);
        StatusCode::NO_CONTENT
    }
}
