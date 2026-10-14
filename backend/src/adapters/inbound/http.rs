use axum::{
    Extension, Json, Router,
    extract::{Path, Request, State},
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
        workspaces::{
            self, AcceptInvitationRequest, CreateInvitationRequest, CreateWorkspaceRequest,
            UpdateWorkspaceRequest,
        },
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
const WORKSPACES_PATH: &str = "/api/v1/workspaces";
const INVITATIONS_PATH: &str = "/api/v1/invitations/{token}/accept";
const DEV_ACCESS_COOKIE: &str = "treefime_access";
const DEV_REFRESH_COOKIE: &str = "treefime_refresh";
const PROD_ACCESS_COOKIE: &str = "__Host-treefime_access";
const PROD_REFRESH_COOKIE: &str = "__Host-treefime_refresh";
const DEV_CSRF_COOKIE: &str = "treefime_csrf";
const PROD_CSRF_COOKIE: &str = "__Host-treefime_csrf";

#[derive(OpenApi)]
#[openapi(
    paths(health, csrf, login, refresh, logout, current_user, update_profile, list_workspaces, create_workspace, update_workspace, list_workspace_members, create_workspace_invitation, revoke_workspace_invitation, remove_workspace_member, accept_workspace_invitation),
    components(schemas(HealthStatus, LoginRequest, ProfileUpdateRequest, AuthResponse, UserResponse, CsrfResponse, ErrorBody, workspaces::CreateWorkspaceRequest, workspaces::UpdateWorkspaceRequest, workspaces::CreateInvitationRequest, workspaces::AcceptInvitationRequest, workspaces::WorkspaceSummary, workspaces::CreatedWorkspace, workspaces::CreatedInvitation, workspaces::WorkspaceMember, workspaces::InvitationAccepted)),
    tags((name = "auth", description = "Authentication and profile management"), (name = "workspaces", description = "Workspace administration and membership"))
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
        .route(WORKSPACES_PATH, get(list_workspaces).post(create_workspace))
        .route("/api/v1/workspaces/{workspace_id}", patch(update_workspace))
        .route(
            "/api/v1/workspaces/{workspace_id}/members",
            get(list_workspace_members),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/members/{user_id}",
            axum::routing::delete(remove_workspace_member),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/invitations",
            post(create_workspace_invitation),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}",
            axum::routing::delete(revoke_workspace_invitation),
        )
        .route(INVITATIONS_PATH, post(accept_workspace_invitation))
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

#[utoipa::path(get, path = "/api/v1/workspaces", responses((status = 200, body = [workspaces::WorkspaceSummary]), (status = 403, description = "Workspace metadata permission required")), tag = "workspaces")]
async fn list_workspaces(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Vec<workspaces::WorkspaceSummary>>, AppError> {
    Ok(Json(
        workspaces::list_workspaces(&state.database, &user).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/workspaces", request_body = CreateWorkspaceRequest, responses((status = 201, body = workspaces::CreatedWorkspace), (status = 403, description = "Workspace creation permission required")), tag = "workspaces")]
async fn create_workspace(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<workspaces::CreatedWorkspace>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(workspaces::create_workspace(&state.database, &user, payload).await?),
    ))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}", params(("workspace_id" = Uuid, Path)), request_body = UpdateWorkspaceRequest, responses((status = 200, body = workspaces::WorkspaceSummary), (status = 403)), tag = "workspaces")]
async fn update_workspace(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<uuid::Uuid>,
    Json(payload): Json<UpdateWorkspaceRequest>,
) -> Result<Json<workspaces::WorkspaceSummary>, AppError> {
    Ok(Json(
        workspaces::update_workspace(&state.database, &user, workspace_id, payload).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/members", params(("workspace_id" = Uuid, Path)), responses((status = 200, body = [workspaces::WorkspaceMember]), (status = 403)), tag = "workspaces")]
async fn list_workspace_members(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<uuid::Uuid>,
) -> Result<Json<Vec<workspaces::WorkspaceMember>>, AppError> {
    Ok(Json(
        workspaces::list_members(&state.database, &user, workspace_id).await?,
    ))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/invitations", params(("workspace_id" = Uuid, Path)), request_body = CreateInvitationRequest, responses((status = 201, body = workspaces::CreatedInvitation), (status = 403)), tag = "workspaces")]
async fn create_workspace_invitation(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(workspace_id): Path<uuid::Uuid>,
    Json(payload): Json<CreateInvitationRequest>,
) -> Result<(StatusCode, Json<workspaces::CreatedInvitation>), AppError> {
    Ok((
        StatusCode::CREATED,
        Json(workspaces::invite_member(&state.database, &user, workspace_id, payload).await?),
    ))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/invitations/{invitation_id}", params(("workspace_id" = Uuid, Path), ("invitation_id" = Uuid, Path)), responses((status = 204), (status = 403)), tag = "workspaces")]
async fn revoke_workspace_invitation(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((workspace_id, invitation_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, AppError> {
    workspaces::revoke_invitation(&state.database, &user, workspace_id, invitation_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/members/{user_id}", params(("workspace_id" = Uuid, Path), ("user_id" = Uuid, Path)), responses((status = 204), (status = 403), (status = 409)), tag = "workspaces")]
async fn remove_workspace_member(
    State(state): State<AuthState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((workspace_id, user_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, AppError> {
    workspaces::remove_member(&state.database, &user, workspace_id, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/invitations/{token}/accept", params(("token" = String, Path)), request_body = AcceptInvitationRequest, responses((status = 200, body = workspaces::InvitationAccepted), (status = 401, description = "Invitation invalid, expired, or account password incorrect")), tag = "workspaces")]
async fn accept_workspace_invitation(
    State(state): State<AuthState>,
    Path(token): Path<String>,
    Json(payload): Json<AcceptInvitationRequest>,
) -> Result<Json<workspaces::InvitationAccepted>, AppError> {
    Ok(Json(
        workspaces::accept_invitation(&state.database, &token, payload).await?,
    ))
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
        || (method == Method::POST
            && path.starts_with("/api/v1/invitations/")
            && path.ends_with("/accept"))
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
