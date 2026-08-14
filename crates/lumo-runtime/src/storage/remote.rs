use std::{
    collections::HashMap,
    fmt, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    thread,
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_core::{
    domain::{AppSnapshot, RuntimeProfile, RuntimeState},
    ports::StateRepository,
    security::{ReplayGuard, SealedPayload, SessionCipher},
    LumoError, LumoResult,
};
use lumo_protocol::{
    group_device_path, group_devices_path, group_invitations_path, group_leave_path,
    group_member_operations_path, group_member_path, group_path, group_state_path,
    group_verify_pin_path, invitation_consume_path, ApiErrorBody, CompactPutStateRequest,
    CompactRemoteStateRecord, CompactSealedPayload, ConsumeInvitationRequest, ControlledOperation,
    ControlledOperationRequest, ControlledOperationResponse, CreateGroupRequest,
    CreateInvitationRequest, DeviceCredentialResponse, DeviceListResponse, DeviceRole,
    DeviceSummary, InvitationResponse, MemberOperationEnvelopeRequest, ProtectedActionRequest,
    PutStateRequest, RemoteStateRecord, DEVICE_ID_HEADER, GROUPS_PATH, NONCE_HEADER,
    TIMESTAMP_HEADER,
};
use rand::{rngs::OsRng, RngCore};
use reqwest::{
    blocking::{Client, Response},
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
    redirect::Policy,
    tls::Version,
    Method, StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    config::RuntimeConfig,
    credentials::{normalize_api_origin, CredentialSlot, DeviceCredential},
    storage::ControlledOperationPort,
};

const MAX_TRANSPORT_ATTEMPTS: usize = 3;
const CACHE_VERSION: u8 = 2;
const CACHE_FILE_NAME: &str = "remote-state-cache.json";
const PENDING_OPERATION_VERSION: u8 = 1;
const PENDING_OPERATION_FILE_NAME: &str = "pending-controlled-operation.json";
const MEMBER_OPERATION_TTL_MS: i64 = 5 * 60 * 1_000;

type RemoteStateKey = (String, String, u64);
type SharedRemoteStates = Mutex<HashMap<RemoteStateKey, Arc<SharedRemoteState>>>;

static HTTPS_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static TEST_HTTP_CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
static SHARED_REMOTE_STATES: OnceLock<SharedRemoteStates> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFreshness {
    Fresh,
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteLoad {
    pub state: RuntimeState,
    pub freshness: RemoteFreshness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteMemberLoad {
    pub snapshot: AppSnapshot,
    pub freshness: RemoteFreshness,
}

#[derive(Debug, Default)]
struct SharedRemoteState {
    cache: Mutex<Option<CachedRecord>>,
    controlled_operation: Mutex<()>,
}

#[derive(Debug, Clone)]
struct CachedRecord {
    etag: Option<String>,
    record: RemoteStateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteCache {
    version: u8,
    api_origin: String,
    group_id: String,
    #[serde(default)]
    role: Option<DeviceRole>,
    etag: Option<String>,
    record: CompactRemoteStateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedControlledOperation {
    version: u8,
    api_origin: String,
    group_id: String,
    device_id: String,
    request: CompactSealedPayload,
}

struct RemoteContext {
    credential: DeviceCredential,
    cipher: SessionCipher,
    shared: Arc<SharedRemoteState>,
    state_path: String,
}

#[derive(Clone)]
pub struct RemoteRepository {
    base_url: String,
    client: Client,
    credentials: CredentialSlot,
    cache_path: Option<Arc<PathBuf>>,
    pending_operation_path: Option<Arc<PathBuf>>,
}

impl fmt::Debug for RemoteRepository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteRepository")
            .field("base_url", &self.base_url)
            .field("credentials", &self.credentials)
            .field("cache_path", &self.cache_path)
            .field("pending_operation_path", &self.pending_operation_path)
            .finish()
    }
}

impl RemoteRepository {
    pub fn from_config(config: &RuntimeConfig) -> LumoResult<Self> {
        let url = config
            .api_url
            .as_deref()
            .ok_or_else(|| LumoError::Configuration("LUMO_API_URL is required".to_owned()))?;
        Self::new_with_cache(url, None, false, Some(&config.data_dir))
    }

    pub fn new(
        base_url: &str,
        credential: Option<DeviceCredential>,
        allow_insecure_http: bool,
    ) -> LumoResult<Self> {
        Self::new_with_cache(base_url, credential, allow_insecure_http, None)
    }

    pub fn new_with_cache(
        base_url: &str,
        credential: Option<DeviceCredential>,
        allow_insecure_http: bool,
        data_dir: Option<&Path>,
    ) -> LumoResult<Self> {
        let base_url = normalize_api_origin(base_url, allow_insecure_http)?;
        if credential
            .as_ref()
            .is_some_and(|value| value.api_origin() != base_url)
        {
            return Err(LumoError::AuthenticationFailed);
        }
        Ok(Self {
            client: shared_client(allow_insecure_http)?,
            credentials: CredentialSlot::new(credential),
            cache_path: data_dir.map(|directory| Arc::new(directory.join(CACHE_FILE_NAME))),
            pending_operation_path: data_dir
                .map(|directory| Arc::new(directory.join(PENDING_OPERATION_FILE_NAME))),
            base_url,
        })
    }

    pub fn credential(&self) -> LumoResult<Option<DeviceCredential>> {
        self.credentials.get()
    }

    pub fn install_credential(&self, credential: DeviceCredential) -> LumoResult<()> {
        if credential.api_origin() != self.base_url {
            return Err(LumoError::AuthenticationFailed);
        }
        if self.credentials.get()?.as_ref().is_some_and(|current| {
            current.group_id() != credential.group_id()
                || current.device_id() != credential.device_id()
        }) {
            self.clear_cached_state()?;
        }
        self.credentials.install(credential)
    }

    pub fn clear_credential(&self) -> LumoResult<()> {
        self.clear_cached_state()?;
        self.credentials.clear()
    }

    pub fn clear_cached_state(&self) -> LumoResult<()> {
        if let Some(credential) = self.credentials.get()? {
            *lock_cache(&shared_remote_state(&credential).cache) = None;
        }
        if let Some(path) = self.cache_path.as_deref() {
            match fs::remove_file(path.as_path()) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(storage_error(error)),
            }
        }
        self.clear_pending_controlled_operation()?;
        Ok(())
    }

    pub fn provision_group(
        &self,
        request_id: &str,
        pin: &str,
        device_name: &str,
    ) -> LumoResult<DeviceCredential> {
        validate_identifier("request", request_id)?;
        let request = CreateGroupRequest {
            request_id: request_id.to_owned(),
            pin: pin.to_owned(),
            device_name: device_name.to_owned(),
        };
        let response: DeviceCredentialResponse =
            self.public_json(Method::POST, GROUPS_PATH, &request, true)?;
        let credential = self.credential_from_response(response, DeviceRole::Controller)?;
        self.install_credential(credential.clone())?;
        Ok(credential)
    }

    pub fn consume_invitation(
        &self,
        request_id: &str,
        invitation_id: &str,
        token: &str,
        pin: &str,
        device_name: &str,
    ) -> LumoResult<DeviceCredential> {
        validate_identifier("request", request_id)?;
        validate_identifier("invitation", invitation_id)?;
        let request = ConsumeInvitationRequest {
            request_id: request_id.to_owned(),
            token: token.to_owned(),
            pin: pin.to_owned(),
            device_name: device_name.to_owned(),
        };
        let response: DeviceCredentialResponse = self.public_json(
            Method::POST,
            &invitation_consume_path(invitation_id),
            &request,
            true,
        )?;
        let credential = self.credential_from_response(response, DeviceRole::Controlled)?;
        self.install_credential(credential.clone())?;
        Ok(credential)
    }

    pub fn create_invitation(&self, pin: &str) -> LumoResult<InvitationResponse> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        self.authenticated_json(
            &context,
            Method::POST,
            &group_invitations_path(context.credential.group_id()),
            &CreateInvitationRequest {
                pin: pin.to_owned(),
            },
            false,
        )
    }

    pub fn verify_pin(&self, pin: &str) -> LumoResult<()> {
        let context = self.context()?;
        self.protected_action(
            &context,
            Method::POST,
            &group_verify_pin_path(context.credential.group_id()),
            pin,
        )
    }

    pub fn list_devices(&self) -> LumoResult<Vec<DeviceSummary>> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        let response: DeviceListResponse = self.authenticated_json_without_body(
            &context,
            Method::GET,
            &group_devices_path(context.credential.group_id()),
            true,
        )?;
        Ok(response.devices)
    }

    pub fn revoke_device(&self, device_id: &str, pin: &str) -> LumoResult<()> {
        validate_identifier("device", device_id)?;
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        self.protected_action(
            &context,
            Method::DELETE,
            &group_device_path(context.credential.group_id(), device_id),
            pin,
        )
    }

    pub fn leave_group(&self, pin: &str) -> LumoResult<()> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controlled)?;
        self.protected_action(
            &context,
            Method::POST,
            &group_leave_path(context.credential.group_id()),
            pin,
        )?;
        self.clear_credential()
    }

    pub fn delete_group(&self, pin: &str) -> LumoResult<()> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        self.protected_action(
            &context,
            Method::DELETE,
            &group_path(context.credential.group_id()),
            pin,
        )?;
        self.clear_credential()
    }

    pub fn load_with_freshness(&self) -> LumoResult<RemoteLoad> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        match self.fetch_record_network(&context) {
            Ok(record) => Ok(RemoteLoad {
                state: record
                    .map(|record| self.decode(&context, &record))
                    .transpose()?
                    .unwrap_or_default(),
                freshness: RemoteFreshness::Fresh,
            }),
            Err(LumoError::RemoteUnavailable) => {
                let cached = self
                    .read_persisted_cache(&context)?
                    .ok_or(LumoError::RemoteUnavailable)?;
                Ok(RemoteLoad {
                    state: self.decode(&context, &cached.record)?,
                    freshness: RemoteFreshness::Stale,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn load_member_with_freshness(&self) -> LumoResult<RemoteMemberLoad> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controlled)?;
        match self.fetch_record_network(&context) {
            Ok(Some(record)) => Ok(RemoteMemberLoad {
                snapshot: self.decode_member(&context, &record)?,
                freshness: RemoteFreshness::Fresh,
            }),
            Ok(None) => Err(LumoError::GroupNotInitialized),
            Err(LumoError::RemoteUnavailable) => {
                let cached = self
                    .read_persisted_cache(&context)?
                    .ok_or(LumoError::RemoteUnavailable)?;
                Ok(RemoteMemberLoad {
                    snapshot: self.decode_member(&context, &cached.record)?,
                    freshness: RemoteFreshness::Stale,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn execute_controlled_operation(
        &self,
        operation: ControlledOperation,
    ) -> LumoResult<ControlledOperationResponse> {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controlled)?;
        let _guard = context
            .shared
            .controlled_operation
            .lock()
            .map_err(|_| LumoError::Storage("controlled operation lock poisoned".to_owned()))?;

        if let Some(pending) = self.read_pending_controlled_operation(&context)? {
            match self.send_controlled_operation(&context, &pending) {
                Ok(response) => {
                    self.clear_pending_controlled_operation()?;
                    if pending.operation == operation {
                        return Ok(response);
                    }
                }
                Err(error) => {
                    if !controlled_operation_outcome_is_ambiguous(&error) {
                        self.clear_pending_controlled_operation()?;
                    }
                    return Err(error);
                }
            }
        }

        let request = ControlledOperationRequest {
            operation_id: Uuid::new_v4().to_string(),
            operation,
        };
        self.write_pending_controlled_operation(&context, &request)?;
        match self.send_controlled_operation(&context, &request) {
            Ok(response) => {
                self.clear_pending_controlled_operation()?;
                Ok(response)
            }
            Err(error) => {
                if !controlled_operation_outcome_is_ambiguous(&error) {
                    self.clear_pending_controlled_operation()?;
                }
                Err(error)
            }
        }
    }

    fn send_controlled_operation(
        &self,
        context: &RemoteContext,
        request: &ControlledOperationRequest,
    ) -> LumoResult<ControlledOperationResponse> {
        let now_ms = system_now_ms();
        let sealed = context
            .cipher
            .seal(request, now_ms, MEMBER_OPERATION_TTL_MS)?;
        let body = MemberOperationEnvelopeRequest {
            envelope: CompactSealedPayload::from(&sealed),
        };
        let response = self.authenticated_response(
            context,
            Method::POST,
            &group_member_operations_path(context.credential.group_id()),
            &body,
            true,
        )?;
        let etag = response_etag(&response);
        let compact = response
            .json::<CompactRemoteStateRecord>()
            .map_err(response_decode_error)?;
        let record = RemoteStateRecord::try_from(compact)?;
        let result: ControlledOperationResponse = context.cipher.open(
            &record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )?;
        self.validate_member_snapshot(context, &result.snapshot, record.revision)?;
        self.cache_member_snapshot(context, etag, &result.snapshot)?;
        Ok(result)
    }

    fn credential_from_response(
        &self,
        response: DeviceCredentialResponse,
        expected_role: DeviceRole,
    ) -> LumoResult<DeviceCredential> {
        if response.role != expected_role {
            return Err(LumoError::AuthenticationFailed);
        }
        DeviceCredential::from_parts(
            &self.base_url,
            response.group_id,
            response.device_id,
            response.role,
            response.device_token,
            response.state_key,
            self.base_url.starts_with("http://"),
        )
    }

    fn context(&self) -> LumoResult<RemoteContext> {
        let credential = self.credentials.require()?;
        if credential.api_origin() != self.base_url {
            return Err(LumoError::AuthenticationFailed);
        }
        Ok(RemoteContext {
            cipher: credential.cipher()?,
            shared: shared_remote_state(&credential),
            state_path: match credential.role() {
                DeviceRole::Controller => group_state_path(credential.group_id()),
                DeviceRole::Controlled => group_member_path(credential.group_id()),
            },
            credential,
        })
    }

    fn fetch_record_network(
        &self,
        context: &RemoteContext,
    ) -> LumoResult<Option<RemoteStateRecord>> {
        if lock_cache(&context.shared.cache).is_none() {
            if let Ok(Some(cached)) = self.read_persisted_cache(context) {
                *lock_cache(&context.shared.cache) = Some(cached);
            }
        }
        let conditional_etag = lock_cache(&context.shared.cache)
            .as_ref()
            .and_then(|cached| cached.etag.clone());
        let response = self.send_authenticated(
            context,
            Method::GET,
            &context.state_path,
            &[],
            conditional_etag.as_deref(),
            true,
        )?;

        if response.status() == StatusCode::NO_CONTENT {
            self.clear_context_cache(context)?;
            return Ok(None);
        }
        if response.status() == StatusCode::NOT_MODIFIED {
            let cached = lock_cache(&context.shared.cache).clone().ok_or_else(|| {
                LumoError::Storage("remote API returned 304 without a verified cache".to_owned())
            })?;
            self.verify_record(context, &cached.record)?;
            return Ok(Some(cached.record));
        }
        let response = parse_success(response)?;
        let etag = response_etag(&response);
        let compact = response
            .json::<CompactRemoteStateRecord>()
            .map_err(response_decode_error)?;
        let record = RemoteStateRecord::try_from(compact)?;
        self.verify_record(context, &record)?;
        self.cache_record(context, etag, record.clone())?;
        Ok(Some(record))
    }

    fn put_record(&self, context: &RemoteContext, request: &PutStateRequest) -> LumoResult<()> {
        let body = Zeroizing::new(
            serde_json::to_vec(&CompactPutStateRequest::from(request))
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        match self.send_authenticated(
            context,
            Method::PUT,
            &context.state_path,
            body.as_slice(),
            None,
            false,
        ) {
            Ok(response) if response.status() == StatusCode::CONFLICT => {
                self.confirm_committed(context, request, LumoError::RevisionConflict)
            }
            Ok(response) if is_transient_status(response.status()) => {
                self.confirm_committed(context, request, LumoError::RemoteUnavailable)
            }
            Ok(response) => {
                let response = parse_success(response)?;
                self.cache_record(context, response_etag(&response), request.record.clone())
            }
            Err(error) => self.confirm_committed(context, request, error),
        }
    }

    fn confirm_committed(
        &self,
        context: &RemoteContext,
        request: &PutStateRequest,
        original: LumoError,
    ) -> LumoResult<()> {
        match self.fetch_record_network(context) {
            Ok(Some(current)) if current == request.record => Ok(()),
            _ => Err(original),
        }
    }

    fn protected_action(
        &self,
        context: &RemoteContext,
        method: Method,
        path: &str,
        pin: &str,
    ) -> LumoResult<()> {
        let body = Zeroizing::new(
            serde_json::to_vec(&ProtectedActionRequest {
                pin: pin.to_owned(),
            })
            .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let response =
            self.send_authenticated(context, method, path, body.as_slice(), None, false)?;
        parse_success(response).map(|_| ())
    }

    fn public_json<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: &str,
        value: &B,
        retry: bool,
    ) -> LumoResult<T> {
        let body = Zeroizing::new(
            serde_json::to_vec(value)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let response = self.send_public(method, path, body.as_slice(), retry)?;
        parse_success(response)?
            .json::<T>()
            .map_err(response_decode_error)
    }

    fn authenticated_json<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        context: &RemoteContext,
        method: Method,
        path: &str,
        value: &B,
        retry: bool,
    ) -> LumoResult<T> {
        let body = Zeroizing::new(
            serde_json::to_vec(value)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let response =
            self.send_authenticated(context, method, path, body.as_slice(), None, retry)?;
        parse_success(response)?
            .json::<T>()
            .map_err(response_decode_error)
    }

    fn authenticated_response<B: Serialize>(
        &self,
        context: &RemoteContext,
        method: Method,
        path: &str,
        value: &B,
        retry: bool,
    ) -> LumoResult<Response> {
        let body = Zeroizing::new(
            serde_json::to_vec(value)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let response =
            self.send_authenticated(context, method, path, body.as_slice(), None, retry)?;
        parse_success(response)
    }

    fn authenticated_json_without_body<T: serde::de::DeserializeOwned>(
        &self,
        context: &RemoteContext,
        method: Method,
        path: &str,
        retry: bool,
    ) -> LumoResult<T> {
        let response = self.send_authenticated(context, method, path, &[], None, retry)?;
        parse_success(response)?
            .json::<T>()
            .map_err(response_decode_error)
    }

    fn send_public(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
        retry: bool,
    ) -> LumoResult<Response> {
        self.send_with_retry(retry, || {
            let mut request = self
                .client
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .header(ACCEPT, "application/json");
            if !body.is_empty() {
                request = request
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.to_vec());
            }
            request.send()
        })
    }

    fn send_authenticated(
        &self,
        context: &RemoteContext,
        method: Method,
        path: &str,
        body: &[u8],
        conditional_etag: Option<&str>,
        retry: bool,
    ) -> LumoResult<Response> {
        self.send_with_retry(retry, || {
            let mut request = self
                .client
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .header(ACCEPT, "application/json")
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", context.credential.device_token()),
                )
                .header(DEVICE_ID_HEADER, context.credential.device_id())
                .header(TIMESTAMP_HEADER, system_now_ms())
                .header(NONCE_HEADER, request_nonce());
            if let Some(etag) = conditional_etag {
                request = request.header(IF_NONE_MATCH, etag);
            }
            if !body.is_empty() {
                request = request
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.to_vec());
            }
            request.send()
        })
    }

    fn send_with_retry<F>(&self, retry: bool, mut send: F) -> LumoResult<Response>
    where
        F: FnMut() -> Result<Response, reqwest::Error>,
    {
        let attempts = if retry { MAX_TRANSPORT_ATTEMPTS } else { 1 };
        for attempt in 0..attempts {
            match send() {
                Ok(response)
                    if is_transient_status(response.status()) && attempt + 1 < attempts =>
                {
                    retry_delay(attempt);
                }
                Ok(response) => return Ok(response),
                Err(error) if is_transient_transport_error(&error) && attempt + 1 < attempts => {
                    retry_delay(attempt);
                }
                Err(error) => return Err(transport_error(error)),
            }
        }
        Err(LumoError::RemoteUnavailable)
    }

    fn decode(
        &self,
        context: &RemoteContext,
        record: &RemoteStateRecord,
    ) -> LumoResult<RuntimeState> {
        ensure_role(&context.credential, DeviceRole::Controller)?;
        record.validate()?;
        let state: RuntimeState = context.cipher.open(
            &record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )?;
        if state.revision != record.revision {
            return Err(LumoError::AuthenticationFailed);
        }
        Ok(state)
    }

    fn decode_member(
        &self,
        context: &RemoteContext,
        record: &RemoteStateRecord,
    ) -> LumoResult<AppSnapshot> {
        ensure_role(&context.credential, DeviceRole::Controlled)?;
        record.validate()?;
        let snapshot: AppSnapshot = context.cipher.open(
            &record.envelope,
            system_now_ms(),
            &mut ReplayGuard::default(),
        )?;
        self.validate_member_snapshot(context, &snapshot, record.revision)?;
        Ok(snapshot)
    }

    fn verify_record(&self, context: &RemoteContext, record: &RemoteStateRecord) -> LumoResult<()> {
        match context.credential.role() {
            DeviceRole::Controller => self.decode(context, record).map(|_| ()),
            DeviceRole::Controlled => self.decode_member(context, record).map(|_| ()),
        }
    }

    fn validate_member_snapshot(
        &self,
        _context: &RemoteContext,
        snapshot: &AppSnapshot,
        revision: u64,
    ) -> LumoResult<()> {
        if snapshot.revision != revision
            || snapshot.profile != RuntimeProfile::Controlled
            || !snapshot.places.is_empty()
            || !snapshot.events.is_empty()
            || !snapshot.commands.is_empty()
            || snapshot.session.is_none()
        {
            return Err(LumoError::AuthenticationFailed);
        }
        Ok(())
    }

    fn encode(
        &self,
        context: &RemoteContext,
        state: &RuntimeState,
    ) -> LumoResult<RemoteStateRecord> {
        let now_ms = system_now_ms();
        Ok(RemoteStateRecord {
            revision: state.revision,
            envelope: context
                .cipher
                .seal(state, now_ms, i64::MAX.saturating_sub(now_ms))?,
        })
    }

    fn cache_record(
        &self,
        context: &RemoteContext,
        etag: Option<String>,
        record: RemoteStateRecord,
    ) -> LumoResult<()> {
        self.verify_record(context, &record)?;
        let cached = CachedRecord { etag, record };
        self.write_persisted_cache(context, &cached)?;
        *lock_cache(&context.shared.cache) = Some(cached);
        Ok(())
    }

    fn cache_member_snapshot(
        &self,
        context: &RemoteContext,
        etag: Option<String>,
        snapshot: &AppSnapshot,
    ) -> LumoResult<()> {
        ensure_role(&context.credential, DeviceRole::Controlled)?;
        let now_ms = system_now_ms();
        let record = RemoteStateRecord {
            revision: snapshot.revision,
            envelope: context
                .cipher
                .seal(snapshot, now_ms, i64::MAX.saturating_sub(now_ms))?,
        };
        self.cache_record(context, etag, record)
    }

    fn read_persisted_cache(&self, context: &RemoteContext) -> LumoResult<Option<CachedRecord>> {
        let Some(path) = self.cache_path.as_deref() else {
            return Ok(None);
        };
        let bytes = match fs::read(path.as_path()) {
            Ok(bytes) => Zeroizing::new(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        let persisted: PersistedRemoteCache = serde_json::from_slice(bytes.as_slice())
            .map_err(|_| LumoError::Storage("invalid remote cache".to_owned()))?;
        if persisted.version != CACHE_VERSION {
            return Ok(None);
        }
        if persisted.api_origin != self.base_url
            || persisted.group_id != context.credential.group_id()
            || persisted.role != Some(context.credential.role())
        {
            return Err(LumoError::AuthenticationFailed);
        }
        let record = RemoteStateRecord::try_from(persisted.record)?;
        self.verify_record(context, &record)?;
        Ok(Some(CachedRecord {
            etag: persisted.etag,
            record,
        }))
    }

    fn write_persisted_cache(
        &self,
        context: &RemoteContext,
        cached: &CachedRecord,
    ) -> LumoResult<()> {
        let Some(path) = self.cache_path.as_deref() else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| LumoError::Storage("remote cache path has no parent".to_owned()))?;
        fs::create_dir_all(parent).map_err(storage_error)?;
        let persisted = PersistedRemoteCache {
            version: CACHE_VERSION,
            api_origin: self.base_url.clone(),
            group_id: context.credential.group_id().to_owned(),
            role: Some(context.credential.role()),
            etag: cached.etag.clone(),
            record: CompactRemoteStateRecord::from(&cached.record),
        };
        let encoded = Zeroizing::new(
            serde_json::to_vec(&persisted)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let temporary = parent.join(format!(".remote-cache-{}.tmp", Uuid::new_v4()));
        write_private(&temporary, encoded.as_slice())?;
        replace_file(&temporary, path.as_path())
    }

    fn read_pending_controlled_operation(
        &self,
        context: &RemoteContext,
    ) -> LumoResult<Option<ControlledOperationRequest>> {
        let Some(path) = self.pending_operation_path.as_deref() else {
            return Ok(None);
        };
        let bytes = match fs::read(path.as_path()) {
            Ok(bytes) => Zeroizing::new(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        let persisted: PersistedControlledOperation = serde_json::from_slice(bytes.as_slice())
            .map_err(|_| LumoError::Storage("invalid pending controlled operation".to_owned()))?;
        if persisted.version != PENDING_OPERATION_VERSION {
            return Err(LumoError::Storage(
                "unsupported pending controlled operation".to_owned(),
            ));
        }
        if persisted.api_origin != self.base_url
            || persisted.group_id != context.credential.group_id()
            || persisted.device_id != context.credential.device_id()
        {
            return Err(LumoError::AuthenticationFailed);
        }
        let envelope: SealedPayload = persisted.request.try_into()?;
        let request: ControlledOperationRequest =
            context
                .cipher
                .open(&envelope, system_now_ms(), &mut ReplayGuard::default())?;
        validate_identifier("controlled operation", &request.operation_id)?;
        Ok(Some(request))
    }

    fn write_pending_controlled_operation(
        &self,
        context: &RemoteContext,
        request: &ControlledOperationRequest,
    ) -> LumoResult<()> {
        let Some(path) = self.pending_operation_path.as_deref() else {
            return Ok(());
        };
        let parent = path.parent().ok_or_else(|| {
            LumoError::Storage("pending controlled operation path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(storage_error)?;
        let now_ms = system_now_ms();
        let local_envelope =
            context
                .cipher
                .seal(request, now_ms, i64::MAX.saturating_sub(now_ms))?;
        let persisted = PersistedControlledOperation {
            version: PENDING_OPERATION_VERSION,
            api_origin: self.base_url.clone(),
            group_id: context.credential.group_id().to_owned(),
            device_id: context.credential.device_id().to_owned(),
            request: CompactSealedPayload::from(&local_envelope),
        };
        let encoded = Zeroizing::new(
            serde_json::to_vec(&persisted)
                .map_err(|error| LumoError::Serialization(error.to_string()))?,
        );
        let temporary = parent.join(format!(
            ".pending-controlled-operation-{}.tmp",
            Uuid::new_v4()
        ));
        write_private(&temporary, encoded.as_slice())?;
        install_new_file(&temporary, path.as_path())
    }

    fn clear_pending_controlled_operation(&self) -> LumoResult<()> {
        let Some(path) = self.pending_operation_path.as_deref() else {
            return Ok(());
        };
        match fs::remove_file(path.as_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn clear_context_cache(&self, context: &RemoteContext) -> LumoResult<()> {
        *lock_cache(&context.shared.cache) = None;
        if let Some(path) = self.cache_path.as_deref() {
            match fs::remove_file(path.as_path()) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(storage_error(error)),
            }
        }
        Ok(())
    }
}

impl StateRepository for RemoteRepository {
    fn load(&self) -> LumoResult<RuntimeState> {
        self.load_with_freshness().map(|loaded| loaded.state)
    }

    fn transact<T, F>(&self, operation: F) -> LumoResult<T>
    where
        F: FnOnce(&mut RuntimeState) -> LumoResult<T>,
    {
        let context = self.context()?;
        ensure_role(&context.credential, DeviceRole::Controller)?;
        // Mutations never hydrate from stale/offline cache: a fresh server CAS is mandatory.
        let current = self.fetch_record_network(&context)?;
        let expected_revision = current.as_ref().map(|record| record.revision);
        let mut state = current
            .map(|record| self.decode(&context, &record))
            .transpose()?
            .unwrap_or_default();
        let original = state.clone();
        let outcome = operation(&mut state);
        if state != original {
            self.put_record(
                &context,
                &PutStateRequest {
                    expected_revision,
                    record: self.encode(&context, &state)?,
                },
            )?;
        }
        outcome
    }
}

impl ControlledOperationPort for RemoteRepository {
    fn load_controlled_snapshot(&self) -> LumoResult<Option<AppSnapshot>> {
        let credential = self.credentials.require()?;
        if credential.role() == DeviceRole::Controller {
            return Ok(None);
        }
        self.load_member_with_freshness()
            .map(|loaded| Some(loaded.snapshot))
    }

    fn apply_controlled_operation(
        &self,
        operation: ControlledOperation,
    ) -> LumoResult<Option<ControlledOperationResponse>> {
        let credential = self.credentials.require()?;
        if credential.role() == DeviceRole::Controller {
            return Ok(None);
        }
        self.execute_controlled_operation(operation).map(Some)
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
        .pool_max_idle_per_host(4)
        .tcp_keepalive(Duration::from_secs(30))
        .tcp_nodelay(true)
        .min_tls_version(Version::TLS_1_2)
        .https_only(!allow_insecure_http)
        .redirect(Policy::none())
        .user_agent(concat!("Lumo/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn shared_remote_state(credential: &DeviceCredential) -> Arc<SharedRemoteState> {
    let states = SHARED_REMOTE_STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut states = states
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    states
        .entry((
            credential.api_origin().to_owned(),
            credential.group_id().to_owned(),
            credential.cache_fingerprint(),
        ))
        .or_insert_with(|| Arc::new(SharedRemoteState::default()))
        .clone()
}

fn lock_cache(cache: &Mutex<Option<CachedRecord>>) -> MutexGuard<'_, Option<CachedRecord>> {
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn controlled_operation_outcome_is_ambiguous(error: &LumoError) -> bool {
    matches!(
        error,
        LumoError::RemoteUnavailable
            | LumoError::Serialization(_)
            | LumoError::Storage(_)
            | LumoError::RevisionConflict
            | LumoError::ExpiredMessage
            | LumoError::ReplayDetected
    )
}

fn parse_success(response: Response) -> LumoResult<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.json::<ApiErrorBody>().ok();
    let code = body.as_ref().map(|body| body.code.as_str());
    Err(match (status, code) {
        (StatusCode::UNAUTHORIZED, Some("clock_skew")) => LumoError::ExpiredMessage,
        (StatusCode::UNAUTHORIZED, Some("authentication_failed")) => {
            LumoError::AuthenticationFailed
        }
        (StatusCode::FORBIDDEN, Some("tracking_disabled")) => LumoError::TrackingDisabled,
        (StatusCode::FORBIDDEN, Some("unauthorized")) => LumoError::Unauthorized,
        (StatusCode::NOT_FOUND, Some("not_found")) => {
            LumoError::NotFound("remote resource".to_owned())
        }
        (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND, None) => {
            LumoError::RemoteUnavailable
        }
        (StatusCode::GONE, _) => LumoError::InvalidInvitation,
        (StatusCode::CONFLICT, Some("replay_detected")) => LumoError::ReplayDetected,
        (StatusCode::CONFLICT, _) => LumoError::RevisionConflict,
        (StatusCode::TOO_MANY_REQUESTS, _) => LumoError::RateLimited,
        (StatusCode::PAYLOAD_TOO_LARGE, _) => {
            LumoError::InvalidInput("remote state exceeds the API size limit".to_owned())
        }
        (StatusCode::BAD_REQUEST, Some("invalid_invitation")) => LumoError::InvalidInvitation,
        (StatusCode::BAD_REQUEST, _) => LumoError::InvalidInput(
            body.map(|value| value.message)
                .unwrap_or_else(|| "remote API rejected the request".to_owned()),
        ),
        (status, _) if status.is_server_error() || is_transient_status(status) => {
            LumoError::RemoteUnavailable
        }
        _ => LumoError::Storage("remote API rejected the request".to_owned()),
    })
}

fn response_etag(response: &Response) -> Option<String> {
    response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn response_decode_error(_error: reqwest::Error) -> LumoError {
    LumoError::Serialization("invalid remote API response".to_owned())
}

fn ensure_role(credential: &DeviceCredential, expected: DeviceRole) -> LumoResult<()> {
    if credential.role() == expected {
        Ok(())
    } else {
        Err(LumoError::Unauthorized)
    }
}

fn validate_identifier(name: &str, value: &str) -> LumoResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| LumoError::InvalidInput(format!("invalid {name} identifier")))
}

fn request_nonce() -> String {
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    URL_SAFE_NO_PAD.encode(nonce)
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
    thread::sleep(Duration::from_millis(if attempt == 0 { 100 } else { 250 }));
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

fn write_private(path: &Path, bytes: &[u8]) -> LumoResult<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

fn replace_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    if destination.exists() {
        fs::remove_file(destination).map_err(storage_error)?;
    }
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::remove_file(temporary);
        return Err(storage_error(error));
    }
    Ok(())
}

fn install_new_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    if destination.exists() {
        let _ = fs::remove_file(temporary);
        return Err(LumoError::Storage(
            "pending controlled operation already exists".to_owned(),
        ));
    }
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::remove_file(temporary);
        return Err(storage_error(error));
    }
    // Android guarantees the app-private file sandbox, but not every OEM allows opening the
    // containing directory for fsync. The payload itself was synced before the atomic rename.
    #[cfg(all(unix, not(target_os = "android")))]
    {
        let parent = destination.parent().ok_or_else(|| {
            LumoError::Storage("pending controlled operation path has no parent".to_owned())
        })?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(storage_error)?;
    }
    Ok(())
}

fn storage_error(error: impl fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use axum::{
        extract::State,
        http::HeaderMap,
        response::{IntoResponse, Response as AxumResponse},
        routing::{get, post},
        Json, Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use lumo_core::{
        application::{CreateGroupInput, SetTrackingInput},
        domain::PermissionState,
        LumoService,
    };
    use tokio::net::TcpListener;

    use super::*;

    fn credential(origin: &str, group_id: Uuid) -> DeviceCredential {
        credential_for_role(origin, group_id, DeviceRole::Controller)
    }

    fn credential_for_role(origin: &str, group_id: Uuid, role: DeviceRole) -> DeviceCredential {
        DeviceCredential::from_parts(
            origin,
            group_id.to_string(),
            Uuid::new_v4().to_string(),
            role,
            URL_SAFE_NO_PAD.encode([7_u8; 32]),
            URL_SAFE_NO_PAD.encode([9_u8; 32]),
            true,
        )
        .expect("credential")
    }

    #[test]
    fn repository_debug_redacts_device_token() {
        let credential = credential("http://127.0.0.1:3000", Uuid::new_v4());
        let token = credential.device_token().to_owned();
        let repository = RemoteRepository::new("http://127.0.0.1:3000", Some(credential), true)
            .expect("repository");
        assert!(!format!("{repository:?}").contains(&token));
    }

    #[test]
    fn persistent_cache_is_bound_to_origin_group_and_authenticated_ciphertext() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let group_id = Uuid::new_v4();
        let active_credential = credential("http://127.0.0.1:3000", group_id);
        let repository = RemoteRepository::new_with_cache(
            "http://127.0.0.1:3000",
            Some(active_credential.clone()),
            true,
            Some(directory.path()),
        )
        .expect("repository");
        let context = repository.context().expect("context");
        let state = RuntimeState {
            revision: 1,
            ..RuntimeState::default()
        };
        let record = repository.encode(&context, &state).expect("record");
        repository
            .cache_record(&context, Some("\"etag\"".to_owned()), record)
            .expect("cache");

        let wrong_origin = RemoteRepository::new_with_cache(
            "http://127.0.0.1:3001",
            Some(credential("http://127.0.0.1:3001", group_id)),
            true,
            Some(directory.path()),
        )
        .expect("wrong origin");
        assert!(matches!(
            wrong_origin.read_persisted_cache(&wrong_origin.context().expect("context")),
            Err(LumoError::AuthenticationFailed)
        ));

        let wrong_group = RemoteRepository::new_with_cache(
            "http://127.0.0.1:3000",
            Some(credential("http://127.0.0.1:3000", Uuid::new_v4())),
            true,
            Some(directory.path()),
        )
        .expect("wrong group");
        assert!(matches!(
            wrong_group.read_persisted_cache(&wrong_group.context().expect("context")),
            Err(LumoError::AuthenticationFailed)
        ));

        let path = directory.path().join(CACHE_FILE_NAME);
        let persisted: PersistedRemoteCache =
            serde_json::from_slice(&fs::read(&path).expect("cache bytes")).expect("cache json");
        let mut wrong_role = persisted.clone();
        wrong_role.role = Some(DeviceRole::Controlled);
        fs::write(
            &path,
            serde_json::to_vec(&wrong_role).expect("wrong-role cache"),
        )
        .expect("write wrong-role cache");
        assert!(matches!(
            repository.read_persisted_cache(&context),
            Err(LumoError::AuthenticationFailed)
        ));

        let mut persisted = persisted;
        persisted.record = CompactRemoteStateRecord::from(
            &repository.encode(&context, &state).expect("second record"),
        );
        let mut value = serde_json::to_value(persisted).expect("value");
        value["record"]["envelope"]["ciphertext"] =
            serde_json::Value::String(URL_SAFE_NO_PAD.encode([0_u8; 32]));
        fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("tamper");
        assert!(matches!(
            repository.read_persisted_cache(&context),
            Err(LumoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn unbound_remote_repository_rejects_state_and_protected_actions() {
        let repository =
            RemoteRepository::new("http://127.0.0.1:3000", None, true).expect("repository");
        assert!(matches!(
            repository.load(),
            Err(LumoError::AuthenticationFailed)
        ));
        assert!(matches!(
            repository.create_invitation("123456"),
            Err(LumoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn failed_remote_leave_keeps_the_controlled_credential_installed() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral listener");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        drop(listener);
        let credential = credential_for_role(&origin, Uuid::new_v4(), DeviceRole::Controlled);
        let device_id = credential.device_id().to_owned();
        let repository =
            RemoteRepository::new(&origin, Some(credential), true).expect("repository");

        assert_eq!(
            repository.leave_group("123456"),
            Err(LumoError::RemoteUnavailable)
        );
        assert_eq!(
            repository
                .credential()
                .expect("credential slot")
                .expect("retained credential")
                .device_id(),
            device_id
        );
    }

    #[derive(Clone)]
    struct ExpectedAuth {
        device_id: String,
        token: String,
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn authenticated_request_uses_bearer_device_and_24_byte_nonce() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let credential = credential(&base_url, Uuid::new_v4());
        let expected = ExpectedAuth {
            device_id: credential.device_id().to_owned(),
            token: credential.device_token().to_owned(),
        };
        let path = group_state_path(credential.group_id());
        let app = Router::new()
            .route(&path, get(assert_authenticated_request))
            .with_state(Arc::new(expected));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        tokio::task::spawn_blocking(move || {
            let repository =
                RemoteRepository::new(&base_url, Some(credential), true).expect("repository");
            repository.load()
        })
        .await
        .expect("client task")
        .expect("state");
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn offline_restart_reads_verified_cache_but_never_mutates_from_it() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let credential = credential(&base_url, Uuid::new_v4());
        let expected = RuntimeState {
            revision: 1,
            ..RuntimeState::default()
        };
        let encode_url = base_url.clone();
        let encode_credential = credential.clone();
        let encode_directory = directory.path().to_path_buf();
        let encode_state = expected.clone();
        let (path, record) = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new_with_cache(
                &encode_url,
                Some(encode_credential),
                true,
                Some(&encode_directory),
            )
            .expect("repository");
            let context = repository.context().expect("context");
            let record = repository.encode(&context, &encode_state).expect("record");
            (context.state_path, record)
        })
        .await
        .expect("encode task");
        let compact = CompactRemoteStateRecord::from(&record);
        let app = Router::new().route(
            &path,
            get(move || {
                let compact = compact.clone();
                async move { Json(compact) }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let online_url = base_url.clone();
        let online_credential = credential.clone();
        let online_directory = directory.path().to_path_buf();
        let online = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new_with_cache(
                &online_url,
                Some(online_credential),
                true,
                Some(&online_directory),
            )?;
            repository.load_with_freshness()
        })
        .await
        .expect("online task")
        .expect("online state");
        assert_eq!(online.freshness, RemoteFreshness::Fresh);
        assert_eq!(online.state, expected);
        server.abort();
        let _ = server.await;
        let offline_directory = directory.path().to_path_buf();
        let operation_ran = Arc::new(AtomicBool::new(false));
        let observed = operation_ran.clone();
        let (offline, mutation) = tokio::task::spawn_blocking(move || {
            let restarted = RemoteRepository::new_with_cache(
                &base_url,
                Some(credential),
                true,
                Some(&offline_directory),
            )
            .expect("restarted repository");
            let context = restarted.context().expect("context");
            *lock_cache(&context.shared.cache) = None;
            let offline = restarted.load_with_freshness();
            let mutation = restarted.transact(|state| {
                observed.store(true, Ordering::SeqCst);
                state.revision = state.revision.saturating_add(1);
                Ok(())
            });
            (offline, mutation)
        })
        .await
        .expect("offline task");
        let offline = offline.expect("cached state");
        assert_eq!(offline.freshness, RemoteFreshness::Stale);
        assert_eq!(offline.state, expected);

        assert_eq!(mutation, Err(LumoError::RemoteUnavailable));
        assert!(!operation_ran.load(Ordering::SeqCst));
    }

    #[derive(Clone)]
    struct CommittedOperationState {
        attempts: Arc<AtomicUsize>,
        operation_ids: Arc<Mutex<Vec<String>>>,
        cipher: SessionCipher,
        snapshot: AppSnapshot,
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn controlled_operation_reuses_id_after_response_loss_and_restart() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let group_id = Uuid::new_v4();
        let credential = credential_for_role(&base_url, group_id, DeviceRole::Controlled);

        let mut runtime = RuntimeState::default();
        LumoService
            .create_group(
                &mut runtime,
                CreateGroupInput {
                    name: "Familia".to_owned(),
                    supervisor_name: "Supervisor".to_owned(),
                    supervisor_phone: "+34123456789".to_owned(),
                    tracked_person_name: "Miembro".to_owned(),
                    tracked_person_phone: "+34987654321".to_owned(),
                    pin: "123456".to_owned(),
                },
                system_now_ms(),
            )
            .expect("group");
        runtime.group.as_mut().expect("group state").id = group_id.to_string();
        let server_state = CommittedOperationState {
            attempts: Arc::new(AtomicUsize::new(0)),
            operation_ids: Arc::new(Mutex::new(Vec::new())),
            cipher: SessionCipher::from_key([9_u8; 32]),
            snapshot: runtime.member_snapshot(),
        };
        let path = group_member_operations_path(&group_id.to_string());
        let app = Router::new()
            .route(&path, post(commit_operation_then_drop_response))
            .with_state(server_state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });

        let operation = ControlledOperation::SetTracking(SetTrackingInput {
            precise_permission: PermissionState::Granted,
            background_permission: PermissionState::Granted,
            battery_optimization_disabled: true,
            enabled: true,
        });
        let first_url = base_url.clone();
        let first_credential = credential.clone();
        let first_directory = directory.path().to_path_buf();
        let first_operation = operation.clone();
        let first = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new_with_cache(
                &first_url,
                Some(first_credential),
                true,
                Some(&first_directory),
            )?;
            repository.execute_controlled_operation(first_operation)
        })
        .await
        .expect("first client task");
        assert_eq!(first, Err(LumoError::RemoteUnavailable));
        assert!(directory.path().join(PENDING_OPERATION_FILE_NAME).is_file());

        let restart_directory = directory.path().to_path_buf();
        let second = tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new_with_cache(
                &base_url,
                Some(credential),
                true,
                Some(&restart_directory),
            )?;
            repository.execute_controlled_operation(operation)
        })
        .await
        .expect("restarted client task")
        .expect("idempotent replay response");
        assert_eq!(second.snapshot, server_state.snapshot);
        assert!(!directory.path().join(PENDING_OPERATION_FILE_NAME).exists());

        let ids = server_state
            .operation_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(ids.len(), MAX_TRANSPORT_ATTEMPTS + 1);
        assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_structured_api_errors_receive_security_classification() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let base_url = format!("http://{}", listener.local_addr().expect("address"));
        let app = Router::new()
            .route(
                "/authentication",
                get(|| async {
                    (
                        StatusCode::UNAUTHORIZED,
                        Json(ApiErrorBody {
                            code: "authentication_failed".to_owned(),
                            message: "rejected".to_owned(),
                        }),
                    )
                }),
            )
            .route(
                "/tracking",
                get(|| async {
                    (
                        StatusCode::FORBIDDEN,
                        Json(ApiErrorBody {
                            code: "tracking_disabled".to_owned(),
                            message: "disabled".to_owned(),
                        }),
                    )
                }),
            )
            .route(
                "/authorization",
                get(|| async {
                    (
                        StatusCode::FORBIDDEN,
                        Json(ApiErrorBody {
                            code: "unauthorized".to_owned(),
                            message: "forbidden".to_owned(),
                        }),
                    )
                }),
            )
            .route(
                "/waf-forbidden",
                get(|| async { (StatusCode::FORBIDDEN, "<html>blocked</html>").into_response() }),
            )
            .route(
                "/waf-not-found",
                get(|| async { (StatusCode::NOT_FOUND, "<html>missing</html>").into_response() }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        tokio::task::spawn_blocking(move || {
            let repository = RemoteRepository::new(&base_url, None, true).expect("repository");
            let classify = |path: &str| {
                let response = repository
                    .send_public(Method::GET, path, &[], false)
                    .expect("HTTP response");
                parse_success(response).expect_err("error response")
            };
            assert_eq!(classify("/authentication"), LumoError::AuthenticationFailed);
            assert_eq!(classify("/tracking"), LumoError::TrackingDisabled);
            assert_eq!(classify("/authorization"), LumoError::Unauthorized);
            assert_eq!(classify("/waf-forbidden"), LumoError::RemoteUnavailable);
            assert_eq!(classify("/waf-not-found"), LumoError::RemoteUnavailable);
        })
        .await
        .expect("client task");
        server.abort();
    }

    async fn assert_authenticated_request(
        State(expected): State<Arc<ExpectedAuth>>,
        headers: HeaderMap,
    ) -> StatusCode {
        assert_eq!(
            headers
                .get(DEVICE_ID_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(expected.device_id.as_str())
        );
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some(format!("Bearer {}", expected.token).as_str())
        );
        assert!(headers
            .get(TIMESTAMP_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok())
            .is_some());
        let nonce = headers
            .get(NONCE_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
            .expect("nonce");
        assert_eq!(nonce.len(), 24);
        StatusCode::NO_CONTENT
    }

    async fn commit_operation_then_drop_response(
        State(state): State<CommittedOperationState>,
        Json(body): Json<MemberOperationEnvelopeRequest>,
    ) -> AxumResponse {
        let envelope: SealedPayload = body.envelope.try_into().expect("request envelope");
        let request: ControlledOperationRequest = state
            .cipher
            .open(&envelope, system_now_ms(), &mut ReplayGuard::default())
            .expect("controlled operation");
        state
            .operation_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request.operation_id);

        if state.attempts.fetch_add(1, Ordering::SeqCst) < MAX_TRANSPORT_ATTEMPTS {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }

        let response = ControlledOperationResponse {
            snapshot: state.snapshot.clone(),
            processed: None,
        };
        let record = RemoteStateRecord {
            revision: response.snapshot.revision,
            envelope: state
                .cipher
                .seal(&response, system_now_ms(), MEMBER_OPERATION_TTL_MS)
                .expect("response envelope"),
        };
        Json(CompactRemoteStateRecord::from(&record)).into_response()
    }
}
