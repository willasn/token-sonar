//! 硅基流动 SiliconFlow：官方账户信息接口（仅余额，无用量）。

use serde_json::Value;

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, UsageSnapshot};
use crate::providers::util::{api_base, now_iso, pick_f64, pick_string};

fn info_url(cfg: &ProviderConfig) -> String {
    let base = api_base(cfg);
    if base.ends_with("/v1/user/info") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/user/info")
    } else {
        format!("{base}/v1/user/info")
    }
}

/// 国内站按 CNY 结算，国际站按 USD 结算。
fn currency(cfg: &ProviderConfig) -> &'static str {
    match reqwest::Url::parse(&api_base(cfg))
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .as_deref()
    {
        Some("api.siliconflow.com") => "USD",
        _ => "CNY",
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
    let v = http.get_json(&info_url(cfg), Some(key), None).await?;
    let (total, charge, gift) = parse_info(&v)?;
    Ok((currency(cfg), total, charge, gift))
}

fn parse_info(v: &Value) -> Result<(f64, f64, f64), String> {
    if v.get("status").and_then(Value::as_bool) == Some(false) {
        let msg = pick_string(v, &["message", "msg"]).unwrap_or_else(|| "未知错误".to_string());
        return Err(format!("硅基流动账户接口返回失败：{msg}"));
    }
    if let Some(code) = v.get("code").and_then(Value::as_i64) {
        if code != 20000 && code != 0 {
            let msg = pick_string(v, &["message", "msg"]).unwrap_or_else(|| format!("code {code}"));
            return Err(format!("硅基流动账户接口返回失败：{msg}"));
        }
    }
    let data = v.get("data").ok_or("硅基流动账户接口未返回 data")?;
    let total = pick_f64(data, &["totalBalance"]).ok_or("硅基流动账户接口未返回总余额")?;
    let charge = pick_f64(data, &["chargeBalance"]).unwrap_or(0.0);
    let gift = pick_f64(data, &["balance"]).unwrap_or(0.0);
    Ok((total, charge, gift))
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    match read_balance(cfg, http).await {
        Ok((currency, total, charge, gift)) => {
            snap.ok = true;
            snap.currency = currency.to_string();
            snap.balance = Some(total);
            for (label, amount) in [("总余额", total), ("充值余额", charge), ("赠送余额", gift)]
            {
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
    let (currency, total, _, _) = read_balance(cfg, http).await?;
    Ok(format!("连接成功，账户总余额 {currency} {total:.2}"))
}
