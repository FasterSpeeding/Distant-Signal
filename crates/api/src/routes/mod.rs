use crate::app::Router;

pub mod health;

pub fn public_router() -> Router {
    Router::new().nest("/health", health::router())
}

pub fn private_router() -> Router {
    Router::new()
}
