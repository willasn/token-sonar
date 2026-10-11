//! 智谱 GLM / Z.ai 开放平台适配器。
//!
//! 只有 `api` 一种接入方式：填入开放平台 API Key，读取账户余额与套餐额度，
//! 平台没有开放逐日明细，仅展示账户汇总中的真实累计消费与套餐额度。
//!
//! | 接入方式 | 凭证 | 域名 | 能读到什么 |
//! |---|---|---|---|
//! | `api` | 开放平台 API Key | `open.bigmodel.cn`（海外 `api.z.ai`） | 账户余额 / 累计消费 + 套餐等级、5 小时 / 周 / MCP 额度使用率与重置时间 |
//!
//! 账户汇总（余额）：`GET {origin}/api/biz/account/query-customer-account-report`
//! 请求头：`Authorization: <api_key>`（裸 Key，**不带** `Bearer` 前缀）
//! 返回：`{ code: 200, data: { balance, availableBalance, rechargeAmount, giveAmount,
//! totalSpendAmount, frozenBalance, ... } }`
//! 该接口未出现在官方文档里，属社区发现并广泛使用的路径，国内 / 海外站点均可用。
//!
//! 套餐额度：`GET {origin}/api/monitor/usage/quota/limit`
//! 返回：`{ code: 200, data: { limits: [{ type, unit, usage, currentValue, percentage, nextResetTime }], level } }`
//! `unit`：3 = 5 小时额度、6 = 周额度、5 = 月度 MCP / 搜索额度。
//!
//! 按量付费账号（未订阅 Coding Plan）额度接口会返回「当前用户不存在 coding plan」，
//! 此时不视为失败，降级为「无编程套餐（按量付费）」提示；余额仍可从账户汇总接口读到，
//! 平台未开放逐日用量接口，因此不提供逐日用量。

use serde_json::Value;

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, UsageSnapshot};
use crate::providers::util::*;

/// 拼接额度查询接口地址：只保留 scheme + host，忽略用户可能填的业务路径。
fn quota_url(base: &str) -> String {
    format!("{}/api/monitor/usage/quota/limit", origin(base))
}

/// 账户汇总接口（余额 / 累计充值 / 累计消费），同样只取 scheme + host。
fn account_url(base: &str) -> String {
    format!("{}/api/biz/account/query-customer-account-report", origin(base))
}

fn origin(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let b = if b.starts_with("http://") || b.starts_with("https://") {
        b.to_string()
    } else {
        format!("https://{b}")
    };
    match b.find("://") {
        Some(i) => {
            let rest = i + 3;
            match b[rest..].find('/') {
                Some(j) => b[..rest + j].to_string(),
                None => b,
            }
        }
        None => b,
    }
}

/// 平台把业务失败也包在 HTTP 200 里，必须按 `code` 判定（成功为 200）。
fn ensure_ok(v: &Value) -> Result<(), String> {
    match pick_f64(v, &["code"]) {
        None => Ok(()),
        Some(c) if c == 200.0 || c == 0.0 => Ok(()),
        Some(c) => {
            let msg = pick_string(v, &["msg", "message"]).unwrap_or_default();
            Err(if msg.is_empty() {
                format!("额度接口返回 code {}", c as i64)
            } else {
                format!("额度接口 code {}: {msg}", c as i64)
            })
        }
    }
}

/// 鉴权类错误翻译成可操作的中文提示。
fn auth_error(e: String) -> String {
    if e.contains("HTTP 401")
        || e.contains("HTTP 403")
        || e.contains("1002")
        || e.contains("1000")
        || e.contains("Authorization")
        || e.contains("invalid")
    {
        format!("API Key 校验失败（{e}），请确认填的是智谱开放平台的 API Key")
    } else {
        e
    }
}

/// 按量付费账号（未订阅 Coding Plan）：额度接口返回「当前用户不存在 coding plan」这类业务错误。
/// 它说明 API Key 有效、只是没有可读的套餐额度，应降级为「无额度」而不是报错。
fn is_no_plan(e: &str) -> bool {
    let lower = e.to_lowercase();
    lower.contains("coding plan") || lower.contains("codingplan")
}

fn fmt_num(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 使用率：优先用「已用 / 上限」计算，缺失时直接用接口给的百分比。
fn percent(current: Option<f64>, usage: Option<f64>, pct: Option<f64>) -> Option<f64> {
    match (current, usage) {
        (Some(c), Some(u)) if u > 0.0 => Some(c / u * 100.0),
        _ => pct,
    }
}

fn unit_label(item: &Value, idx: usize) -> String {
    let kind = pick_string(item, &["type"]).unwrap_or_default();
    let unit = pick_f64(item, &["unit"]).map(|u| u as i64);
    match unit {
        Some(3) => "5 小时额度".to_string(),
        Some(6) => "周额度".to_string(),
        Some(5) => {
            if kind.eq_ignore_ascii_case("TIME_LIMIT") {
                "MCP 次数额度".to_string()
            } else {
                "月度额度".to_string()
            }
        }
        _ => {
            if kind.eq_ignore_ascii_case("TIME_LIMIT") {
                "MCP 次数额度".to_string()
            } else {
                match idx {
                    0 => "5 小时额度".to_string(),
                    1 => "周额度".to_string(),
                    n => format!("额度 {}", n + 1),
                }
            }
        }
    }
}

/// 重置时间：接口可能给 unix 秒/毫秒或 ISO 字符串。
fn reset_time(item: &Value) -> Option<String> {
    let raw = pick(
        item,
        &[
            "nextResetTime",
            "next_reset_time",
            "resetTime",
            "reset_time",
        ],
    )?;
    match raw {
        Value::Number(n) => n.as_i64().and_then(ts_to_date),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else if let Ok(ts) = s.parse::<i64>() {
                ts_to_date(ts)
            } else {
                Some(s.chars().take(19).collect())
            }
        }
        _ => None,
    }
}

/// 在返回体里兜底找「最像额度列表」的对象数组。
fn find_limit_array(v: &Value) -> Vec<Value> {
    let mut arrays: Vec<&Vec<Value>> = Vec::new();
    record_arrays(v, &mut arrays, 0);
    arrays
        .into_iter()
        .find(|a| {
            a.iter().any(|r| {
                pick(r, &["usage", "currentValue", "percentage"]).is_some()
                    && pick(r, &["unit", "type"]).is_some()
            })
        })
        .cloned()
        .unwrap_or_default()
}

/// 额度 → 指标。返回是否读到额度。
fn apply_quota(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let data = v.get("data").cloned().unwrap_or(Value::Null);

    let limits: Vec<Value> = data
        .get("limits")
        .or_else(|| v.get("limits"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let items = if limits.is_empty() {
        find_limit_array(&data)
    } else {
        limits
    };

    if let Some(level) = pick_string(&data, &["level", "plan", "planName", "packageName"])
        .or_else(|| pick_string(v, &["level", "plan"]))
    {
        snap.metrics.push(Metric {
            label: "套餐等级".to_string(),
            value: level,
            hint: None,
        });
    }

    let mut found = false;
    for (idx, item) in items.iter().enumerate() {
        let usage = pick_f64(item, &["usage", "limit", "total"]);
        let current = pick_f64(
            item,
            &["currentValue", "current_value", "current", "used"],
        );
        let pct = percent(
            current,
            usage,
            pick_f64(item, &["percentage", "percent", "usedPercentage"]),
        );
        let reset = reset_time(item);

        let mut parts: Vec<String> = Vec::new();
        if let (Some(c), Some(u)) = (current, usage) {
            parts.push(format!("已用 {} / 上限 {}", fmt_num(c), fmt_num(u)));
        } else if let Some(c) = current {
            parts.push(format!("已用 {}", fmt_num(c)));
        }
        if let Some(r) = &reset {
            parts.push(format!("重置于 {r}"));
        }

        if pct.is_none() && parts.is_empty() {
            continue;
        }

        let value = match pct {
            Some(p) => format!("{p:.1}% 已用"),
            None => "—".to_string(),
        };
        snap.metrics.push(Metric {
            label: unit_label(item, idx),
            value,
            hint: if parts.is_empty() {
                None
            } else {
                Some(parts.join(" · "))
            },
        });
        found = true;
    }

    if found {
        snap.metrics.push(Metric {
            label: "额度数据来源".to_string(),
            value: "GLM 开放平台额度接口".to_string(),
            hint: Some("GET /api/monitor/usage/quota/limit".to_string()),
        });
    }
    found
}

// ---------------------------------------------------------------------------
// 账户余额 / 累计消费
// ---------------------------------------------------------------------------

/// 账户汇总返回体里的可用余额：优先 availableBalance，缺失时退回 balance。
fn balance_of(v: &Value) -> Option<f64> {
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    pick_f64(&data, &["availableBalance", "available_balance"])
        .or_else(|| pick_f64(&data, &["balance"]))
        .or_else(|| pick_f64(v, &["availableBalance", "available_balance", "balance"]))
}

/// 账户汇总 → 余额 / 充值 / 累计消费。返回是否读到余额。
fn apply_balance(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let data = v.get("data").cloned().unwrap_or(Value::Null);

    let Some(balance) = balance_of(v) else {
        return false;
    };
    snap.balance = Some(balance);
    snap.granted = pick_f64(&data, &["rechargeAmount", "recharge_amount"]);
    // 累计消费只取平台账户汇总数据
    if let Some(spend) = pick_f64(&data, &["totalSpendAmount", "total_spend_amount"]) {
        snap.used = Some(spend);
    }

    let mut hint: Vec<String> = Vec::new();
    if let Some(x) = pick_f64(&data, &["rechargeAmount", "recharge_amount"]) {
        hint.push(format!("累计充值 {x:.2}"));
    }
    if let Some(x) = pick_f64(&data, &["giveAmount", "give_amount"]) {
        if x > 0.0 {
            hint.push(format!("赠送 {x:.2}"));
        }
    }
    if let Some(x) = pick_f64(&data, &["totalSpendAmount", "total_spend_amount"]) {
        hint.push(format!("累计消费 {x:.2}"));
    }
    if let Some(x) = pick_f64(&data, &["frozenBalance", "frozen_balance"]) {
        if x > 0.0 {
            hint.push(format!("冻结 {x:.2}"));
        }
    }

    snap.metrics.insert(
        0,
        Metric {
            label: format!("账户余额（{}）", snap.currency),
            value: format!("{balance:.2}"),
            hint: if hint.is_empty() {
                None
            } else {
                Some(hint.join(" · "))
            },
        },
    );
    true
}

// ---------------------------------------------------------------------------
// 抓取入口
// ---------------------------------------------------------------------------

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    let key = cfg.token.clone().unwrap_or_default().trim().to_string();
    let base = cfg
        .base_url
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url().to_string());

    let mut quota_ok = false;
    // 按量付费账号没有 Coding Plan：额度接口返回「无套餐」，按「无数据」降级而不是报错
    let mut no_plan = false;
    let mut balance_ok = false;
    // 余额接口不可用时的提示（有其它数据时降级成「读取提示」）
    let mut notice: Option<String> = None;
    let auth = [("Authorization", key.as_str())];
    if key.is_empty() {
        snap.error = Some("未填写 API Key".to_string());
    } else {
        let url = quota_url(&base);
        match http.get_json_h(&url, None, None, &auth).await {
            Ok(v) => match ensure_ok(&v) {
                Ok(()) => quota_ok = apply_quota(&mut snap, &v),
                Err(e) if is_no_plan(&e) => no_plan = true,
                Err(e) => snap.error = Some(auth_error(e)),
            },
            Err(e) => snap.error = Some(auth_error(e)),
        }
    }

    // 账户余额 / 累计消费：账户汇总接口（未在官方文档列出）
    if !key.is_empty() {
        let url = account_url(&base);
        match http.get_json_h(&url, None, None, &auth).await {
            Ok(v) => match ensure_ok(&v) {
                Ok(()) => balance_ok = apply_balance(&mut snap, &v),
                Err(e) => notice = Some(e),
            },
            Err(e) => notice = Some(e),
        }
    }

    if no_plan {
        snap.metrics.insert(
            0,
            Metric {
                label: "套餐状态".to_string(),
                value: "无编程套餐（按量付费）".to_string(),
                hint: Some(
                    "额度接口（quota/limit）为 Coding Plan 专属，按量付费账号读不到套餐额度；余额与累计消费见账户汇总，平台未开放逐日用量接口"
                        .to_string(),
                ),
            },
        );
    }

    let failure = snap.error.take().or(notice);
    let has_data = quota_ok || no_plan || balance_ok;
    if has_data {
        snap.ok = true;
        if let Some(e) = failure {
            snap.metrics.push(Metric {
                label: "读取提示".to_string(),
                value: e.chars().take(60).collect(),
                hint: None,
            });
        }
    } else {
        snap.ok = false;
        snap.error = failure.or_else(|| Some("未能读取到任何数据".to_string()));
    }

    snap
}

// ---------------------------------------------------------------------------
// 连通性测试
// ---------------------------------------------------------------------------

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let key = cfg.token.clone().unwrap_or_default().trim().to_string();
    if key.is_empty() {
        return Err("请先填写 API Key".to_string());
    }
    let base = cfg
        .base_url
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url().to_string());

    let auth = [("Authorization", key.as_str())];

    // 账户余额：账户汇总接口，失败只跳过，不影响连通性判定
    let balance = http
        .get_json_h(&account_url(&base), None, None, &auth)
        .await
        .ok()
        .filter(|v| ensure_ok(v).is_ok())
        .and_then(|v| balance_of(&v));

    let v = http
        .get_json_h(&quota_url(&base), None, None, &auth)
        .await
        .map_err(auth_error)?;
    let no_plan = match ensure_ok(&v) {
        Ok(()) => false,
        // 按量付费账号没有 Coding Plan：Key 有效，只是没有可读的套餐额度
        Err(e) if is_no_plan(&e) => true,
        Err(e) => return Err(auth_error(e)),
    };

    if no_plan {
        return Ok(match balance {
            Some(b) => format!(
                "连接成功，账户余额 ¥{b:.2}；账号无编程套餐（按量付费），额度接口为 Coding Plan 专属，平台未开放逐日用量接口"
            ),
            None => "连接成功，账号无编程套餐（按量付费）：额度接口为 Coding Plan 专属，平台未开放逐日用量接口"
                .to_string(),
        });
    }

    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = balance {
        parts.push(format!("账户余额 ¥{b:.2}"));
    }

    if let Some(level) = pick_string(&data, &["level", "plan", "planName", "packageName"])
        .or_else(|| pick_string(&v, &["level", "plan"]))
    {
        parts.push(format!("套餐 {level}"));
    }

    let limits: Vec<Value> = data
        .get("limits")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let items = if limits.is_empty() {
        find_limit_array(&data)
    } else {
        limits
    };

    if let Some(item) = items
        .iter()
        .find(|i| pick_f64(i, &["unit"]) == Some(3.0))
        .or_else(|| items.first())
    {
        if let Some(p) = percent(
            pick_f64(item, &["currentValue", "current_value", "current", "used"]),
            pick_f64(item, &["usage", "limit", "total"]),
            pick_f64(item, &["percentage", "percent"]),
        ) {
            parts.push(format!("{} 已用 {p:.1}%", unit_label(item, 0)));
        }
    }

    if parts.is_empty() {
        return Err("接口连通，但未识别到额度字段".to_string());
    }
    Ok(format!("连接成功，{}", parts.join("，")))
}
