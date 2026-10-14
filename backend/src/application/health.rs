use crate::{domain::health::HealthStatus, ports::inbound::HealthUseCase};

pub struct GetHealth;

impl HealthUseCase for GetHealth {
    fn execute(&self) -> HealthStatus {
        HealthStatus::healthy()
    }
}
