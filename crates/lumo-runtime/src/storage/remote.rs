use std::{fmt, time::Duration};

use lumo_core::{
    domain::RuntimeState,
    ports::StateRepository,
    security::{ReplayGuard, SessionCipher},
    LumoError, LumoResult,
};
use lumo_protocol::{
    derive_state_key, ApiErrorBody, PutStateRequest, RemoteStateRecord, RequestAuthenticator,
    STATE_PATH,
};
use reqwest::{
    blocking::{Client, Response},
    header::CONTENT_TYPE,
    Method, StatusCode,
};

use crate::config::RuntimeConfig;

const TIMESTAMP_HEADER: &str = "x-lumo-timestamp";
const NONCE_HEADER: &str = "x-lumo-nonce";
const SIGNATURE_HEADER: &str = "x-lumo-signature";

#[derive(Clone)]
pub struct RemoteRepository {
    base_url: String,
    client: Client,
    authenticator: RequestAuthenticator,
    cipher: SessionCipher,
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
        let base_url = base_url.trim_end_matches('/').to_owned();
        if !base_url.starts_with("https://")
            && !(allow_insecure_http && base_url.starts_with("http://"))
        {
            return Err(LumoError::Configuration(
                "remote API requires HTTPS".to_owned(),
            ));
        }
        Ok(Self {
            base_url,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .https_only(!allow_insecure_http)
                .build()
                .map_err(remote_error)?,
            authenticator: RequestAuthenticator::new(password.to_owned())?,
            cipher: SessionCipher::from_key(derive_state_key(password)?),
        })
    }

    fn fetch_record(&self) -> LumoResult<Option<RemoteStateRecord>> {
        let response = self.send(Method::GET, STATE_PATH, Vec::new())?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        parse_success(response).and_then(|response| response.json().map_err(remote_error))
    }

    fn put_record(&self, request: &PutStateRequest) -> LumoResult<()> {
        let body = serde_json::to_vec(request)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        let response = self.send(Method::PUT, STATE_PATH, body)?;
        if response.status() == StatusCode::CONFLICT {
            return Err(LumoError::RevisionConflict);
        }
        parse_success(response).map(|_| ())
    }

    fn send(&self, method: Method, path: &str, body: Vec<u8>) -> LumoResult<Response> {
        let now_ms = system_now_ms();
        let signed = self
            .authenticator
            .sign(method.as_str(), path, &body, now_ms);
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .header(TIMESTAMP_HEADER, signed.timestamp_ms)
            .header(NONCE_HEADER, signed.nonce)
            .header(SIGNATURE_HEADER, signed.signature);
        if !body.is_empty() {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }
        request.send().map_err(remote_error)
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

fn parse_success(response: Response) -> LumoResult<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.json::<ApiErrorBody>().ok();
    Err(match status {
        StatusCode::UNAUTHORIZED => LumoError::AuthenticationFailed,
        StatusCode::CONFLICT => LumoError::RevisionConflict,
        _ => LumoError::Storage(
            body.map(|body| body.message)
                .unwrap_or_else(|| format!("remote API returned {status}")),
        ),
    })
}

fn remote_error(error: impl fmt::Display) -> LumoError {
    LumoError::Storage(format!("remote API error: {error}"))
}

fn system_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
