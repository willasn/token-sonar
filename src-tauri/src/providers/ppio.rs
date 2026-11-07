//! PPIO 派欧云（`api.ppio.com`）适配器。
//!
//! 只有 `api` 一种接入方式，一个 API Key 即可：
//!
//! | 接口 | 用途 |
//! |---|---|
//! | `GET /openapi/v1/billing/balance/detail` | 可用余额、现金余额、信用额度、欠费 |
//! | `GET /openapi/v1/billing/bill/list?cycleType=Day&productCategory=llm` | 按日、按模型的 LLM Token 与费用 |
//! | `GET /v3/user` | 账户额度（`credit_balance`，仅作补充） |
//!
//! 余额接口的金额单位是 1/10000 元（10000 = 1 元）；账单接口的
//! `amountDecimal` / `payableDecimal` 已是元，仅旧字段 `amount` 需要除以 10000。
//! 账单只有 LLM 按量消费，不含 GPU / 存储等其他产品。

use std::collections::BTreeMap;

use chrono::Local;
use serde_json::Value;

use crate::http::Http;
use crate::models::{DailyUsage, Metric, ModelUsage, ProviderConfig, UsageSnapshot};
use crate::providers::newapi::{month_period, today_period};
use crate::providers::util::{
    api_base, now_iso, pick, pick_f64, pick_string, to_f64, ts_to_date, unixtime_days_ago,
};

const BALANCE_PATH: &str = "/openapi/v1/billing/balance/detail";
const BILL_PATH: &str = "/openapi/v1/billing/bill/list";
const USER_PATH: &str = "/v3/user";

/// 余额接口的金额单位：1/10000 元。
const UNIT_SCALE: f64 = 10_000.0;

/// 账单里代表 Token 的用量字段：输入、输出、读缓存、写 5 分钟 / 1 小时缓存。
const TOKEN_FIELDS: [&str; 5] = ["billNum0", "billNum1", "billNum2", "billNum3", "billNum5"];

struct Balance {
    available: f64,
    cash: f64,
    credit: f64,
    owed: f64,
}

fn endpoint(cfg: &ProviderConfig, path: &str) -> String {
    format!("{}{}", api_base(cfg), path)
}

fn read_key(cfg: &ProviderConfig) -> Result<&str, String> {
    let key = cfg.token.as_deref().unwrap_or("").trim();
    if key.is_empty() {
        return Err("请先填写 PPIO API Key".to_string());
    }
    Ok(key)
}

fn scaled(v: &Value, key: &str) -> f64 {
    pick_f64(v, &[key]).unwrap_or(0.0) / UNIT_SCALE
}

async fn read_balance(cfg: &ProviderConfig, http: &Http) -> Result<Balance, String> {
    let key = read_key(cfg)?;
    let v = http.get_json(&endpoint(cfg, BALANCE_PATH), Some(key), None).await?;
    if pick_f64(&v, &["availableBalance"]).is_none() {
        let msg = pick_string(&v, &["message", "msg", "error"]).unwrap_or_else(|| "未知错误".into());
        return Err(format!("PPIO 余额接口未返回 availableBalance：{msg}"));
    }
    Ok(Balance {
        available: scaled(&v, "availableBalance"),
        cash: scaled(&v, "cashBalance"),
        credit: scaled(&v, "creditLimit"),
        owed: scaled(&v, "outstandingInvoices"),
    })
}

/// 读取最近 `days` 天的 LLM 按日账单（已按日期 + 模型聚合）。
async fn read_bills(
    cfg: &ProviderConfig,
    http: &Http,
    days: usize,
) -> Result<(Vec<DailyUsage>, Vec<ModelUsage>), String> {
    let key = read_key(cfg)?;
    let start = unixtime_days_ago(days as i64 - 1);
    let end = Local::now().timestamp();
    let url = format!(
        "{}?cycleType=Day&productCategory=llm&startTime={start}&endTime={end}",
        endpoint(cfg, BILL_PATH)
    );
    let v = http.get_json(&url, Some(key), None).await?;
    let bills = v
        .get("bills")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut by_day: BTreeMap<String, DailyUsage> = BTreeMap::new();
    let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();

    for bill in bills.iter() {
        let Some(date) = pick_f64(bill, &["startTime"])
            .map(|ts| ts as i64)
            .and_then(ts_to_date)
        else {
            continue;
        };
        let tokens: f64 = TOKEN_FIELDS
            .iter()
            .filter_map(|k| pick_f64(bill, &[k]))
            .sum();
        let cost = pick_f64(bill, &["amountDecimal"])
            .or_else(|| pick_f64(bill, &["payableDecimal"]))
            // 旧字段 amount 的单位是 1/10000 元
            .or_else(|| pick_f64(bill, &["amount"]).map(|x| x / UNIT_SCALE))
            .unwrap_or(0.0);
        let requests = pick_f64(bill, &["requestCount"]).unwrap_or(0.0);

        let day = by_day.entry(date.clone()).or_insert(DailyUsage {
            date: date.clone(),
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        day.requests += requests;
        day.tokens += tokens;
        day.cost += cost;

        let model = pick_string(bill, &["productName"])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未知模型".to_string());
        let item = by_model.entry(model.clone()).or_insert(ModelUsage {
            model,
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        item.requests += requests;
        item.tokens += tokens;
        item.cost += cost;
    }

    Ok((
        by_day.into_values().collect(),
        by_model.into_values().collect(),
    ))
}

/// 账户额度（`credit_balance`）仅作补充展示，失败不影响主流程。
async fn read_credit_balance(cfg: &ProviderConfig, http: &Http) -> Option<f64> {
    let key = read_key(cfg).ok()?;
    let v = http.get_json(&endpoint(cfg, USER_PATH), Some(key), None).await.ok()?;
    pick(&v, &["credit_balance"]).and_then(to_f64).map(|x| x / UNIT_SCALE)
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    let balance = match read_balance(cfg, http).await {
        Ok(b) => b,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };

    snap.ok = true;
    snap.balance = Some(balance.available);
    snap.metrics.push(Metric {
        label: format!("可用余额（{}）", snap.currency),
        value: format!("{:.2}", balance.available),
        hint: Some("PPIO 账单接口：可用余额，单位为元".to_string()),
    });
    snap.metrics.push(Metric {
        label: format!("现金余额（{}）", snap.currency),
        value: format!("{:.2}", balance.cash),
        hint: None,
    });
    if balance.credit > 0.0 {
        snap.metrics.push(Metric {
            label: format!("信用额度（{}）", snap.currency),
            value: format!("{:.2}", balance.credit),
            hint: None,
        });
    }
    if balance.owed > 0.0 {
        snap.metrics.push(Metric {
            label: format!("欠费金额（{}）", snap.currency),
            value: format!("{:.2}", balance.owed),
            hint: None,
        });
    }

    match read_bills(cfg, http, crate::providers::WINDOW_DAYS).await {
        Ok((days, mut models)) => {
            models.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
            let cost: f64 = days.iter().map(|d| d.cost).sum();
            let tokens: f64 = days.iter().map(|d| d.tokens).sum();
            let requests: f64 = days.iter().map(|d| d.requests).sum();
            snap.used = Some(cost);
            snap.tokens_total = Some(tokens);
            snap.requests_total = Some(requests);
            snap.today = today_period(&days);
            snap.month = month_period(&days);
            snap.supports_usage_api = true;
            snap.days = days;
            snap.models = models;
            snap.metrics.push(Metric {
                label: "近 30 天 LLM 消费（CNY）".to_string(),
                value: format!("{cost:.2}"),
                hint: Some("来自 PPIO 按日账单，仅含 LLM API 按量消费".to_string()),
            });
            snap.metrics.push(Metric {
                label: "近 30 天 Token".to_string(),
                value: format!("{tokens:.0}"),
                hint: Some(format!("{requests:.0} 次请求")),
            });
        }
        Err(e) => {
            snap.metrics.push(Metric {
                label: "用量数据".to_string(),
                value: "不可用".to_string(),
                hint: Some(e),
            });
        }
    }

    if let Some(credit) = read_credit_balance(cfg, http).await {
        snap.balance_raw = Some(credit);
        snap.metrics.push(Metric {
            label: "账户额度（credit_balance）".to_string(),
            value: format!("{credit:.2}"),
            hint: Some("PPIO /v3/user 返回，含代金券等额度，可能与可用余额不同".to_string()),
        });
    }

    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let balance = read_balance(cfg, http).await?;
    Ok(format!(
        "连接成功，账户可用余额 CNY {:.2}",
        balance.available
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实接口测试：仅在设置 `PPIO_TEST_TOKEN` 环境变量时执行，避免 CI 依赖外网与个人凭证。
    #[tokio::test]
    async fn live_balance_and_bills() {
        let Ok(token) = std::env::var("PPIO_TEST_TOKEN") else {
            return;
        };
        let cfg: ProviderConfig = serde_json::from_value(serde_json::json!({
            "id": "ppio-test",
            "name": "PPIO",
            "kind": "ppio",
            "token": token,
            "currency": "CNY",
            "quotaPerUnit": 500000,
            "displayInCurrency": true,
            "createdAt": "",
            "updatedAt": ""
        }))
        .expect("构造 PPIO 测试配置失败");

        let http = Http::new().expect("初始化 HTTP 客户端失败");
        let snap = fetch(&cfg, &http).await;
        assert!(snap.ok, "PPIO 余额读取失败: {:?}", snap.error);
        assert!(snap.balance.is_some(), "未解析到余额");
        println!(
            "balance={:?} used={:?} tokens={:?} requests={:?} days={} models={}",
            snap.balance,
            snap.used,
            snap.tokens_total,
            snap.requests_total,
            snap.days.len(),
            snap.models.len()
        );
    }
}
