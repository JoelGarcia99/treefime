#[derive(Debug)]
pub enum AppError {
    BadRequest(&'static str),
    Unauthorized,
    Forbidden(&'static str),
    Conflict(&'static str),
    Internal,
}

impl AppError {
    pub fn bad_request(message: &'static str) -> Self {
        Self::BadRequest(message)
    }

    pub fn unauthorized() -> Self {
        Self::Unauthorized
    }

    pub fn forbidden(message: &'static str) -> Self {
        Self::Forbidden(message)
    }

    pub fn conflict(message: &'static str) -> Self {
        Self::Conflict(message)
    }

    pub fn internal() -> Self {
        Self::Internal
    }

    pub fn database<E>(_error: E) -> Self {
        Self::Internal
    }
}
