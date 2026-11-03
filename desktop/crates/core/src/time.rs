//! 时间工具：统一使用本地时区 `YYYY-MM-DDTHH:MM:SS` 文本，便于 SQLite 排序与前端解析。

use chrono::{Local, NaiveDateTime, TimeZone};

const FMT: &str = "%Y-%m-%dT%H:%M:%S";

pub fn now() -> String {
    Local::now().format(FMT).to_string()
}

pub fn format(dt: chrono::DateTime<Local>) -> String {
    dt.format(FMT).to_string()
}

pub fn parse(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, FMT)
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok())
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(text)
                .ok()
                .map(|d| d.naive_local())
        })
}

pub fn to_local(text: &str) -> Option<chrono::DateTime<Local>> {
    parse(text).and_then(|naive| Local.from_local_datetime(&naive).single())
}

/// 兼容前端可能传入的纯数字时间戳。
pub fn normalize(input: Option<String>) -> String {
    match input {
        Some(v) if !v.trim().is_empty() => v,
        _ => now(),
    }
}
