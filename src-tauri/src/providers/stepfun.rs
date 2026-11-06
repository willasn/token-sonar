//! 阶跃星辰 StepFun：官方账户信息接口（仅余额，无用量）。
//!
//! `GET https://api.stepfun.com/v1/accounts`（Bearer API Key），返回
//! `type`（prepaid / postpaid）、`balance`、`total_cash_balance`、`total_voucher_balance`。
//! 官方未提供逐日用量或模型统计的公开接口，使用详情仅控制台可见。

use serde_json::Value;

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, UsageSnapshot};
use crate::providers::util::{api_base, now_iso, pick_f64, pick_string};

fn accounts_url(cfg: &ProviderConfig) -> String {
    let base = api_base(cfg);
    if base.ends_with("/v1/accounts") {
        base
    } else if base.ends_with("/v1") {
        format!("{base}/accounts")
    } else {
        format!("{base}/v1/accounts")
    }
}

/// 国内站按 CNY 结算，国际站（api.stepfun.ai）按 USD 结算。
fn currency(cfg: &ProviderConfig) -> &'static str {
    match reqwest::Url::parse(&api_base(cfg))
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .as_deref()
    {
        Some("api.stepfun.ai") => "USD",
        _ => "CNY",
    }
}

struct Account {
    kind: Option<String>,
    balance: f64,
    cash: f64,
    voucher: f64,
}

async fn read_account(cfg: &ProviderConfig, http: &Http) -> Result<Account, String> {
    let key = cfg.token.as_deref().unwrap_or("").trim();
    if key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }
    let v = http.get_json(&accounts_url(cfg), Some(key), None).await?;
    parse_account(&v)
}

fn parse_account(v: &Value) -> Result<Account, String> {
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        let msg = pick_string(err, &["message", "msg"]).unwrap_or_else(|| "未知错误".to_string());
        return Err(format!("阶跃星辰账户接口返回失败：{msg}"));
    }
    let balance = pick_f64(v, &["balance"]).ok_or("阶跃星辰账户接口未返回可用余额")?;
    Ok(Account {
        kind: pick_string(v, &["type"]).map(|t| match t.as_str() {
            "prepaid" => "预付费".to_string(),
            "postpaid" => "后付费".to_string(),
            _ => t,
        }),
        balance,
        cash: pick_f64(v, &["total_cash_balance"]).unwrap_or(0.0),
        voucher: pick_f64(v, &["total_voucher_balance"]).unwrap_or(0.0),
    })
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    match read_account(cfg, http).await {
        Ok(account) => {
            let currency = currency(cfg);
            snap.ok = true;
            snap.currency = currency.to_string();
            snap.balance = Some(account.balance);
            snap.metrics.push(Metric {
                label: format!("可用余额（{currency}）"),
                value: format!("{:.2}", account.balance),
                hint: Some("阶跃星辰官方账户接口；按站点币种展示".to_string()),
            });
            snap.metrics.push(Metric {
                label: format!("累计充值（{currency}）"),
                value: format!("{:.2}", account.cash),
                hint: None,
            });
            snap.metrics.push(Metric {
                label: format!("累计赠送（{currency}）"),
                value: format!("{:.2}", account.voucher),
                hint: None,
            });
            if let Some(kind) = account.kind {
                snap.metrics.push(Metric {
                    label: "账户类型".to_string(),
                    value: kind,
                    hint: Some("prepaid 预付费 / postpaid 后付费".to_string()),
                });
            }
        }
        Err(e) => snap.error = Some(e),
    }

    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let account = read_account(cfg, http).await?;
    Ok(format!(
        "连接成功，账户可用余额 {} {:.2}",
        currency(cfg),
        account.balance
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_balance_fields_and_rejects_error_body() {
        let account = parse_account(&json!({
            "object": "account",
            "type": "prepaid",
            "balance": 12.5,
            "total_cash_balance": 100.0,
            "total_voucher_balance": 26.0
        }))
        .unwrap();
        assert_eq!(account.balance, 12.5);
        assert_eq!(account.cash, 100.0);
        assert_eq!(account.voucher, 26.0);
        assert_eq!(account.kind.as_deref(), Some("预付费"));

        assert!(parse_account(&json!({"error": {"message": "invalid api key"}})).is_err());
        assert!(parse_account(&json!({"object": "account"})).is_err());
    }
}
