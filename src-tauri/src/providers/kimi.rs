//! Kimi 开放平台：官方余额接口。

 use serde_json::Value;

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, UsageSnapshot};
use crate::providers::util::{now_iso, pick_f64};

fn balance_url(cfg: &ProviderConfig) -> String {
    let base = cfg
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url())
        .trim_end_matches('/');
    let base = if base.starts_with("http://") || base.starts_with("https://") {
        base.to_string()
    } else {
        format!("https://{base}")
    };
    if base.ends_with("/v1/users/me/balance") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/users/me/balance")
    } else {
        format!("{base}/v1/users/me/balance")
    }
}

async fn read_balance(
    cfg: &ProviderConfig,
    http: &Http,
) -> Result<(&'static str, f64, f64, f64), String> {
    let key = cfg.token.as_deref().unwrap_or("").trim();
    if key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }
    let url = balance_url(cfg);
    let parsed = reqwest::Url::parse(&url)
        .map_err(|_| "Kimi 余额仅支持 HTTPS 官方余额端点".to_string())?;
    if parsed.scheme() != "https"
        || parsed.port_or_known_default() != Some(443)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/v1/users/me/balance"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Kimi 余额仅支持 HTTPS 官方余额端点".to_string());
    }
    let currency = match parsed.host_str() {
        Some("api.moonshot.cn") => "CNY",
        Some("api.moonshot.ai") => "USD",
        _ => return Err("Kimi 余额仅支持 HTTPS 官方余额端点".to_string()),
    };
    let v = http.get_json(&url, Some(key), None).await?;
    let (available, voucher, cash) = parse_balance(&v)?;
    Ok((currency, available, voucher, cash))
}

fn parse_balance(v: &Value) -> Result<(f64, f64, f64), String> {
    if v.get("code").and_then(Value::as_i64) != Some(0)
        || v.get("status").and_then(Value::as_bool) != Some(true)
    {
        let code = v.get("code").map(Value::to_string).unwrap_or_else(|| "缺失".to_string());
        return Err(format!("Kimi 余额接口业务失败（code {code}，status 非 true）"));
    }
    let data = v.get("data").ok_or("Kimi 余额接口未返回 data")?;
    let available = pick_f64(data, &["available_balance"])
        .ok_or("Kimi 余额接口未返回可用余额")?;
    let voucher = pick_f64(data, &["voucher_balance"])
        .ok_or("Kimi 余额接口未返回代金券余额")?;
    let cash = pick_f64(data, &["cash_balance"])
        .ok_or("Kimi 余额接口未返回现金余额")?;
    Ok((available, voucher, cash))
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    match read_balance(cfg, http).await {
        Ok((currency, available, voucher, cash)) => {
            snap.ok = true;
            snap.currency = currency.to_string();
            snap.balance = Some(available);
            for (label, amount) in [
                ("可用余额", available),
                ("代金券余额", voucher),
                ("现金余额", cash),
            ] {
                snap.metrics.push(Metric {
                    label: format!("{label}（{currency}）"),
                    value: format!("{amount:.2}"),
                    hint: None,
                });
            }
        }
        Err(e) => snap.error = Some(e),
    }

    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let (currency, available, _, _) = read_balance(cfg, http).await?;
    Ok(format!("连接成功，可用余额 {currency} {available:.2}"))
}
