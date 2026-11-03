use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub nickname: Option<String>,
    pub avatar: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub status: i64,
    pub create_time: String,
    #[serde(skip_serializing)]
    pub update_time: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserVo {
    pub id: i64,
    pub username: String,
    pub nickname: String,
    pub avatar: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub status: i64,
    pub create_time: String,
    pub roles: Vec<String>,
}

impl From<User> for UserVo {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            nickname: u.nickname.unwrap_or_default(),
            avatar: u.avatar,
            email: u.email,
            phone: u.phone,
            status: u.status,
            create_time: u.create_time,
            roles: vec!["admin".into()],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterReq {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub confirm_password: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReq {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub confirm_password: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResp {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user_id: i64,
    pub username: String,
    pub nickname: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitStatusResp {
    pub initialized: bool,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileUpdateReq {
    pub nickname: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub avatar: Option<String>,
}

impl Default for ProfileUpdateReq {
    fn default() -> Self {
        Self {
            nickname: None,
            email: None,
            phone: None,
            avatar: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordReq {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemRuntimeVersion {
    /// 与前端 `currentVersion` 对齐（旧字段名 appVersion 保留兼容）
    pub current_version: String,
    pub current_version_display: String,
    pub app_version: String,
    pub build_time: String,
    pub runtime: String,
    pub rust_version: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub sort: i64,
    pub status: i64,
    pub remark: Option<String>,
    pub create_time: String,
    pub update_time: String,
}
