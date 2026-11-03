//! 模型供应商抽象层：对话、图像、视频、语音统一入口。

pub mod agent;
pub mod media;
pub mod provider;
pub mod types;

pub use provider::{ImageRequest, ImageResult, ProviderRegistry, VideoPoll, VideoSubmit};
pub use types::{ChatMessage, ChatRequest, ChatResponse, StreamDelta, ToolCall, ToolDef};
