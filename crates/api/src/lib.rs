//! HTTP API 层：axum 双 router（chat :8080 / admin :8081）+ handler + 中间件。

pub mod state;
pub mod error;
pub mod tun_trait;
pub mod middleware;
pub mod handlers;
pub mod routes;

pub use state::AppState;
pub use error::{ApiError, ApiResult};
pub use tun_trait::{TunControl, TunStatus};
pub use routes::{chat_router, admin_router, health_router};
