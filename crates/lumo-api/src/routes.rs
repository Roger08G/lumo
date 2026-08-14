use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::{header::ETAG, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use lumo_core::{LumoError, LumoResult};
use lumo_protocol::{
    ApiErrorBody, CompactPutStateRequest, CompactRemoteStateRecord, HealthResponse,
    PutStateRequest, RemoteStateRecord, API_VERSION,
};

use crate::{
    auth::{authenticate, system_now_ms},
    storage::ApiStore,
    ApiState,
};

const IF_NONE_MATCH_HEADER: &str = "if-none-match";

pub async fn health(State(state): State<ApiState>) -> Response {
    match tokio::task::spawn_blocking(move || state.store.healthcheck()).await {
        Ok(Ok(())) => Json(HealthResponse {
            status: "ok".to_owned(),
            api_version: API_VERSION.to_owned(),
        })
        .into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorBody {
                code: "unhealthy".to_owned(),
                message: "service unavailable".to_owned(),
            }),
        )
            .into_response(),
    }
}

pub async fn get_state(
    State(state): State<ApiState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authenticate(&state, &method, uri.path(), &headers, &[], system_now_ms()) {
        return api_error(error);
    }
    match load_state(state.store).await {
        Ok(Some(record)) => {
            let etag = state_etag("v1", &record);
            if is_not_modified(&headers, &etag) {
                return with_etag(StatusCode::NOT_MODIFIED.into_response(), &etag);
            }
            with_etag(Json(record).into_response(), &etag)
        }
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

pub async fn get_compact_state(
    State(state): State<ApiState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authenticate(&state, &method, uri.path(), &headers, &[], system_now_ms()) {
        return api_error(error);
    }
    match load_state(state.store).await {
        Ok(Some(record)) => {
            let etag = state_etag("compact", &record);
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

pub async fn put_state(
    State(state): State<ApiState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(error) = authenticate(
        &state,
        &method,
        uri.path(),
        &headers,
        &body,
        system_now_ms(),
    ) {
        return api_error(error);
    }
    let request = match serde_json::from_slice::<PutStateRequest>(&body) {
        Ok(request) => request,
        Err(error) => return invalid_body(error),
    };
    store_state(state.store, request, "v1").await
}

pub async fn put_compact_state(
    State(state): State<ApiState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(error) = authenticate(
        &state,
        &method,
        uri.path(),
        &headers,
        &body,
        system_now_ms(),
    ) {
        return api_error(error);
    }
    let request = match serde_json::from_slice::<CompactPutStateRequest>(&body) {
        Ok(request) => match PutStateRequest::try_from(request) {
            Ok(request) => request,
            Err(error) => return api_error(error),
        },
        Err(error) => return invalid_body(error),
    };
    store_state(state.store, request, "compact").await
}

async fn load_state(store: ApiStore) -> LumoResult<Option<RemoteStateRecord>> {
    tokio::task::spawn_blocking(move || store.load())
        .await
        .map_err(|_| LumoError::Storage("API storage task failed".to_owned()))?
}

async fn store_state(store: ApiStore, request: PutStateRequest, version: &str) -> Response {
    let etag = state_etag(version, &request.record);
    let result = tokio::task::spawn_blocking(move || {
        store.compare_and_swap(request.expected_revision, &request.record)
    })
    .await
    .map_err(|_| LumoError::Storage("API storage task failed".to_owned()));

    match result {
        Ok(Ok(true)) => with_etag(StatusCode::NO_CONTENT.into_response(), &etag),
        Ok(Ok(false)) => revision_conflict(),
        Ok(Err(error)) => api_error(error),
        Err(error) => api_error(error),
    }
}

fn invalid_body(error: serde_json::Error) -> Response {
    api_error(LumoError::InvalidInput(format!(
        "invalid request body: {error}"
    )))
}

fn revision_conflict() -> Response {
    (
        StatusCode::CONFLICT,
        Json(ApiErrorBody {
            code: "revision_conflict".to_owned(),
            message: "remote state changed; refresh and retry".to_owned(),
        }),
    )
        .into_response()
}

fn state_etag(version: &str, record: &RemoteStateRecord) -> String {
    format!(
        "\"lumo-{version}-{}-{}\"",
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

fn api_error(error: LumoError) -> Response {
    let (status, code) = match &error {
        LumoError::AuthenticationFailed => (StatusCode::UNAUTHORIZED, "authentication_failed"),
        LumoError::ExpiredMessage => (StatusCode::UNAUTHORIZED, "clock_skew"),
        LumoError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        LumoError::ReplayDetected => (StatusCode::CONFLICT, "replay_detected"),
        LumoError::RevisionConflict => (StatusCode::CONFLICT, "revision_conflict"),
        LumoError::InvalidInput(_) => (StatusCode::BAD_REQUEST, "invalid_input"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
        "internal server error".to_owned()
    } else {
        error.to_string()
    };
    (
        status,
        Json(ApiErrorBody {
            code: code.to_owned(),
            message,
        }),
    )
        .into_response()
}
