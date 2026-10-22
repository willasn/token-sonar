pub mod bailian;
pub mod deepseek;
pub mod glm;
pub mod hunyuan;
pub mod kimi;
pub mod mimo;
pub mod newapi;
pub mod util;
pub mod volcengine;

use crate::http::Http;
use crate::models::{ProviderConfig, ProviderKind, UsageSnapshot};

/// 统计窗口（天）
pub const WINDOW_DAYS: usize = 30;

pub async fn fetch_snapshot(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = match cfg.kind {
        ProviderKind::DeepSeek => deepseek::fetch(cfg, http).await,
        ProviderKind::NewApi => newapi::fetch(cfg, http, WINDOW_DAYS).await,
        ProviderKind::Glm => glm::fetch(cfg, http).await,
        ProviderKind::Kimi => kimi::fetch(cfg, http).await,
        ProviderKind::Volcengine => volcengine::fetch(cfg, http).await,
        ProviderKind::Bailian => bailian::fetch(cfg, http).await,
        ProviderKind::Hunyuan => hunyuan::fetch(cfg, http).await,
        ProviderKind::Mimo => mimo::fetch(cfg, http).await,
    };
    if !matches!(cfg.kind, ProviderKind::Bailian | ProviderKind::Hunyuan) {
        snap.days = crate::providers::newapi::fill_days(std::mem::take(&mut snap.days), WINDOW_DAYS);
    }
    snap
}

/// 连通性测试：只做最小代价的一次调用。
pub async fn probe(cfg: &ProviderConfig) -> Result<String, String> {
    let http = Http::new()?;
    match cfg.kind {
        ProviderKind::DeepSeek => deepseek::probe(cfg, &http).await,
        ProviderKind::NewApi => newapi::probe(cfg, &http).await,
        ProviderKind::Glm => glm::probe(cfg, &http).await,
        ProviderKind::Kimi => kimi::probe(cfg, &http).await,
        ProviderKind::Volcengine => volcengine::probe(cfg, &http).await,
        ProviderKind::Bailian => bailian::probe(cfg, &http).await,
        ProviderKind::Hunyuan => hunyuan::probe(cfg, &http).await,
        ProviderKind::Mimo => mimo::probe(cfg, &http).await,
    }
}

// ---------------------------------------------------------------------------
// 调用大模型（价格智能获取等场景）
// ---------------------------------------------------------------------------

/// 是否支持调用该渠道的对话模型（仅 OpenAI 兼容厂商）。
pub fn supports_chat(kind: ProviderKind) -> bool {
    matches!(
        kind,
        ProviderKind::DeepSeek | ProviderKind::NewApi | ProviderKind::Glm | ProviderKind::Kimi
    )
}

/// 用该渠道的模型跑一次对话，返回模型输出的文本。
pub async fn chat(
    cfg: &ProviderConfig,
    http: &Http,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    match cfg.kind {
        ProviderKind::DeepSeek => deepseek::chat(cfg, http, model, system, user).await,
        ProviderKind::NewApi => newapi::chat(cfg, http, model, system, user).await,
        ProviderKind::Glm => glm::chat(cfg, http, model, system, user).await,
        ProviderKind::Kimi => kimi::chat(cfg, http, model, system, user).await,
        other => Err(format!(
            "{} 暂不支持调用模型（当前支持 OpenAI 兼容渠道：DeepSeek / GLM / Kimi / New API）",
            other.default_name()
        )),
    }
}

/// 读取该渠道的可用模型列表（不支持或读取失败时返回空）。
pub async fn list_models(cfg: &ProviderConfig, http: &Http) -> Vec<String> {
    match cfg.kind {
        ProviderKind::DeepSeek => deepseek::list_models(cfg, http).await,
        ProviderKind::NewApi => newapi::list_models(cfg, http).await,
        ProviderKind::Glm => glm::list_models(cfg, http).await,
        ProviderKind::Kimi => kimi::list_models(cfg, http).await,
        _ => Vec::new(),
    }
}
