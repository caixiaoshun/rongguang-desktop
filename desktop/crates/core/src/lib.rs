//! 融光桌面端核心层：错误模型、配置、数据库与领域实体。

pub mod config;
pub mod db;
pub mod error;
pub mod json;
pub mod models;
pub mod paths;
pub mod time;

pub use error::{AppError, AppResult};
