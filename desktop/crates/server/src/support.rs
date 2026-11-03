//! 通用响应、错误映射与数据库小工具。

pub mod ffmpeg;

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use axum::Json;
use serde::Serialize;

use fusion_core::error::{ApiResponse, AppError};
use fusion_core::time;

pub type ApiResult<T> = Result<Json<ApiResponse<T>>, AppError>;

pub fn ok<T: Serialize>(value: T) -> ApiResult<T> {
    Ok(Json(ApiResponse::ok(value)))
}

pub fn ok_true() -> ApiResult<bool> {
    Ok(Json(ApiResponse::ok(true)))
}


pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::internal(format!("密码加密失败: {e}")))
}

pub fn verify_password(password: &str, hashed: &str) -> bool {
    match PasswordHash::new(hashed) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// 把前端传入的 JSON 值统一转成数据库 TEXT。
pub fn to_json_text(value: &Option<serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::Null) | None => None,
        Some(v) => Some(v.to_string()),
    }
}

/// 接受「已是 JSON 字符串」或「结构化对象」两种入参。
pub fn to_json_text_from_str(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn now() -> String {
    time::now()
}

pub fn new_task_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// 解析 JSON 数组里的字符串列表。
pub fn parse_string_list(value: &Option<String>) -> Vec<String> {
    match value {
        Some(text) if !text.trim().is_empty() => {
            serde_json::from_str::<Vec<String>>(text).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// 过滤掉 `None`，用于动态 UPDATE 语句拼接。
pub struct UpdateBuilder {
    sets: Vec<String>,
    pub values: Vec<SqlValue>,
}

pub enum SqlValue {
    Text(Option<String>),
    Int(Option<i64>),
    Real(Option<f64>),
    Bool(Option<bool>),
}

impl UpdateBuilder {
    pub fn new() -> Self {
        Self {
            sets: Vec::new(),
            values: Vec::new(),
        }
    }

    pub fn text(&mut self, column: &str, value: &Option<String>) {
        if let Some(v) = value {
            self.sets.push(format!("{column} = ?"));
            self.values.push(SqlValue::Text(Some(v.clone())));
        }
    }

    pub fn text_allow_null(&mut self, column: &str, value: &Option<String>) {
        if let Some(v) = value {
            self.sets.push(format!("{column} = ?"));
            self.values.push(SqlValue::Text(Some(v.clone())));
        }
    }

    pub fn int(&mut self, column: &str, value: &Option<i64>) {
        if let Some(v) = value {
            self.sets.push(format!("{column} = ?"));
            self.values.push(SqlValue::Int(Some(*v)));
        }
    }

    pub fn real(&mut self, column: &str, value: &Option<f64>) {
        if let Some(v) = value {
            self.sets.push(format!("{column} = ?"));
            self.values.push(SqlValue::Real(Some(*v)));
        }
    }

    pub fn boolean(&mut self, column: &str, value: &Option<bool>) {
        if let Some(v) = value {
            self.sets.push(format!("{column} = ?"));
            self.values.push(SqlValue::Bool(Some(*v)));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// 追加 `update_time` 并返回 SET 子句。
    pub fn finish(mut self) -> (String, Vec<SqlValue>) {
        self.sets.push("update_time = ?".into());
        self.values.push(SqlValue::Text(Some(time::now())));
        (self.sets.join(", "), self.values)
    }
}

pub fn bind_values<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    values: &'q [SqlValue],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for value in values {
        query = match value {
            SqlValue::Text(v) => query.bind(v.clone()),
            SqlValue::Int(v) => query.bind(v.clone()),
            SqlValue::Real(v) => query.bind(v.clone()),
            SqlValue::Bool(v) => query.bind(v.clone()),
        };
    }
    query
}
