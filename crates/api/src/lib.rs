//! HTTP API 层：axum 双 router（chat :8080 / admin :8081）+ handler + 中间件。

pub mod error;
pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod state;
pub mod tun_trait;

pub use error::{ApiError, ApiResult};
pub use routes::{admin_router, chat_router, health_router};
pub use state::AppState;
pub use tun_trait::{TunControl, TunStatus};
