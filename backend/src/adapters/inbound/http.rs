use axum::{Json, Router, routing::get};

use crate::{application::health::GetHealth, ports::inbound::HealthUseCase};

pub fn router() -> Router {
    Router::new().route("/api/v1/health", get(health))
}

async fn health() -> Json<crate::domain::health::HealthStatus> {
    Json(GetHealth.execute())
}
