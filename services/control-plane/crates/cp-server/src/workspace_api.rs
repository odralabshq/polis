use axum::{Json, extract::State};
use cp_api_types::{AgentResponse, ContainersResponse, WorkspaceResponse};

use crate::{HttpState, error::AppResult, state::WorkspaceStore};

/// Return the current workspace snapshot.
///
/// # Errors
///
/// Returns an error when the workspace state cannot be loaded.
pub async fn workspace<S>(State(state): State<HttpState<S>>) -> AppResult<Json<WorkspaceResponse>>
where
    S: WorkspaceStore,
{
    Ok(Json(state.store.get_workspace().await?))
}

/// Return the active agent snapshot.
///
/// # Errors
///
/// Returns an error when the agent state cannot be loaded.
pub async fn agent<S>(State(state): State<HttpState<S>>) -> AppResult<Json<AgentResponse>>
where
    S: WorkspaceStore,
{
    Ok(Json(state.store.get_agent().await?))
}

/// Return the detailed container list for the workspace.
///
/// # Errors
///
/// Returns an error when container details cannot be loaded.
pub async fn containers<S>(State(state): State<HttpState<S>>) -> AppResult<Json<ContainersResponse>>
where
    S: WorkspaceStore,
{
    Ok(Json(state.store.list_containers().await?))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{collections::HashMap, sync::Arc};

    use async_trait::async_trait;
    use axum::{Router, body::Body, http::Request, routing::get};
    use cp_api_types::{ContainerInfo, ContainerSummary, ResourceUsage};
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    use super::*;
    use crate::state::WorkspaceStore;

    #[derive(Clone)]
    struct TestStore;

    #[async_trait]
    impl WorkspaceStore for TestStore {
        async fn get_workspace(&self) -> AppResult<WorkspaceResponse> {
            Ok(WorkspaceResponse {
                status: "running".to_string(),
                uptime_seconds: Some(60),
                containers: ContainerSummary {
                    total: 2,
                    healthy: 2,
                    unhealthy: 0,
                    starting: 0,
                },
                networks: HashMap::from([(
                    "gateway-bridge".to_string(),
                    "10.20.0.0/24".to_string(),
                )]),
            })
        }

        async fn get_agent(&self) -> AppResult<AgentResponse> {
            Ok(AgentResponse {
                name: "openclaw".to_string(),
                display_name: "OpenClaw".to_string(),
                version: "1.0.0".to_string(),
                status: "running".to_string(),
                health: "healthy".to_string(),
                uptime_seconds: Some(60),
                ports: Vec::new(),
                resources: ResourceUsage {
                    memory_usage_mb: 256,
                    memory_limit_mb: 512,
                    cpu_percent: 10.0,
                },
                stale: false,
            })
        }

        async fn list_containers(&self) -> AppResult<ContainersResponse> {
            Ok(ContainersResponse {
                containers: vec![ContainerInfo {
                    name: "polis-workspace".to_string(),
                    service: "workspace".to_string(),
                    status: "running".to_string(),
                    health: "healthy".to_string(),
                    uptime_seconds: Some(60),
                    memory_usage_mb: 256,
                    memory_limit_mb: 512,
                    cpu_percent: 10.0,
                    network: "internal-bridge".to_string(),
                    ip: "10.30.0.10".to_string(),
                    stale: false,
                }],
            })
        }
    }

    #[tokio::test]
    async fn workspace_handler_returns_snapshot() {
        let app = test_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/workspace")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn agent_handler_returns_snapshot() {
        let app = test_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn containers_handler_returns_snapshot() {
        let app = test_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/containers")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    fn test_router() -> Router {
        let (sender, _) = broadcast::channel(4);
        let state = HttpState::new(Arc::new(TestStore), sender);

        Router::new()
            .route("/workspace", get(workspace::<TestStore>))
            .route("/agent", get(agent::<TestStore>))
            .route("/containers", get(containers::<TestStore>))
            .with_state(state)
    }
}
