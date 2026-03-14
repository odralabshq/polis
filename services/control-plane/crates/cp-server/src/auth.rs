//! Authentication middleware and RBAC helpers.

#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

use axum::{
    Json,
    body::Body,
    extract::State,
    http::{
        Request, StatusCode,
        header::{AUTHORIZATION, HeaderMap},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use cp_api_types::ErrorResponse;
use serde::Serialize;
use std::str::FromStr;

use crate::{HttpState, state::AuthStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Viewer,
    Agent,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
            Self::Agent => "agent",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "admin" => Some(Self::Admin),
            "operator" => Some(Self::Operator),
            "viewer" => Some(Self::Viewer),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    #[must_use]
    pub fn allows(self, permission: Permission) -> bool {
        match self {
            Self::Admin => true,
            Self::Operator => matches!(
                permission,
                Permission::ReadDashboard
                    | Permission::ReadBlocked
                    | Permission::ReadLevel
                    | Permission::MutateGovernance
            ),
            Self::Viewer => matches!(
                permission,
                Permission::ReadDashboard | Permission::ReadBlocked | Permission::ReadLevel
            ),
            Self::Agent => matches!(permission, Permission::ReadBlocked | Permission::ReadLevel),
        }
    }
}

impl FromStr for Role {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ReadDashboard,
    ReadBlocked,
    ReadLevel,
    MutateGovernance,
    MutateConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthSession {
    role: Role,
}

impl AuthSession {
    #[must_use]
    pub fn new(role: Role) -> Self {
        Self { role }
    }

    #[must_use]
    pub fn role(self) -> Role {
        self.role
    }
}

/// Authentication middleware that validates Bearer tokens from the
/// `Authorization` header or `?token=` query parameter.
///
/// # CSRF Resistance
///
/// This middleware intentionally uses Bearer tokens (not cookies) for
/// authentication. Bearer tokens in `Authorization` headers are not
/// automatically attached by browsers, making the API inherently resistant
/// to CSRF attacks. If cookie-based authentication is ever added, explicit
/// CSRF token validation must be implemented.
pub async fn auth_middleware<S>(
    State(state): State<HttpState<S>>,
    mut request: Request<Body>,
    next: Next,
) -> Response
where
    S: AuthStore,
{
    if !state.store.auth_enabled() {
        request
            .extensions_mut()
            .insert(AuthSession::new(Role::Admin));
        return next.run(request).await;
    }

    let client_id = client_id(request.headers());
    let token = bearer_token(request.headers()).or_else(|| query_token(request.uri().query()));
    let Some(token) = token else {
        return auth_failure_response(
            state.store.as_ref(),
            &client_id,
            "missing authentication token",
        )
        .await;
    };

    match state.store.validate_token(&token).await {
        Ok(role) => {
            request.extensions_mut().insert(AuthSession::new(role));
            next.run(request).await
        }
        Err(_) => {
            auth_failure_response(
                state.store.as_ref(),
                &client_id,
                "invalid authentication token",
            )
            .await
        }
    }
}

pub async fn require_permission(
    request: Request<Body>,
    next: Next,
    permission: Permission,
) -> Response {
    let role = request
        .extensions()
        .get::<AuthSession>()
        .copied()
        .unwrap_or_else(|| AuthSession::new(Role::Admin))
        .role();

    if role.allows(permission) {
        next.run(request).await
    } else {
        json_error(
            StatusCode::FORBIDDEN,
            "authenticated role does not have permission for this endpoint",
        )
    }
}

async fn auth_failure_response<S>(store: &S, client_id: &str, reason: &str) -> Response
where
    S: AuthStore,
{
    let rate_limited = store
        .register_auth_failure(client_id, reason)
        .await
        .unwrap_or(false);
    let status = if rate_limited {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::UNAUTHORIZED
    };
    let message = if rate_limited {
        "too many failed authentication attempts"
    } else {
        reason
    };
    json_error(status, message)
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
        .into_response()
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(ToString::to_string)
}

/// Extract a token from the `?token=` query parameter.
///
/// # Security Note
///
/// Query-string tokens are required for `EventSource` (SSE) connections
/// which do not support custom headers. The dashboard JavaScript strips
/// the token from the URL bar via `history.replaceState` immediately
/// after reading it to prevent leaking in browser history and Referer
/// headers. Server-side access logs should avoid recording full query
/// strings when auth is enabled.
fn query_token(query: Option<&str>) -> Option<String> {
    query.and_then(|query| {
        query
            .split('&')
            .find_map(|segment| segment.strip_prefix("token=").map(ToString::to_string))
    })
}

fn client_id(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .map_or_else(|| "127.0.0.1".to_string(), ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{Permission, Role, bearer_token, query_token};

    #[test]
    fn role_permissions_match_design() {
        assert!(Role::Admin.allows(Permission::MutateConfig));
        assert!(Role::Operator.allows(Permission::MutateGovernance));
        assert!(!Role::Operator.allows(Permission::MutateConfig));
        assert!(Role::Viewer.allows(Permission::ReadDashboard));
        assert!(!Role::Viewer.allows(Permission::MutateGovernance));
        assert!(Role::Agent.allows(Permission::ReadBlocked));
        assert!(Role::Agent.allows(Permission::ReadLevel));
        assert!(!Role::Agent.allows(Permission::ReadDashboard));
    }

    #[test]
    fn extracts_tokens_from_header_and_query() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer polis_admin_deadbeef".parse().expect("header"),
        );

        assert_eq!(
            bearer_token(&headers).as_deref(),
            Some("polis_admin_deadbeef")
        );
        assert_eq!(
            query_token(Some("foo=bar&token=polis_viewer_abc")),
            Some("polis_viewer_abc".to_string())
        );
    }
}
