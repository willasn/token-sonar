//! MiniMax 开放平台适配器（国内 / 海外同一套接口，仅域名与币种不同）。
//!
//! 只有 `api` 一种接入方式，一个 API Key 即可，不需要 GroupId、AK/SK 或登录：
//!
//! | 凭证 | 接口 | 能读到什么 |
//! |---|---|---|
//! | 按量付费 Key（`sk-api-` 开头） | `GET {origin}/account/query_balance` | 可用余额、现金、代金券、授信、欠费 |
//! | 订阅 Key（Token Plan） | `GET {origin}/v1/token_plan/remains` | 各模型家族的 5 小时 / 周窗口用量与重置时间 |
//! | 订阅 Key（Coding Plan） | `GET {origin}/v1/coding_plan/remains` 或 `{platform}/v1/api/openplatform/coding_plan/remains` | 同上 |
//!
//! 鉴权统一为 `Authorization: Bearer <API Key>`，业务成败看 `base_resp.status_code`（0 为成功）。
//! 站点只接受官方域名：国内 `api.minimaxi.com`（CNY）、海外 `api.minimax.io`（USD）。
//!
//! 平台未开放逐日用量、账单明细与 API Key 列表；按量账户只有钱包余额，
//! 订阅账户只有滚动窗口额度，两者都不含金额与 Token 明细。

use chrono::{Local, TimeZone};
use serde_json::Value;

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, QuotaWindow, UsageSnapshot};
use crate::providers::util::{api_base, now_iso, pick, pick_f64, pick_string};

const CN_HOST: &str = "api.minimaxi.com";
const INTL_HOST: &str = "api.minimax.io";

struct Remains {
    source: &'static str,
    body: Value,
}

/// 只保留 scheme + host，并校验必须是官方 HTTPS 站点。
fn api_origin(cfg: &ProviderConfig) -> Result<String, String> {
    let raw = api_base(cfg);
    let raw = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        format!("https://{raw}")
    };
    let parsed = reqwest::Url::parse(&raw).map_err(|_| "MiniMax 站点地址格式不正确".to_string())?;
    let host = parsed.host_str().unwrap_or("");
    let official = host == CN_HOST || host == INTL_HOST;
    if parsed.scheme() != "https"
        || parsed.port_or_known_default() != Some(443)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !official
    {
        return Err(
            "MiniMax 仅支持官方 HTTPS 端点：国内 api.minimaxi.com / 海外 api.minimax.io".to_string(),
        );
    }
    Ok(format!("https://{host}"))
}

fn currency_of(origin: &str) -> &'static str {
    if origin.contains("minimax.io") {
        "USD"
    } else {
        "CNY"
    }
}

/// 平台把业务失败也包在 HTTP 200 里，必须按 `base_resp.status_code` 判定。
fn ensure_ok(v: &Value) -> Result<(), String> {
    let Some(resp) = pick(v, &["base_resp"]) else {
        return Ok(());
    };
    let Some(code) = pick_f64(resp, &["status_code"]) else {
        return Ok(());
    };
    if code == 0.0 {
        return Ok(());
    }
    let msg = pick_string(resp, &["status_msg"]).unwrap_or_default();
    Err(if msg.is_empty() {
        format!("MiniMax 接口返回 status_code {}", code as i64)
    } else {
        format!("MiniMax 接口 status_code {}：{msg}", code as i64)
    })
}

fn is_auth_error(e: &str) -> bool {
    let lower = e.to_lowercase();
    e.contains("HTTP 401")
        || e.contains("HTTP 403")
        || lower.contains("1004")
        || lower.contains("login fail")
        || lower.contains("authorization")
}

/// 鉴权类错误翻译成可操作提示（区域填错是最常见原因）。
fn friendly(e: String) -> String {
    if is_auth_error(&e) {
        format!(
            "API Key 校验失败，请确认 Key 与站点区域一致（国内 api.minimaxi.com / 海外 api.minimax.io）：{e}"
        )
    } else {
        e
    }
}

// ---------------------------------------------------------------------------
// 按量付费：账户余额
// ---------------------------------------------------------------------------

async fn read_balance(origin: &str, key: &str, http: &Http) -> Result<Value, String> {
    let url = format!("{origin}/account/query_balance");
    let v = http.get_json(&url, Some(key), None).await.map_err(friendly)?;
    ensure_ok(&v).map_err(friendly)?;
    if pick_f64(&v, &["available_amount"]).is_none() {
        return Err("MiniMax 余额接口未返回 available_amount".to_string());
    }
    Ok(v)
}

/// 余额 → 指标。返回是否读到可用余额。
fn apply_balance(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let Some(available) = pick_f64(v, &["available_amount"]) else {
        return false;
    };
    snap.balance = Some(available);

    let cash = pick_f64(v, &["cash_balance"]);
    let voucher = pick_f64(v, &["voucher_balance"]);
    let credit = pick_f64(v, &["credit_balance"]);
    let owed = pick_f64(v, &["owed_amount"]);

    let mut hint: Vec<String> = Vec::new();
    if let Some(x) = credit.filter(|x| *x != 0.0) {
        hint.push(format!("授信 {x:.2}"));
    }
    if let Some(x) = owed.filter(|x| *x != 0.0) {
        hint.push(format!("欠费 {x:.2}"));
    }

    snap.metrics.insert(
        0,
        Metric {
            label: format!("账户余额（{}）", snap.currency),
            value: format!("{available:.2}"),
            hint: if hint.is_empty() {
                None
            } else {
                Some(hint.join(" · "))
            },
        },
    );
    for (label, amount) in [
        ("现金余额", cash),
        ("代金券余额", voucher),
        ("授信额度", credit),
        ("欠费金额", owed),
    ] {
        if let Some(x) = amount {
            snap.metrics.push(Metric {
                label: format!("{label}（{}）", snap.currency),
                value: format!("{x:.2}"),
                hint: None,
            });
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 订阅：Token Plan / Coding Plan 滚动窗口
// ---------------------------------------------------------------------------

fn has_remains(v: &Value) -> bool {
    v.get("model_remains")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(Value::is_object))
        .unwrap_or(false)
}

/// 依次尝试 Token Plan、Coding Plan 的两个已知路径；鉴权失败时不再继续试。
async fn read_remains(origin: &str, key: &str, http: &Http) -> Result<Remains, String> {
    let platform = origin.replacen("://api.", "://platform.", 1);
    let candidates: [(&'static str, String); 3] = [
        ("Token Plan", format!("{origin}/v1/token_plan/remains")),
        ("Coding Plan", format!("{origin}/v1/coding_plan/remains")),
        (
            "Coding Plan",
            format!("{platform}/v1/api/openplatform/coding_plan/remains"),
        ),
    ];

    let mut last = String::new();
    for (source, url) in candidates {
        match http.get_json(&url, Some(key), None).await {
            Ok(v) => match ensure_ok(&v) {
                Ok(()) if has_remains(&v) => return Ok(Remains { source, body: v }),
                Ok(()) => last = format!("额度接口未返回 model_remains：{url}"),
                Err(e) => {
                    let e = friendly(e);
                    if is_auth_error(&e) {
                        return Err(e);
                    }
                    last = e;
                }
            },
            Err(e) => {
                let e = friendly(e);
                if is_auth_error(&e) {
                    return Err(e);
                }
                last = e;
            }
        }
    }
    Err(if last.is_empty() {
        "未读取到套餐额度".to_string()
    } else {
        last
    })
}

/// `*_usage_count` 在新旧响应里语义不一致：有显式剩余百分比时用它消歧，
/// 缺失时按官方旧口径（剩余次数）解释。
fn resolve_remaining(total: f64, reported: f64, remaining_pct: Option<f64>) -> f64 {
    if total <= 0.0 {
        return 0.0;
    }
    let reported = reported.clamp(0.0, total);
    let Some(pct) = remaining_pct.filter(|p| p.is_finite()) else {
        return reported;
    };
    let as_remaining = reported / total * 100.0;
    let as_used = (total - reported) / total * 100.0;
    if (as_used - pct).abs() < (as_remaining - pct).abs() {
        total - reported
    } else {
        reported
    }
}

/// 重置时间：接口给毫秒时间戳。
fn reset_at(item: &Value, key: &str) -> Option<String> {
    let ts = pick_f64(item, &[key])? as i64;
    if ts <= 0 {
        return None;
    }
    let dt = if ts > 1_000_000_000_000 {
        Local.timestamp_millis_opt(ts).single()
    } else {
        Local.timestamp_opt(ts, 0).single()
    }?;
    Some(dt.format("%m-%d %H:%M").to_string())
}

fn build_window(
    item: &Value,
    total_key: &str,
    usage_key: &str,
    pct_key: &str,
    end_key: &str,
    label: &str,
) -> Option<QuotaWindow> {
    let total = pick_f64(item, &[total_key])?;
    if total <= 0.0 {
        return None;
    }
    let reported = pick_f64(item, &[usage_key]).unwrap_or(0.0);
    let remaining = resolve_remaining(total, reported, pick_f64(item, &[pct_key]));
    let used = (total - remaining).clamp(0.0, total);
    Some(QuotaWindow {
        label: label.to_string(),
        used,
        quota: total,
        percent: used / total * 100.0,
        reset_at: reset_at(item, end_key),
        reset_in: None,
    })
}

/// 把接口里的模型标识翻成卡片上可读的家族名。
fn model_label(name: &str) -> String {
    let n = name.trim();
    if n.is_empty() {
        return "套餐额度".to_string();
    }
    let lower = n.to_lowercase();
    if lower.starts_with("minimax-m") || lower.starts_with("abab") {
        "M 系列".to_string()
    } else if lower.contains("speech") || lower.contains("t2a") {
        "语音".to_string()
    } else if lower.contains("image") {
        "图像".to_string()
    } else if lower.contains("video") {
        "视频".to_string()
    } else if lower.contains("music") {
        "音乐".to_string()
    } else {
        n.to_string()
    }
}

/// 额度响应 → QuotaWindow 列表。返回是否读到任何窗口。
fn apply_remains(snap: &mut UsageSnapshot, source: &str, v: &Value) -> bool {
    let items: Vec<Value> = v
        .get("model_remains")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut found = false;
    for item in items {
        if !item.is_object() {
            continue;
        }
        let label = model_label(&pick_string(&item, &["model_name"]).unwrap_or_default());

        // status 3 = 不限量，没有可展示的比例
        if pick_f64(&item, &["current_interval_status"]) == Some(3.0) {
            snap.metrics.push(Metric {
                label: format!("{label} · 5h"),
                value: "不限量".to_string(),
                hint: None,
            });
            found = true;
            continue;
        }

        if let Some(w) = build_window(
            &item,
            "current_interval_total_count",
            "current_interval_usage_count",
            "current_interval_remaining_percent",
            "end_time",
            &format!("{label} · 5h"),
        ) {
            snap.quotas.push(w);
            found = true;
        }

        let weekly_total = pick_f64(&item, &["current_weekly_total_count"]).unwrap_or(0.0);
        if weekly_total > 0.0 {
            if let Some(w) = build_window(
                &item,
                "current_weekly_total_count",
                "current_weekly_usage_count",
                "current_weekly_remaining_percent",
                "weekly_end_time",
                &format!("{label} · 周"),
            ) {
                snap.quotas.push(w);
                found = true;
            }
        }
    }

    if found {
        snap.metrics.push(Metric {
            label: "套餐类型".to_string(),
            value: source.to_string(),
            hint: Some("按 API Key 自动识别".to_string()),
        });
    }
    found
}

// ---------------------------------------------------------------------------
// 抓取入口
// ---------------------------------------------------------------------------

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    let key = cfg.token.as_deref().unwrap_or("").trim().to_string();
    if key.is_empty() {
        snap.error = Some("请先填写 API Key".to_string());
        return snap;
    }
    let origin = match api_origin(cfg) {
        Ok(o) => o,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };
    snap.currency = currency_of(&origin).to_string();

    let mut failure: Option<String> = None;
    let mut has_data = false;

    if key.starts_with("sk-api-") {
        // 按量付费 Key：只有钱包余额
        match read_balance(&origin, &key, http).await {
            Ok(v) => {
                has_data = apply_balance(&mut snap, &v);
                if !has_data {
                    failure = Some("余额接口未识别到 available_amount".to_string());
                }
            }
            Err(e) => failure = Some(e),
        }
    } else {
        // 订阅 Key：先读套餐窗口，失败再兜底按量余额
        match read_remains(&origin, &key, http).await {
            Ok(r) => {
                has_data = apply_remains(&mut snap, r.source, &r.body);
                if !has_data {
                    failure = Some("额度接口返回空 model_remains".to_string());
                }
            }
            Err(plan_err) => match read_balance(&origin, &key, http).await {
                Ok(v) => {
                    has_data = apply_balance(&mut snap, &v);
                    if has_data {
                        snap.metrics.push(Metric {
                            label: "读取提示".to_string(),
                            value: "未检测到订阅套餐，已按按量付费余额读取".to_string(),
                            hint: Some(plan_err.chars().take(80).collect()),
                        });
                    } else {
                        failure = Some(plan_err);
                    }
                }
                Err(balance_err) => {
                    failure = Some(format!("{plan_err}；余额接口：{balance_err}"));
                }
            },
        }
    }

    if has_data {
        snap.ok = true;
        if let Some(e) = failure {
            snap.metrics.push(Metric {
                label: "读取提示".to_string(),
                value: e.chars().take(80).collect(),
                hint: None,
            });
        }
    } else {
        snap.ok = false;
        snap.error = Some(failure.unwrap_or_else(|| "未能读取到任何数据".to_string()));
    }

    snap
}

// ---------------------------------------------------------------------------
// 连通性测试
// ---------------------------------------------------------------------------

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let key = cfg.token.as_deref().unwrap_or("").trim().to_string();
    if key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }
    let origin = api_origin(cfg)?;
    let currency = currency_of(&origin);

    if key.starts_with("sk-api-") {
        let v = read_balance(&origin, &key, http).await?;
        let available = pick_f64(&v, &["available_amount"]).unwrap_or(0.0);
        return Ok(format!("连接成功，可用余额 {currency} {available:.2}"));
    }

    match read_remains(&origin, &key, http).await {
        Ok(r) => {
            let items: Vec<Value> = r
                .body
                .get("model_remains")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut parts: Vec<String> = vec![r.source.to_string()];
            if let Some(item) = items.iter().find(|i| {
                pick_f64(i, &["current_interval_total_count"]).unwrap_or(0.0) > 0.0
            }) {
                let label = model_label(&pick_string(item, &["model_name"]).unwrap_or_default());
                if let Some(w) = build_window(
                    item,
                    "current_interval_total_count",
                    "current_interval_usage_count",
                    "current_interval_remaining_percent",
                    "end_time",
                    &format!("{label} · 5h"),
                ) {
                    parts.push(format!("{} 已用 {:.1}%", w.label, w.percent));
                    if let Some(reset) = w.reset_at {
                        parts.push(format!("重置于 {reset}"));
                    }
                }
            }
            Ok(format!("连接成功，{}", parts.join("，")))
        }
        Err(plan_err) => match read_balance(&origin, &key, http).await {
            Ok(v) => {
                let available = pick_f64(&v, &["available_amount"]).unwrap_or(0.0);
                Ok(format!(
                    "连接成功，可用余额 {currency} {available:.2}；未检测到订阅套餐"
                ))
            }
            Err(balance_err) => Err(format!(
                "{}；余额接口：{}",
                plan_err,
                balance_err
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_remaining_count() {
        // 没有显式百分比时按「剩余次数」解释
        assert_eq!(resolve_remaining(1500.0, 228.0, None), 228.0);
    }

    #[test]
    fn disambiguate_by_percent() {
        // 228 更接近「已用 84.8%」而不是「剩余 15.2%」，应解释为已用
        assert_eq!(resolve_remaining(1500.0, 228.0, Some(84.8)), 1272.0);
        assert_eq!(resolve_remaining(1500.0, 228.0, Some(15.2)), 228.0);
    }

    #[test]
    fn clamp_reported() {
        assert_eq!(resolve_remaining(100.0, 120.0, None), 100.0);
    }

    #[test]
    fn balance_fields() {
        let v = serde_json::json!({
            "available_amount": "98.00001",
            "cash_balance": "88.50",
            "voucher_balance": "9.50",
            "credit_balance": "0",
            "owed_amount": "0"
        });
        assert_eq!(pick_f64(&v, &["available_amount"]), Some(98.00001));
    }
}
