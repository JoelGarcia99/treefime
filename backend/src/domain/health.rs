#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct HealthStatus {
    pub status: &'static str,
}

impl HealthStatus {
    pub fn healthy() -> Self {
        Self { status: "ok" }
    }
}
