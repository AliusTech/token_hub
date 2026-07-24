//! 系统接口（健康检查）。

use axum::response::IntoResponse;

pub async fn health() -> impl IntoResponse {
    "ok"
}
