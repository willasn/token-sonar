//! 海外厂商（OpenAI / Anthropic / Gemini / xAI）。
//!
//! 这几家目前没有可用的公开「账户余额 / 逐日用量」接口（均需组织级管理密钥或
//! 只在官方后台展示），因此这里只做 API Key 连通性校验：保存后能看到 Key 是否有效，
//! 具体余额与用量请在官方后台查看。

use crate::http::{join_url, Http};
use crate::models::{Metric, ProviderConfig, ProviderKind, UsageSnapshot};
use crate::providers::util::{api_base, now_iso};

fn token(cfg: &ProviderConfig) -> Result<&str, String> {
    let key = cfg.token.as_deref().unwrap_or("").trim();
    if key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }
    Ok(key)
}

/// 拼接 `/{version}/models`，容忍用户把版本号写进站点地址。
fn models_url(base: &str, version: &str) -> String {
    if base.ends_with(&format!("/{version}")) {
        format!("{base}/models")
    } else {
        join_url(base, &format!("/{version}/models"))
    }
}

/// 用一次只读的模型列表请求校验 Key 是否有效。
async fn check_key(cfg: &ProviderConfig, http: &Http) -> Result<(), String> {
    let key = token(cfg)?;
    let base = api_base(cfg);
    let result = match cfg.kind {
        ProviderKind::OpenAi | ProviderKind::Xai => {
            http.get_json(&models_url(&base, "v1"), Some(key), None)
                .await
        }
        ProviderKind::Anthropic => {
            http.get_json_h(
                &models_url(&base, "v1"),
                None,
                None,
                &[("x-api-key", key), ("anthropic-version", "2023-06-01")],
            )
            .await
        }
        ProviderKind::Gemini => {
            let url = format!("{}?key={}", models_url(&base, "v1beta"), key);
            http.get_json(&url, None, None).await
        }
        other => return Err(format!("{} 暂不支持自动校验", other.default_name())),
    };
    result.map(|_| ())
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    match check_key(cfg, http).await {
        Ok(()) => {
            snap.ok = true;
            snap.metrics.push(Metric {
                label: "用量数据".to_string(),
                value: "请在官方后台查看".to_string(),
                hint: None,
            });
        }
        Err(e) => snap.error = Some(e),
    }

    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    check_key(cfg, http).await?;
    Ok(format!(
        "连接成功，{} API Key 有效（余额与用量请在官方后台查看）",
        cfg.kind.default_name()
    ))
}
