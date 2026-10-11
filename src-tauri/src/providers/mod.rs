pub mod bailian;
pub mod deepseek;
pub mod glm;
pub mod hunyuan;
pub mod kimi;
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
    }
}
