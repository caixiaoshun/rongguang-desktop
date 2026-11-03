pub mod ai;
pub mod agent;
pub mod asset;
pub mod comfyui;
pub mod generation;
pub mod project;
pub mod script;
pub mod storage;
pub mod storyboard;
pub mod system;
pub mod team;

use serde::{Deserialize, Serialize};

/// 通用分页结果，字段名与前端 `PageResult` 一致。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResult<T> {
    pub list: Vec<T>,
    pub total: i64,
    pub page_no: i64,
    pub page_size: i64,
}

impl<T> PageResult<T> {
    pub fn new(list: Vec<T>, total: i64, page_no: i64, page_size: i64) -> Self {
        Self {
            list,
            total,
            page_no,
            page_size,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageQuery {
    pub page_no: i64,
    pub page_size: i64,
    pub keyword: Option<String>,
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page_no: 1,
            page_size: 20,
            keyword: None,
        }
    }
}

impl PageQuery {
    pub fn offset(&self) -> i64 {
        ((self.page_no.max(1) - 1) * self.page_size.max(1)).max(0)
    }
}
