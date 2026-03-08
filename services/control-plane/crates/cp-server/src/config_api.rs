use axum::{
    Json, Router,
    extract::{Query, State},
    routing::{get, post, put},
};
use cp_api_types::{
    ActionResponse, BypassAddRequest, BypassListResponse, ConfigResponse, LevelRequest,
    SecurityConfigResponse,
};
use serde::Deserialize;

use crate::{
    BroadcastMessage, HttpState,
    auth::{self, Permission},
    error::AppResult,
    state::RuntimeConfigStore,
};

#[derive(Debug, Clone, Deserialize)]
pub struct DeleteBypassQuery {
    pub domain: String,
}

/// Return the full runtime configuration snapshot.
///
/// # Errors
///
/// Returns an error when configuration data cannot be loaded.
pub async fn config<S>(State(state): State<HttpState<S>>) -> AppResult<Json<ConfigResponse>>
where
    S: RuntimeConfigStore,
{
    Ok(Json(state.store.get_config().await?))
}

/// Return the mutable security section.
///
/// # Errors
///
/// Returns an error when security configuration cannot be loaded.
pub async fn security<S>(
    State(state): State<HttpState<S>>,
) -> AppResult<Json<SecurityConfigResponse>>
where
    S: RuntimeConfigStore,
{
    Ok(Json(state.store.get_security_config().await?))
}

/// Update the runtime security level through the Phase 2 config API.
///
/// # Errors
///
/// Returns an error when validation fails or the underlying update cannot be applied.
pub async fn update_security<S>(
    State(state): State<HttpState<S>>,
    Json(request): Json<LevelRequest>,
) -> AppResult<Json<ActionResponse>>
where
    S: RuntimeConfigStore,
{
    let response = state
        .store
        .set_security_level_via_config(&request.level)
        .await?;
    state.notify(BroadcastMessage::Config(cp_api_types::ConfigEvent {
        event_type: "level_changed".to_string(),
        level: Some(request.level.to_ascii_lowercase()),
        domain: None,
    }));
    state.notify(BroadcastMessage::Full);
    Ok(Json(response))
}

/// Return compiled and runtime bypass domains.
///
/// # Errors
///
/// Returns an error when bypass domains cannot be loaded.
pub async fn list_bypass<S>(
    State(state): State<HttpState<S>>,
) -> AppResult<Json<BypassListResponse>>
where
    S: RuntimeConfigStore,
{
    Ok(Json(state.store.list_bypass_domains().await?))
}

/// Add a runtime bypass domain.
///
/// # Errors
///
/// Returns an error when validation fails or the change cannot be persisted.
pub async fn add_bypass<S>(
    State(state): State<HttpState<S>>,
    Json(request): Json<BypassAddRequest>,
) -> AppResult<Json<ActionResponse>>
where
    S: RuntimeConfigStore,
{
    let normalized = state.store.normalize_bypass_domain(&request.domain)?;
    let response = state.store.add_bypass_domain(&request.domain).await?;
    state.notify(crate::BroadcastMessage::Config(cp_api_types::ConfigEvent {
        event_type: "bypass_added".to_string(),
        level: None,
        domain: Some(state.store.display_bypass_domain(&normalized)),
    }));
    Ok(Json(response))
}

/// Remove a runtime bypass domain.
///
/// # Errors
///
/// Returns an error when validation fails or the change cannot be persisted.
pub async fn delete_bypass<S>(
    State(state): State<HttpState<S>>,
    Query(query): Query<DeleteBypassQuery>,
) -> AppResult<Json<ActionResponse>>
where
    S: RuntimeConfigStore,
{
    let normalized = state.store.normalize_bypass_domain(&query.domain)?;
    let response = state.store.delete_bypass_domain(&query.domain).await?;
    state.notify(BroadcastMessage::Config(cp_api_types::ConfigEvent {
        event_type: "bypass_removed".to_string(),
        level: None,
        domain: Some(state.store.display_bypass_domain(&normalized)),
    }));
    Ok(Json(response))
}

pub fn routes<S>() -> Router<HttpState<S>>
where
    S: RuntimeConfigStore + Clone + Send + Sync + 'static,
{
    let read_routes = Router::new()
        .route(
            "/config",
            get(config::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/config/security",
            get(security::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        )
        .route(
            "/config/bypass",
            get(list_bypass::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::ReadDashboard)
            })),
        );

    let mutate_routes = Router::new()
        .route(
            "/config/security",
            put(update_security::<S>).route_layer(axum::middleware::from_fn(|request, next| {
                auth::require_permission(request, next, Permission::MutateConfig)
            })),
        )
        .route(
            "/config/bypass",
            post(add_bypass::<S>)
                .delete(delete_bypass::<S>)
                .route_layer(axum::middleware::from_fn(|request, next| {
                    auth::require_permission(request, next, Permission::MutateConfig)
                })),
        );

    read_routes.merge(mutate_routes)
}
