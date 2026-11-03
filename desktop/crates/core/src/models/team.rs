use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub logo: Option<String>,
    pub description: Option<String>,
    pub owner_user_id: i64,
    pub status: i64,
    pub create_time: String,
    pub update_time: String,
    #[sqlx(default)]
    pub member_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub id: i64,
    pub team_id: i64,
    pub user_id: i64,
    pub role: i64,
    pub status: i64,
    pub create_time: String,
    #[sqlx(default)]
    pub username: Option<String>,
    #[sqlx(default)]
    pub nickname: Option<String>,
    #[sqlx(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeamCreateReq {
    pub name: String,
    pub description: Option<String>,
    pub logo: Option<String>,
}

impl Default for TeamCreateReq {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            logo: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeamUpdateReq {
    pub id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub logo: Option<String>,
    pub status: Option<i64>,
}

impl Default for TeamUpdateReq {
    fn default() -> Self {
        Self {
            id: 0,
            name: None,
            description: None,
            logo: None,
            status: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeamMemberAddReq {
    pub team_id: i64,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub role: Option<i64>,
}

impl Default for TeamMemberAddReq {
    fn default() -> Self {
        Self {
            team_id: 0,
            user_id: None,
            username: None,
            role: None,
        }
    }
}
