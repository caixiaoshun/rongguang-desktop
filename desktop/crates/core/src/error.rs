//! 统一错误模型。
//!
//! 与 Java 版 `CommonResult` 保持一致：业务错误通过 200 状态码 + `code != 0` 表达，
//! 只有鉴权失败等少数场景返回 HTTP 401。

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Business(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("网络错误: {0}")]
    Http(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn business(msg: impl Into<String>) -> Self {
        Self::Business(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// 业务错误码。0 表示成功，与前端 `CommonResult.code` 判定保持一致。
    pub fn code(&self) -> i32 {
        match self {
            AppError::Business(_) | AppError::BadRequest(_) => 400,
            AppError::NotFound(_) => 404,
            AppError::Unauthorized(_) => 401,
            AppError::Forbidden(_) => 403,
            AppError::Db(_) | AppError::Io(_) | AppError::Json(_) | AppError::Internal(_) => 500,
            AppError::Http(_) => 502,
        }
    }

    /// 鉴权/权限错误使用真实 HTTP 状态码，其余走 200 + code。
    pub fn status(&self) -> u16 {
        match self {
            AppError::Unauthorized(_) => 401,
            AppError::Forbidden(_) => 403,
            _ => 200,
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        AppError::Internal(value.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// 统一响应包装：`{ code, msg, data }`。
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub msg: String,
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 0,
            msg: "success".into(),
            data: Some(data),
        }
    }
}

impl ApiResponse<()> {
    pub fn ok_empty() -> Self {
        Self {
            code: 0,
            msg: "success".into(),
            data: None,
        }
    }
}

/// 直接把 `AppError` 变成 HTTP 响应：鉴权类错误用真实状态码，业务错误统一 200。
impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let status = StatusCode::from_u16(self.status()).unwrap_or(StatusCode::OK);
        if status == StatusCode::OK {
            tracing::debug!("业务错误: {self}");
        } else {
            tracing::warn!("请求失败: {self}");
        }
        let body = ApiResponse::<()> {
            code: self.code(),
            msg: self.to_string(),
            data: None,
        };
        (status, axum::Json(body)).into_response()
    }
}
