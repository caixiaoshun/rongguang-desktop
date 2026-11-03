//! JSON 文本字段的序列化辅助。
//!
//! 数据库里 `properties`、`characters` 等列以 TEXT 存储原始 JSON；
//! 对外 JSON 契约要求它们是对象/数组而非字符串，因此统一在此处转换。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize_opt<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(text) if !text.trim().is_empty() => {
            let parsed: serde_json::Value =
                serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
            parsed.serialize(serializer)
        }
        _ => serializer.serialize_none(),
    }
}

pub fn deserialize_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(v.to_string()),
    })
}

pub fn serialize_opt_default_array<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(text) if !text.trim().is_empty() => {
            let parsed: serde_json::Value =
                serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::Array(vec![]));
            parsed.serialize(serializer)
        }
        _ => serializer.serialize_none(),
    }
}
