use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use serde::Serialize;
use subtle::ConstantTimeEq;
use utoipa::OpenApi;
use utoipa_scalar::{Scalar, Servable};

use crate::{
    application::{
        auth::{
            AuthResponse, AuthState, AuthenticatedUser, CsrfResponse, LoginRequest,
            ProfileUpdateRequest, UserResponse, generate_csrf_token, token_from_cookie_header,
        },
        error::AppError,
        health::GetHealth,
    },
    domain::health::HealthStatus,
    ports::inbound::HealthUseCase,
};

const HEALTH_PATH: &str = "/api/v1/health";
const LOGIN_PATH: &str = "/api/v1/auth/login";
const REFRESH_PATH: &str = "/api/v1/auth/refresh";
const LOGOUT_PATH: &str = "/api/v1/auth/logout";
const PROFILE_PATH: &str = "/api/v1/auth/profile";
const CSRF_PATH: &str = "/api/v1/auth/csrf";
const DOCS_PATH: &str = "/api/v1/docs";
const OPENAPI_PATH: &str = "/api/v1/openapi.json";
const DEV_ACCESS_COOKIE: &str = "treefime_access";
const DEV_REFRESH_COOKIE: &str = "treefime_refresh";
const PROD_ACCESS_COOKIE: &str = "__Host-treefime_access";
const PROD_REFRESH_COOKIE: &str = "__Host-treefime_refresh";
const DEV_CSRF_COOKIE: &str = "treefime_csrf";
const PROD_CSRF_COOKIE: &str = "__Host-treefime_csrf";

#[derive(OpenApi)]
#[openapi(
    paths(health, csrf, login, refresh, logout, current_user, update_profile),
    components(schemas(HealthStatus, LoginRequest, ProfileUpdateRequest, AuthResponse, UserResponse, CsrfResponse, ErrorBody)),
    tags((name = "auth", description = "Authentication and profile management"))
)]
struct ApiDoc;

pub fn router(state: AuthState) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(CSRF_PATH, get(csrf))
        .route(LOGIN_PATH, post(login))
        .route(REFRESH_PATH, post(refresh))
        .route(LOGOUT_PATH, post(logout))
        .route("/api/v1/auth/me", get(current_user))
        .route(PROFILE_PATH, patch(update_profile))
        .route(OPENAPI_PATH, get(openapi_json))
        .merge(Scalar::with_url(DOCS_PATH, ApiDoc::openapi()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authorize_request,
        ))
        .with_state(state)
}

#[utoipa::path(get, path = "/api/v1/health", responses((status = 200, description = "API health", body = HealthStatus)))]
async fn health() -> Json<HealthStatus> {
    Json(GetHealth.execute())
}

#[utoipa::path(get, path = "/api/v1/auth/csrf", responses((status = 200, description = "CSRF token for subsequent requests", body = CsrfResponse)))]
async fn csrf(State(state): State<AuthState>, headers: HeaderMap) -> Result<Response, AppError> {
    let cookie_name = csrf_cookie_name(state.secure_cookies);
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    let token = match token_from_cookie_header(cookies, cookie_name) {
        Some(existing) => existing.to_owned(),
        None => generate_csrf_token()?,
    };
    let mut response = Json(CsrfResponse {
        csrf_token: token.clone(),
    })
    .into_response();
    append_csrf_cookie(&mut response, cookie_name, &token, state.secure_cookies);
    Ok(response)
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginRequest,
    responses((status = 200, description = "Signed in; tokens are set as HttpOnly cookies", body = AuthResponse), (status = 401, description = "Invalid credentials", body = ErrorBody))
)]
async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let issued = state.login(payload, user_agent).await?;
    Ok(set_auth_cookies(
        state.secure_cookies,
        issued.access_token,
        issued.refresh_token,
        issued.response,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/refresh",
    responses((status = 200, description = "Refresh token rotated and new cookies issued", body = AuthResponse), (status = 401, description = "Refresh token is invalid, expired, or reused", body = ErrorBody))
)]
async fn refresh(State(state): State<AuthState>, headers: HeaderMap) -> Result<Response, AppError> {
    let (_, refresh_cookie) = cookie_names(state.secure_cookies);
    let csrf_cookie = csrf_cookie_name(state.secure_cookies);
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    let token =
        token_from_cookie_header(cookies, refresh_cookie).ok_or_else(AppError::unauthorized)?;
    let csrf_token = token_from_cookie_header(cookies, csrf_cookie).unwrap_or_default();
    let issued = state.refresh(token, csrf_token).await?;
    Ok(set_auth_cookies(
        state.secure_cookies,
        issued.access_token,
        issued.refresh_token,
        issued.response,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses((status = 204, description = "Current device session revoked"))
)]
async fn logout(State(state): State<AuthState>, headers: HeaderMap) -> Result<Response, AppError> {
    let (access_cookie, refresh_cookie) = cookie_names(state.secure_cookies);
    let csrf_cookie = csrf_cookie_name(state.secure_cookies);
    let cookies = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    let access = token_from_cookie_header(cookies, access_cookie);
    let refresh = token_from_cookie_header(cookies, refresh_cookie);
    let csrf = token_from_cookie_header(cookies, csrf_cookie).ok_or_else(AppError::unauthorized)?;
    state
        .verify_cookie_session_csrf(access, refresh, csrf)
        .await?;
    state.revoke_from_cookies(access, refresh).await?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    clear_cookie(&mut response, access_cookie, state.secure_cookies);
    clear_cookie(&mut response, refresh_cookie, state.secure_cookies);
    clear_cookie(&mut response, csrf_cookie, state.secure_cookies);
    Ok(response)
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses((status = 200, description = "Current authenticated user", body = UserResponse), (status = 401, description = "Authentication required", body = ErrorBody), (status = 403, description = "Root credentials must be changed first", body = ErrorBody))
)]
async fn current_user(Extension(user): Extension<AuthenticatedUser>) -> Json<UserResponse> {
    Json(user.into())
}

#[utoipa::path(
    patch,
    path = "/api/v1/auth/profile",
    request_body = ProfileUpdateRequest,
    responses((status = 200, description = "Profile updated", body = UserResponse), (status = 400, description = "Invalid profile data", body = ErrorBody), (status = 403, description = "Root credentials must be changed first", body = ErrorBody))
)]
async fn update_profile(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<ProfileUpdateRequest>,
) -> Result<Json<UserResponse>, AppError> {
    Ok(Json(state.update_profile(&user, payload).await?))
}

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

async fn authorize_request(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = request.uri().path();
    let method = request.method().clone();
    let csrf_token = if path != REFRESH_PATH
        && matches!(
            method,
            Method::POST | Method::PATCH | Method::PUT | Method::DELETE
        ) {
        let cookies = request
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok());
        let csrf_cookie = token_from_cookie_header(cookies, csrf_cookie_name(state.secure_cookies))
            .ok_or_else(|| AppError::forbidden("CSRF token required."))?;
        let csrf_header = request
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::forbidden("CSRF token required."))?;
        if csrf_cookie
            .as_bytes()
            .ct_eq(csrf_header.as_bytes())
            .unwrap_u8()
            != 1
        {
            return Err(AppError::forbidden("Invalid CSRF token."));
        }
        Some(csrf_header.to_owned())
    } else {
        None
    };
    if is_public(path, &method) {
        return Ok(next.run(request).await);
    }

    let (access_cookie, _) = cookie_names(state.secure_cookies);
    let token = token_from_cookie_header(
        request
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok()),
        access_cookie,
    )
    .ok_or_else(AppError::unauthorized)?;
    let user = state.authenticate(token).await?;
    if let (Some(session_id), Some(csrf_token)) = (user.session_id, csrf_token.as_deref()) {
        state.verify_session_csrf(session_id, csrf_token).await?;
    }
    if user.must_update_credentials && !(path == PROFILE_PATH && method == Method::PATCH) {
        return Err(AppError::forbidden(
            "The first-run root account must update its email and password before using the API.",
        ));
    }
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

fn is_public(path: &str, method: &Method) -> bool {
    path == HEALTH_PATH
        || path == CSRF_PATH
        || path == DOCS_PATH
        || path == OPENAPI_PATH
        || (method == Method::POST && matches!(path, LOGIN_PATH | REFRESH_PATH | LOGOUT_PATH))
}

fn cookie_names(secure: bool) -> (&'static str, &'static str) {
    if secure {
        (PROD_ACCESS_COOKIE, PROD_REFRESH_COOKIE)
    } else {
        (DEV_ACCESS_COOKIE, DEV_REFRESH_COOKIE)
    }
}

fn csrf_cookie_name(secure: bool) -> &'static str {
    if secure {
        PROD_CSRF_COOKIE
    } else {
        DEV_CSRF_COOKIE
    }
}

fn set_auth_cookies(
    secure: bool,
    access: String,
    refresh: String,
    response_body: AuthResponse,
) -> Response {
    let (access_cookie, refresh_cookie) = cookie_names(secure);
    let csrf_cookie = csrf_cookie_name(secure);
    let csrf_token = response_body.csrf_token.clone();
    let mut response = Json(response_body).into_response();
    append_cookie(&mut response, access_cookie, &access, 15 * 60, secure);
    append_cookie(
        &mut response,
        refresh_cookie,
        &refresh,
        30 * 24 * 60 * 60,
        secure,
    );
    append_csrf_cookie(&mut response, csrf_cookie, &csrf_token, secure);
    response
}

fn append_csrf_cookie(response: &mut Response, name: &str, token: &str, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!("{name}={token}; Path=/; SameSite=Strict; Max-Age=2592000{secure_flag}");
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn append_cookie(response: &mut Response, name: &str, value: &str, max_age: u64, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie =
        format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure_flag}");
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn clear_cookie(response: &mut Response, name: &str, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}");
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

#[derive(Serialize, utoipa::ToSchema)]
struct ErrorBody {
    error: &'static str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Authentication required or credentials are invalid.",
            ),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            AppError::Conflict(message) => (StatusCode::CONFLICT, message),
            AppError::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "The request could not be completed.",
            ),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
