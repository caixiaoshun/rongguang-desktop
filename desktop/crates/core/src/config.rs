//! 运行时配置：监听端口、JWT 密钥、默认供应商等，持久化在数据目录 `config.json`。

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::AppResult;
use crate::time;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// 内嵌 HTTP 服务端口；0 表示由系统分配。
    pub port: u16,
    /// JWT 签名密钥，首次启动随机生成。
    pub jwt_secret: String,
    /// 默认主供应商平台标识。
    pub default_platform: String,
    /// Grok 服务地址。
    ///
    /// 默认留空：这是用户自己的中转站地址，不能由程序预置——仓库和安装包都是公开分发的，
    /// 预置等于把维护者的私有域名一起发出去，也不利于用户换成自己的服务。
    /// 用户需要在「设置 → AI 模型」里自行填写，前端会拦住空值。
    pub grok_base_url: String,
    /// 桌面端只允许本机访问。
    pub bind_localhost_only: bool,
    /// ffmpeg 可执行文件路径（空表示使用随包或 PATH）。
    pub ffmpeg_path: String,
    pub updated_at: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 0,
            jwt_secret: random_secret(),
            default_platform: "grok".into(),
            // 不预置任何服务地址，见字段注释
            grok_base_url: String::new(),
            bind_localhost_only: true,
            ffmpeg_path: String::new(),
            updated_at: time::now(),
        }
    }
}

impl AppConfig {
    pub fn load(root: &Path) -> AppResult<Self> {
        let file = root.join("config.json");
        if !file.exists() {
            let cfg = AppConfig::default();
            cfg.save(root)?;
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut cfg: AppConfig = serde_json::from_str(&text).unwrap_or_default();
        if cfg.jwt_secret.trim().is_empty() {
            cfg.jwt_secret = random_secret();
        }
        Ok(cfg)
    }

    pub fn save(&self, root: &Path) -> AppResult<()> {
        let file = root.join("config.json");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(file, text)?;
        Ok(())
    }
}

fn random_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..48)
        .map(|_| {
            let idx: usize = rng.gen_range(0..62);
            let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
            chars[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安装包是公开分发的，默认配置里不能出现任何维护者的服务地址或密钥。
    ///
    /// 这条断言是防回归用的：曾经 `grok_base_url` 默认写死了维护者的中转站域名，
    /// 编进二进制后 `strings` 就能直接看到，等于把私有地址随安装包发出去。
    #[test]
    fn default_config_must_not_bake_in_any_endpoint_or_secret() {
        let cfg = AppConfig::default();
        assert!(
            cfg.grok_base_url.is_empty(),
            "grok_base_url 默认必须为空，实际是 {:?}",
            cfg.grok_base_url
        );
        // 兜底：整个序列化结果里不能出现 http(s) 地址
        let text = serde_json::to_string(&cfg).unwrap_or_default();
        assert!(
            !text.contains("http://") && !text.contains("https://"),
            "默认配置里不允许出现服务地址: {text}"
        );
    }
}
