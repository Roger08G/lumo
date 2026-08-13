use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use lumo_core::LumoError;
use lumo_protocol::{ApiErrorBody, HealthResponse, PutStateRequest, API_VERSION};

use crate::{
    auth::{authenticate, system_now_ms},
    ApiState,
};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
        api_version: API_VERSION.to_owned(),
    })
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
    match state.store.load() {
        Ok(Some(record)) => Json(record).into_response(),
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
    let request: PutStateRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return api_error(LumoError::InvalidInput(format!(
                "invalid request body: {error}"
            )))
        }
    };
    match state
        .store
        .compare_and_swap(request.expected_revision, &request.record)
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(ApiErrorBody {
                code: "revision_conflict".to_owned(),
                message: "remote state changed; refresh and retry".to_owned(),
            }),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

fn api_error(error: LumoError) -> Response {
    let (status, code) = match &error {
        LumoError::AuthenticationFailed | LumoError::ExpiredMessage => {
            (StatusCode::UNAUTHORIZED, "authentication_failed")
        }
        LumoError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
        LumoError::ReplayDetected => (StatusCode::CONFLICT, "replay_detected"),
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
