//! 鉴权：本地单用户场景下的 JWT 会话。
//!
//! 与 Java 版保持契约：`Authorization: Bearer <token>`，刷新令牌同构。
//! 单机版不引入多设备吊销复杂度，令牌内只放用户标识与过期时间。

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use fusion_core::error::AppError;
use fusion_core::models::system::LoginResp;

use crate::state::AppState;

pub const ACCESS_TTL_SECONDS: i64 = 7 * 24 * 3600;
pub const REFRESH_TTL_SECONDS: i64 = 30 * 24 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub username: String,
    pub exp: i64,
    pub iat: i64,
    /// `access` / `refresh`
    pub typ: String,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

impl AuthUser {
    pub fn anonymous() -> Self {
        Self {
            id: 1,
            username: "local".into(),
        }
    }
}

pub fn issue_tokens(
    secret: &str,
    user_id: i64,
    username: &str,
    nickname: &str,
) -> Result<LoginResp, AppError> {
    let now = Utc::now();
    let access = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(ACCESS_TTL_SECONDS)).timestamp(),
        typ: "access".into(),
    };
    let refresh = Claims {
        sub: user_id,
        username: username.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(REFRESH_TTL_SECONDS)).timestamp(),
        typ: "refresh".into(),
    };
    let key = EncodingKey::from_secret(secret.as_bytes());
    let access_token = encode(&Header::new(Algorithm::HS256), &access, &key)
        .map_err(|e| AppError::internal(format!("签发令牌失败: {e}")))?;
    let refresh_token = encode(&Header::new(Algorithm::HS256), &refresh, &key)
        .map_err(|e| AppError::internal(format!("签发令牌失败: {e}")))?;

    Ok(LoginResp {
        access_token,
        refresh_token,
        expires_in: ACCESS_TTL_SECONDS,
        user_id,
        username: username.to_string(),
        nickname: nickname.to_string(),
    })
}

pub fn verify(secret: &str, token: &str, expect: &str) -> Result<Claims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Unauthorized("登录状态已失效，请重新登录".into()))?;
    if data.claims.typ != expect {
        return Err(AppError::Unauthorized("令牌类型不匹配".into()));
    }
    Ok(data.claims)
}

/// 从请求头或查询串提取令牌。
fn extract_token(parts: &Parts) -> Option<String> {
    if let Some(value) = parts.headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(text) = value.to_str() {
            if let Some(rest) = text.strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
    }
    parts.uri.query().and_then(|q| {
        q.split('&')
            .find_map(|pair| pair.strip_prefix("access_token=").map(|v| v.to_string()))
    })
}

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        // 桌面版取消了登录体系：请求一律视为本机所有者。
        // 若显式携带令牌，仍按真实用户解析，便于将来扩展多用户。
        if let Some(token) = extract_token(parts) {
            if let Ok(claims) = verify(&state.jwt_secret(), &token, "access") {
                return Ok(Self {
                    id: claims.sub,
                    username: claims.username,
                });
            }
        }
        Ok(state.local_user().await)
    }
}

/// 允许匿名访问的提取器：未登录时返回本地默认用户。
#[derive(Debug, Clone)]
pub struct OptionalUser(pub AuthUser);

impl FromRequestParts<Arc<AppState>> for OptionalUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(Self(user)),
            Err(_) => Ok(Self(AuthUser::anonymous())),
        }
    }
}
