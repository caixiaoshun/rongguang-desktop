//! 顶层 Router：前端静态资源 + `/media` 媒体文件 + `/api` 业务接口。

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use axum::routing::get;
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::routes;
use crate::state::AppState;

pub fn build(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
            Method::PATCH,
        ])
        .allow_headers(Any)
        .expose_headers([
            header::CONTENT_TYPE,
            header::CONTENT_LENGTH,
            header::CONTENT_DISPOSITION,
        ])
        .max_age(std::time::Duration::from_secs(600));

    let media_dir = state.paths.media_dir.clone();
    let upload_dir = state.paths.upload_dir.clone();
    let web_dir = state.paths.web_dir.clone();
    let index = web_dir.join("index.html");

    let api = routes::all().with_state(state);

    let mut app = Router::new()
        .route("/api/health", get(health))
        .merge(api)
        .nest_service("/media", ServeDir::new(media_dir))
        .nest_service("/uploads", ServeDir::new(upload_dir));

    // 前端静态资源；单页应用回退到 index.html。
    if index.exists() {
        app = app.fallback_service(
            ServeDir::new(&web_dir).not_found_service(ServeFile::new(&index)),
        );
    } else {
        app = app.fallback(|| async {
            (
                [(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"))],
                "前端资源未就绪：请先执行前端构建（out/ 目录），或将 web/ 放到可执行文件同级。",
            )
        });
    }

    app.layer(axum::extract::DefaultBodyLimit::max(200 * 1024 * 1024))
        .layer(TraceLayer::new_for_http()).layer(cors)
}

async fn health() -> &'static str {
    "ok"
}
