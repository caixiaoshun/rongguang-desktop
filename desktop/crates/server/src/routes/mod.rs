//! 路由模块汇总。每个子模块对外暴露 `routes() -> Router<Arc<AppState>>`。

pub mod agent_config;
pub mod ai;
pub mod asset;
pub mod assistant;
pub mod auth;
pub mod comfyui;
pub mod generation;
pub mod project;
pub mod script;
pub mod storage;
pub mod storyboard;
pub mod system;
pub mod tasks;
pub mod team;
pub mod tools;
pub mod update;

use std::sync::Arc;

use axum::Router;

use crate::state::AppState;

/// 把所有业务路由合并成一个 Router。
pub fn all() -> Router<Arc<AppState>> {
    Router::new()
        .merge(system::routes())
        .merge(agent_config::routes())
        .merge(comfyui::routes())
        .merge(auth::routes())
        .merge(team::routes())
        .merge(project::routes())
        .merge(script::routes())
        .merge(storyboard::routes())
        .merge(asset::routes())
        .merge(ai::routes())
        .merge(generation::routes())
        .merge(storage::routes())
        .merge(tasks::routes())
        .merge(update::routes())
        .merge(assistant::routes())
}
