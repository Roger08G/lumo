use std::net::{IpAddr, SocketAddr};

use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{header::ETAG, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use lumo_core::{
    security::{validate_pin, SessionCipher},
    LumoError, LumoResult,
};
use lumo_protocol::{
    CompactPutStateRequest, CompactRemoteStateRecord, ConsumeInvitationRequest, CreateGroupRequest,
    CreateInvitationRequest, DeviceCredentialResponse, DeviceListResponse, DeviceRole,
    InvitationResponse, MemberOperationEnvelopeRequest, ProtectedActionRequest, PutStateRequest,
    RemoteStateRecord,
};
use uuid::Uuid;

use crate::{
    auth::{
        authenticate_device_mutation, authenticate_device_read, parse_device_auth, system_now_ms,
    },
    routes::{api_error, invalid_body, revision_conflict},
    storage::{ConsumeInvitation, Idempotent, NewDevice, NewGroup, NewInvitation},
    ApiState,
};

const IF_NONE_MATCH_HEADER: &str = "if-none-match";
const MAX_DEVICE_NAME_CHARS: usize = 64;
const MEMBER_RESPONSE_TTL_MS: i64 = 5 * 60 * 1_000;

pub async fn create_group(
    State(state): State<ApiState>,
    ConnectInfo(peer_address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<CreateGroupRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if Uuid::parse_str(&request.request_id).is_err() {
        return api_error(LumoError::InvalidInput(
            "requestId must be a UUID".to_owned(),
        ));
    }
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let device_name = match validate_device_name(&request.device_name) {
        Ok(name) => name,
        Err(error) => return api_error(error),
    };

    let now_ms = system_now_ms();
    let peer = client_ip(&headers, peer_address, state.trust_proxy_headers);
    let bootstrap_key = state.master.bootstrap_key(&format!("ip:{peer}"));
    let request_digest = state.master.idempotency_digest(
        "create_group",
        &[request.pin.as_bytes(), device_name.as_bytes()],
    );
    // The public bootstrap performs memory-hard PIN hashing. Serialize this
    // complete replay/reservation/hash/commit sequence so repeated requestIds
    // neither consume quota twice nor multiply Argon2 memory on the 256 MiB
    // production container.
    let _hash_permit = match state.bootstrap_hash_gate.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => return api_error(LumoError::RateLimited),
    };
    let replay_store = state.store.clone();
    let replay_master = state.master.clone();
    let replay_request_id = request.request_id.clone();
    let replay_digest = request_digest.clone();
    match run_blocking(move || {
        replay_store.load_create_group_replay_v2(
            &replay_master,
            &replay_request_id,
            &replay_digest,
            now_ms,
        )
    })
    .await
    {
        Ok(Some(Idempotent::Replay(credential))) => {
            return (StatusCode::CREATED, Json(credential)).into_response();
        }
        Ok(Some(Idempotent::Conflict)) => return idempotency_conflict(),
        Ok(None) => {}
        Ok(Some(Idempotent::Fresh(_))) => unreachable!("stored records are replays"),
        Err(error) => return api_error(error),
    }

    let reservation_store = state.store.clone();
    let reservation_key = bootstrap_key.clone();
    let reservation_request_id = request.request_id.clone();
    let reservation_digest = request_digest.clone();
    let reservation_limits = state.limits.clone();
    match run_blocking(move || {
        reservation_store.reserve_group_bootstrap_v2(
            &reservation_key,
            &reservation_request_id,
            &reservation_digest,
            now_ms,
            &reservation_limits,
        )
    })
    .await
    {
        Ok(Idempotent::Fresh(()) | Idempotent::Replay(())) => {}
        Ok(Idempotent::Conflict) => return idempotency_conflict(),
        Err(error) => return api_error(error),
    }

    let group_id = Uuid::new_v4().to_string();
    let device_id = Uuid::new_v4().to_string();
    let device_token = state.master.random_token();
    let state_key = state.master.generate_state_key();
    let (state_key_nonce, state_key_ciphertext) =
        match state.master.wrap_state_key(&group_id, &state_key) {
            Ok(wrapped) => wrapped,
            Err(error) => return api_error(error),
        };
    let token_hash = state.master.token_hash(&device_token);
    let pin = request.pin;
    let store = state.store.clone();
    let limits = state.limits.clone();
    let master = state.master.clone();
    let new_group = NewGroup {
        id: group_id.clone(),
        pin_hash: String::new(),
        state_key_nonce,
        state_key_ciphertext,
        controller: NewDevice {
            id: device_id.clone(),
            name: device_name,
            role: DeviceRole::Controller,
            token_hash,
            member_key_nonce: None,
            member_key_ciphertext: None,
        },
    };
    let credential = DeviceCredentialResponse {
        group_id,
        device_id,
        role: DeviceRole::Controller,
        device_token,
        state_key: URL_SAFE_NO_PAD.encode(state_key),
    };
    let request_id = request.request_id;
    let result = run_blocking(move || {
        let mut new_group = new_group;
        new_group.pin_hash = master.hash_group_pin(&new_group.id, &pin)?;
        store.create_group_idempotent_v2(
            &master,
            &request_id,
            &request_digest,
            &new_group,
            &credential,
            now_ms,
            &limits,
        )
    })
    .await;
    match result {
        Ok(Idempotent::Fresh(credential) | Idempotent::Replay(credential)) => {
            (StatusCode::CREATED, Json(credential)).into_response()
        }
        Ok(Idempotent::Conflict) => idempotency_conflict(),
        Err(error) => api_error(error),
    }
}

pub async fn get_group_state(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let actor = match authenticate_device_read(&state, &group_id, &headers, system_now_ms()).await {
        Ok(actor) => actor,
        Err(error) => return api_error(error),
    };
    if actor.role != DeviceRole::Controller {
        return api_error(LumoError::Unauthorized);
    }
    let store = state.store.clone();
    let state_group_id = group_id.clone();
    match run_blocking(move || store.load_state_v2(&state_group_id)).await {
        Ok(Some(record)) => {
            let etag = state_etag(&group_id, &record);
            if is_not_modified(&headers, &etag) {
                return with_etag(StatusCode::NOT_MODIFIED.into_response(), &etag);
            }
            with_etag(
                Json(CompactRemoteStateRecord::from(&record)).into_response(),
                &etag,
            )
        }
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn put_group_state(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor =
        match authenticate_device_mutation(&state, &group_id, &headers, system_now_ms()).await {
            Ok(actor) => actor,
            Err(error) => return api_error(error),
        };
    if actor.role != DeviceRole::Controller {
        return api_error(LumoError::Unauthorized);
    }
    let request = match serde_json::from_slice::<CompactPutStateRequest>(&body) {
        Ok(request) => match PutStateRequest::try_from(request) {
            Ok(request) => request,
            Err(error) => return api_error(error),
        },
        Err(error) => return invalid_body(error),
    };
    let etag = state_etag(&group_id, &request.record);
    let store = state.store.clone();
    let now_ms = system_now_ms();
    let result = run_blocking(move || {
        store.compare_and_swap_v2(
            &group_id,
            request.expected_revision,
            &request.record,
            now_ms,
        )
    })
    .await;
    match result {
        Ok(true) => with_etag(StatusCode::NO_CONTENT.into_response(), &etag),
        Ok(false) => revision_conflict(),
        Err(error) => api_error(error),
    }
}

pub async fn create_invitation(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor =
        match authenticate_device_mutation(&state, &group_id, &headers, system_now_ms()).await {
            Ok(actor) => actor,
            Err(error) => return api_error(error),
        };
    if actor.role != DeviceRole::Controller {
        return api_error(LumoError::Unauthorized);
    }
    let request = match serde_json::from_slice::<CreateInvitationRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let invitation_id = Uuid::new_v4().to_string();
    let token = state.master.random_token();
    let token_hash = state.master.token_hash(&token);
    let master = state.master.clone();
    let now_ms = system_now_ms();
    let expires_at_ms = now_ms.saturating_add(state.limits.invite_ttl_ms);
    let store = state.store.clone();
    let limits = state.limits.clone();
    let new_invitation = NewInvitation {
        id: invitation_id.clone(),
        group_id,
        controller_id: actor.device_id,
        pin: zeroize::Zeroizing::new(request.pin),
        token_hash,
        role: request.role,
        created_at_ms: now_ms,
    };
    let result =
        run_blocking(move || store.create_invitation_v2(&master, &new_invitation, &limits)).await;
    match result {
        Ok(()) => (
            StatusCode::CREATED,
            Json(InvitationResponse {
                invitation_id,
                token,
                expires_at_ms,
                role: request.role,
            }),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn consume_invitation(
    State(state): State<ApiState>,
    Path(invitation_id): Path<String>,
    body: Bytes,
) -> Response {
    if Uuid::parse_str(&invitation_id).is_err() {
        return api_error(LumoError::InvalidInvitation);
    }
    let request = match serde_json::from_slice::<ConsumeInvitationRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if Uuid::parse_str(&request.request_id).is_err() {
        return api_error(LumoError::InvalidInput(
            "requestId must be a UUID".to_owned(),
        ));
    }
    if validate_pin(&request.pin).is_err() || !valid_token(&request.token) {
        return api_error(LumoError::InvalidInvitation);
    }
    let device_name = match validate_device_name(&request.device_name) {
        Ok(name) => name,
        Err(error) => return api_error(error),
    };
    let device_id = Uuid::new_v4().to_string();
    let device_token = state.master.random_token();
    let member_key = state.master.generate_state_key();
    let request_digest = state.master.idempotency_digest(
        "consume_invitation",
        &[
            invitation_id.as_bytes(),
            request.token.as_bytes(),
            request.pin.as_bytes(),
            device_name.as_bytes(),
        ],
    );
    let new_device = NewDevice {
        id: device_id.clone(),
        name: device_name,
        role: DeviceRole::Controlled,
        token_hash: state.master.token_hash(&device_token),
        member_key_nonce: None,
        member_key_ciphertext: None,
    };
    let store = state.store.clone();
    let master = state.master.clone();
    let limits = state.limits.clone();
    let now_ms = system_now_ms();
    let consumption = ConsumeInvitation {
        request_id: request.request_id,
        request_digest,
        invitation_id,
        token: zeroize::Zeroizing::new(request.token),
        pin: zeroize::Zeroizing::new(request.pin),
        device: new_device,
        consumed_at_ms: now_ms,
        device_token: zeroize::Zeroizing::new(device_token),
        member_key,
    };
    let result =
        run_blocking(move || store.consume_invitation_v2(&master, &consumption, &limits)).await;
    match result {
        Ok(Idempotent::Fresh(consumed) | Idempotent::Replay(consumed)) => {
            (StatusCode::CREATED, Json(consumed.credential)).into_response()
        }
        Ok(Idempotent::Conflict) => idempotency_conflict(),
        Err(error) => api_error(error),
    }
}

pub async fn get_group_member(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let now_ms = system_now_ms();
    let auth = match parse_device_auth(&group_id, &headers, now_ms) {
        Ok(auth) => auth,
        Err(error) => return api_error(error),
    };
    let store = state.store.clone();
    let master = state.master.clone();
    match run_blocking(move || store.load_member_snapshot_v2(&master, &group_id, &auth, now_ms))
        .await
    {
        Ok(Some(result)) => {
            let revision = result.snapshot.revision;
            match SessionCipher::from_key(result.member_key).seal(
                &result.snapshot,
                now_ms,
                MEMBER_RESPONSE_TTL_MS,
            ) {
                Ok(envelope) => Json(CompactRemoteStateRecord::from(&RemoteStateRecord {
                    revision,
                    envelope,
                }))
                .into_response(),
                Err(error) => api_error(error),
            }
        }
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn apply_group_member_operation(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<MemberOperationEnvelopeRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    let envelope = match lumo_core::security::SealedPayload::try_from(request.envelope) {
        Ok(envelope) => envelope,
        Err(error) => return api_error(error),
    };
    let now_ms = system_now_ms();
    let auth = match parse_device_auth(&group_id, &headers, now_ms) {
        Ok(auth) => auth,
        Err(error) => return api_error(error),
    };
    let store = state.store.clone();
    let master = state.master.clone();
    match run_blocking(move || {
        store.apply_member_operation_v2(&master, &group_id, &auth, &envelope, now_ms)
    })
    .await
    {
        Ok(Idempotent::Fresh(result) | Idempotent::Replay(result)) => {
            let revision = result.response.snapshot.revision;
            match SessionCipher::from_key(result.member_key).seal(
                &result.response,
                now_ms,
                MEMBER_RESPONSE_TTL_MS,
            ) {
                Ok(envelope) => Json(CompactRemoteStateRecord::from(&RemoteStateRecord {
                    revision,
                    envelope,
                }))
                .into_response(),
                Err(error) => api_error(error),
            }
        }
        Ok(Idempotent::Conflict) => idempotency_conflict(),
        Err(error) => api_error(error),
    }
}

pub async fn verify_group_pin(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<ProtectedActionRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let now_ms = system_now_ms();
    let auth = match parse_device_auth(&group_id, &headers, now_ms) {
        Ok(auth) => auth,
        Err(error) => return api_error(error),
    };
    let store = state.store.clone();
    let master = state.master.clone();
    match run_blocking(move || {
        store.verify_pin_authorized_v2(&master, &group_id, &auth, &request.pin, now_ms)
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn list_devices(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let actor = match authenticate_device_read(&state, &group_id, &headers, system_now_ms()).await {
        Ok(actor) => actor,
        Err(error) => return api_error(error),
    };
    if actor.role != DeviceRole::Controller {
        return api_error(LumoError::Unauthorized);
    }
    let store = state.store.clone();
    match run_blocking(move || store.list_devices_v2(&group_id)).await {
        Ok(devices) => Json(DeviceListResponse { devices }).into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn revoke_device(
    State(state): State<ApiState>,
    Path((group_id, device_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if Uuid::parse_str(&device_id).is_err() {
        return api_error(LumoError::NotFound("device".to_owned()));
    }
    let request = match serde_json::from_slice::<ProtectedActionRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let now_ms = system_now_ms();
    let auth = match parse_device_auth(&group_id, &headers, now_ms) {
        Ok(auth) => auth,
        Err(error) => return api_error(error),
    };
    let store = state.store.clone();
    let master = state.master.clone();
    match run_blocking(move || {
        store.revoke_device_authorized_v2(
            &master,
            &group_id,
            &auth,
            &device_id,
            &request.pin,
            now_ms,
        )
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn leave_group(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor =
        match authenticate_device_mutation(&state, &group_id, &headers, system_now_ms()).await {
            Ok(actor) => actor,
            Err(error) => return api_error(error),
        };
    let request = match serde_json::from_slice::<ProtectedActionRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let store = state.store.clone();
    let master = state.master.clone();
    let now_ms = system_now_ms();
    match run_blocking(move || {
        store.leave_group_v2(&master, &group_id, &actor.device_id, &request.pin, now_ms)
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn delete_group(
    State(state): State<ApiState>,
    Path(group_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let actor =
        match authenticate_device_mutation(&state, &group_id, &headers, system_now_ms()).await {
            Ok(actor) => actor,
            Err(error) => return api_error(error),
        };
    if actor.role != DeviceRole::Controller {
        return api_error(LumoError::Unauthorized);
    }
    let request = match serde_json::from_slice::<ProtectedActionRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    if let Err(error) = validate_pin(&request.pin) {
        return api_error(error);
    }
    let store = state.store.clone();
    let master = state.master.clone();
    let now_ms = system_now_ms();
    match run_blocking(move || {
        store.delete_group_v2(&master, &group_id, &actor.device_id, &request.pin, now_ms)
    })
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

fn validate_device_name(value: &str) -> LumoResult<String> {
    let value = value.trim();
    let count = value.chars().count();
    if count == 0 || count > MAX_DEVICE_NAME_CHARS || value.chars().any(char::is_control) {
        return Err(LumoError::InvalidInput(
            "device name must contain between 1 and 64 visible characters".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn valid_token(token: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(token)
        .ok()
        .is_some_and(|decoded| decoded.len() == 32)
}

fn idempotency_conflict() -> Response {
    (
        StatusCode::CONFLICT,
        Json(lumo_protocol::ApiErrorBody {
            code: "idempotency_conflict".to_owned(),
            message: "requestId was already used for a different request".to_owned(),
        }),
    )
        .into_response()
}

fn client_ip(headers: &HeaderMap, peer_address: SocketAddr, trust_proxy_headers: bool) -> String {
    if trust_proxy_headers {
        if let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .and_then(|value| value.parse::<IpAddr>().ok())
        {
            return ip.to_string();
        }
    }
    peer_address.ip().to_string()
}

fn state_etag(group_id: &str, record: &RemoteStateRecord) -> String {
    format!(
        "\"lumo-v2-{group_id}-{}-{}\"",
        record.revision, record.envelope.message_id
    )
}

fn is_not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(IF_NONE_MATCH_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .map(str::trim)
                .any(|candidate| candidate == "*" || candidate == etag)
        })
}

fn with_etag(mut response: Response, etag: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(etag) {
        response.headers_mut().insert(ETAG, value);
    }
    response
}

async fn run_blocking<T, F>(operation: F) -> LumoResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> LumoResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| LumoError::Storage("API storage task failed".to_owned()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ip_ignores_spoofable_forwarding_headers() {
        let peer: SocketAddr = "127.0.0.1:3000".parse().expect("peer");
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.10".parse().expect("header"));
        headers.insert("cf-connecting-ip", "203.0.113.11".parse().expect("header"));

        assert_eq!(client_ip(&headers, peer, true), "127.0.0.1");

        headers.insert("x-real-ip", "198.51.100.20".parse().expect("header"));
        assert_eq!(client_ip(&headers, peer, true), "198.51.100.20");
        assert_eq!(client_ip(&headers, peer, false), "127.0.0.1");
    }
}
