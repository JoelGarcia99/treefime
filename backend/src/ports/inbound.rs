use crate::domain::health::HealthStatus;

pub trait HealthUseCase {
    fn execute(&self) -> HealthStatus;
}
