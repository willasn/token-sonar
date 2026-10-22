//! 小米 MiMo 开放平台适配器。
//!
//! 只走「网页登录」一条链路：外置窗口登录小米账号后，后端读取
//! `platform.xiaomimimo.com` 的 Cookie 会话，再访问控制台内部接口。
//!
//! | 接口 | 内容 |
//! |---|---|
//! | `GET /api/v1/balance` | 余额、充值 / 赠送 / 冻结金额、透支额度 |
//! | `GET /api/v1/usage` | 累计消费、累计 Token（含缓存 / 批量 / ASR）与联网插件调用 |
//! | `GET /api/v1/usage/bill/monthly` | 月度账单（消费 / 赠送 / 现金） |
//! | `POST /api/v1/usage/detail/list` | 逐日逐模型费用、Token 与请求数 |
//! | `POST /api/v1/usage/plugin/list` | 联网服务（网页搜索 / 解析）用量 |
//! | `GET /api/v1/tokenPlan/detail` / `/tokenPlan/usage` | Token Plan 套餐与额度 |
//! | `GET /api/v1/apiKeys` | API Key 列表 |
//! | `GET /api/v1/userProfile` | 账号信息（`idc` 决定 CNY / USD） |
//!
//! ⚠️ `/api/v1/*` 是控制台前端自用接口，无公开文档、无 SLA；
//! 所有解析都做容错，接口失败只降级为「读取提示」，不整卡报错。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::http::Http;
use crate::models::{
    AccessMode, ApiKeyInfo, DailyUsage, Metric, ModelUsage, PeriodUsage, ProviderConfig,
    QuotaWindow, UsageSnapshot,
};
use crate::providers::util::*;

const PLATFORM_BASE: &str = "https://platform.xiaomimimo.com";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

fn console_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("accept", "application/json, text/plain, */*"),
        ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
        ("user-agent", UA),
        ("referer", "https://platform.xiaomimimo.com/console/usage"),
        ("origin", PLATFORM_BASE),
    ]
}

fn console_url(path: &str) -> String {
    format!("{PLATFORM_BASE}/api/v1{path}")
}

/// 401 统一翻译成「重新授权」提示。
fn friendly(e: String) -> String {
    if e.contains("HTTP 401") || e.contains("\"code\":401") {
        "登录已失效，请在编辑账号里点击「登录获取」重新授权".to_string()
    } else {
        e
    }
}

/// 控制台接口把业务失败也包在 HTTP 200 里（`{code, message, data}`）。
fn ensure_ok(v: &Value) -> Result<(), String> {
    match v.get("code").and_then(|c| c.as_i64()) {
        None | Some(0) | Some(200) => Ok(()),
        Some(code) => {
            let msg = pick_string(v, &["message", "msg"]).unwrap_or_default();
            Err(if msg.is_empty() {
                format!("小米 MiMo 接口返回 code {code}")
            } else {
                format!("小米 MiMo 接口 code {code}: {msg}")
            })
        }
    }
}

fn payload(v: &Value) -> &Value {
    match v.get("data") {
        Some(d) if !d.is_null() => d,
        _ => v,
    }
}

fn rows(v: &Value) -> Vec<&Value> {
    let p = payload(v);
    p.as_array()
        .map(|a| a.iter().collect())
        .or_else(|| p.get("list").and_then(|l| l.as_array()).map(|a| a.iter().collect()))
        .unwrap_or_default()
}

fn cookie_of(cfg: &ProviderConfig) -> Result<String, String> {
    cfg.session()
        .ok_or_else(|| "未获取到平台登录凭证，请点击「登录获取」重新授权".to_string())
}

// ---------------------------------------------------------------------------
// 抓取入口
// ---------------------------------------------------------------------------

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    if !matches!(cfg.mode(), AccessMode::Login) {
        snap.error = Some("小米 MiMo 仅支持「登录获取」接入（官方 API Key 不提供余额 / 用量接口）".to_string());
        return snap;
    }

    let cookie = match cookie_of(cfg) {
        Ok(c) => c,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };
    let headers = console_headers();
    let mut notices: Vec<String> = Vec::new();
    let mut currency = cfg.currency.clone();
    if currency.trim().is_empty() {
        currency = "CNY".to_string();
    }

    // 1) 账号 + 币种：idc 310 = 中国大陆（CNY），其余（314 欧盟 / 海外）为 USD
    if let Ok(v) = http
        .get_json_h(&console_url("/userProfile"), None, Some(&cookie), &headers)
        .await
    {
        if ensure_ok(&v).is_ok() {
            let d = payload(&v);
            snap.account = pick_string(
                d,
                &["email", "phone", "nickname", "nickName", "userName", "userId"],
            );
            if let Some(idc) = pick_f64(d, &["idc"]) {
                currency = if idc as i64 == 310 { "CNY" } else { "USD" }.to_string();
            }
        }
    }
    snap.currency = currency.clone();

    // 2) 余额
    match http
        .get_json_h(&console_url("/balance"), None, Some(&cookie), &headers)
        .await
    {
        Ok(v) => {
            if let Err(e) = ensure_ok(&v) {
                notices.push(e);
            } else {
                apply_balance(&mut snap, payload(&v), &currency);
            }
        }
        Err(e) => notices.push(friendly(e)),
    }

    // 3) Token Plan 套餐
    let mut plan_detail = Value::Null;
    match http
        .get_json_h(&console_url("/tokenPlan/detail"), None, Some(&cookie), &headers)
        .await
    {
        Ok(v) if ensure_ok(&v).is_ok() => plan_detail = payload(&v).clone(),
        Ok(_) => {}
        Err(e) => notices.push(friendly(e)),
    }
    let mut plan_usage = Value::Null;
    match http
        .get_json_h(&console_url("/tokenPlan/usage"), None, Some(&cookie), &headers)
        .await
    {
        Ok(v) if ensure_ok(&v).is_ok() => plan_usage = payload(&v).clone(),
        Ok(_) => {}
        Err(e) => notices.push(friendly(e)),
    }
    if !plan_detail.is_null() || !plan_usage.is_null() {
        apply_plan(&mut snap, &plan_detail, &plan_usage);
    }

    // 4) 累计消费与累计 Token
    match http
        .get_json_h(&console_url("/usage"), None, Some(&cookie), &headers)
        .await
    {
        Ok(v) => {
            if let Err(e) = ensure_ok(&v) {
                notices.push(e);
            } else {
                apply_summary(&mut snap, payload(&v), &currency);
            }
        }
        Err(e) => notices.push(friendly(e)),
    }

    // 5) 逐日逐模型明细（当月 + 上月，覆盖近 30 天趋势）
    let now = chrono::Local::now();
    use chrono::Datelike;
    let (y1, m1) = (now.year(), now.month());
    let (y0, m0) = prev_month(y1, m1);

    let mut detail: Vec<Value> = Vec::new();
    for (y, m) in [(y1, m1), (y0, m0)] {
        match fetch_detail(http, &cookie, &headers, y, m).await {
            Ok(list) => detail.extend(list),
            Err(e) => {
                if (y, m) == (y1, m1) {
                    notices.push(e);
                }
            }
        }
    }
    if !detail.is_empty() {
        apply_detail(&mut snap, &detail);
        snap.supports_usage_api = true;
    }

    // 6) 月度账单（当月已出账口径）
    match http
        .get_json_h(&console_url("/usage/bill/monthly"), None, Some(&cookie), &headers)
        .await
    {
        Ok(v) if ensure_ok(&v).is_ok() => apply_monthly_bill(&mut snap, &v, (y1, m1), &currency),
        Ok(_) => {}
        Err(e) => notices.push(friendly(e)),
    }
    if let Some(m) = &snap.month {
        snap.metrics.push(Metric {
            label: format!("本月消费（{currency}）"),
            value: format!("{:.2}", m.cost),
            hint: Some(format!("{} ~ {}", m.from, m.to)),
        });
        snap.metrics.push(Metric {
            label: "本月 Token".to_string(),
            value: format!("{}", m.tokens.round() as i64),
            hint: Some(format!("{} 次请求", m.requests.round() as i64)),
        });
    }

    // 7) 联网插件用量
    match fetch_plugin(http, &cookie, &headers, y1, m1).await {
        Ok(list) => apply_plugin(&mut snap, &list, &currency),
        Err(_) => {}
    }

    // 8) API Keys（控制台只返回脱敏值）
    if let Ok(v) = http
        .get_json_h(&console_url("/apiKeys"), None, Some(&cookie), &headers)
        .await
    {
        if ensure_ok(&v).is_ok() {
            snap.api_keys = merge_keys(parse_keys(&v), cfg);
        }
    }
    if snap.api_keys.is_empty() {
        snap.api_keys = merge_keys(Vec::new(), cfg);
    }
    if !snap.api_keys.is_empty() {
        snap.metrics.push(Metric {
            label: "API Keys".to_string(),
            value: format!("{} 个", snap.api_keys.len()),
            hint: Some(format!(
                "其中 {} 个来自手动录入",
                snap.api_keys.iter().filter(|k| k.source == "manual").count()
            )),
        });
    }

    // 只要有一个维度读到数据就算成功，其余失败原因降级成「读取提示」
    let has_data = snap.balance.is_some()
        || snap.month.is_some()
        || snap.used.is_some()
        || snap.days.iter().any(|d| d.cost > 0.0 || d.tokens > 0.0);

    let mut failure = snap.error.take();
    if failure.is_none() {
        failure = notices.first().cloned();
    }

    if has_data {
        snap.ok = true;
        if let Some(e) = failure {
            snap.metrics.push(Metric {
                label: "读取提示".to_string(),
                value: truncate(&e, 60),
                hint: None,
            });
        }
    } else {
        snap.ok = false;
        snap.error = failure.or_else(|| Some("未能读取到任何数据".to_string()));
    }

    snap
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn prev_month(y: i32, m: u32) -> (i32, u32) {
    if m <= 1 {
        (y - 1, 12)
    } else {
        (y, m - 1)
    }
}

// ---------------------------------------------------------------------------
// 余额
// ---------------------------------------------------------------------------

fn apply_balance(snap: &mut UsageSnapshot, d: &Value, currency: &str) {
    let total = pick_f64(d, &["balance", "totalBalance"]);
    let cash = pick_f64(d, &["cashBalance", "cash_balance"]);
    let gift = pick_f64(d, &["giftBalance", "gift_balance"]);
    let frozen = pick_f64(d, &["frozenBalance", "frozen_balance"]);
    let overdraft = pick_f64(d, &["remainingOverdraftLimit", "overdraftLimit"]);

    snap.balance = total;
    snap.granted = gift;

    if let Some(t) = total {
        snap.metrics.insert(
            0,
            Metric {
                label: format!("余额（{currency}）"),
                value: format!("{t:.2}"),
                hint: Some("小米 MiMo 账户余额，按量付费扣费".to_string()),
            },
        );
    }
    if let Some(v) = cash {
        snap.metrics.push(Metric {
            label: "充值余额".to_string(),
            value: format!("{v:.2}"),
            hint: None,
        });
    }
    if let Some(v) = gift {
        snap.metrics.push(Metric {
            label: "赠送余额".to_string(),
            value: format!("{v:.2}"),
            hint: None,
        });
    }
    if let Some(v) = frozen {
        snap.metrics.push(Metric {
            label: "冻结金额".to_string(),
            value: format!("{v:.2}"),
            hint: None,
        });
    }
    if let Some(v) = overdraft {
        snap.metrics.push(Metric {
            label: "剩余透支额度".to_string(),
            value: format!("{v:.2}"),
            hint: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Token Plan 套餐
// ---------------------------------------------------------------------------

fn apply_plan(snap: &mut UsageSnapshot, detail: &Value, usage: &Value) {
    let plan_name = pick_string(detail, &["planName", "plan_name"])
        .or_else(|| pick_string(usage, &["planName", "plan_name"]));
    let plan_code = pick_string(detail, &["planCode", "plan_code"])
        .or_else(|| pick_string(usage, &["planCode", "plan_code"]));
    let expired = detail
        .get("expired")
        .and_then(|v| v.as_bool())
        .or_else(|| usage.get("expired").and_then(|v| v.as_bool()));
    let period_end = pick_string(
        detail,
        &["currentPeriodEnd", "current_period_end", "expireTime", "expire_time"],
    )
    .or_else(|| {
        pick_string(
            usage,
            &["currentPeriodEnd", "current_period_end", "expireTime", "expire_time"],
        )
    });

    if let Some(name) = &plan_name {
        snap.metrics.push(Metric {
            label: "Token Plan 套餐".to_string(),
            value: match &plan_code {
                Some(code) if !code.is_empty() => format!("{name}（{code}）"),
                _ => name.clone(),
            },
            hint: Some("订阅套餐与按量付费余额不互通".to_string()),
        });
    }
    if let Some(end) = &period_end {
        snap.metrics.push(Metric {
            label: "套餐到期时间".to_string(),
            value: truncate(end, 19),
            hint: None,
        });
    }
    if let Some(expired) = expired {
        snap.metrics.push(Metric {
            label: "套餐状态".to_string(),
            value: if expired { "已结束" } else { "生效中" }.to_string(),
            hint: None,
        });
    }

    let usage_obj = usage
        .get("usage")
        .cloned()
        .or_else(|| detail.get("usage").cloned())
        .unwrap_or(Value::Null);
    let items = usage_obj
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for item in items {
        let name = pick_string(&item, &["name"]).unwrap_or_else(|| "额度".to_string());
        let used = pick_f64(&item, &["used"]).unwrap_or(0.0);
        let limit = pick_f64(&item, &["limit", "quota", "total"]).unwrap_or(0.0);
        let percent = pick_f64(&item, &["percent"])
            .unwrap_or_else(|| if limit > 0.0 { used / limit * 100.0 } else { 0.0 });
        snap.quotas.push(QuotaWindow {
            label: format!("Token Plan · {}", plan_item_label(&name)),
            used,
            quota: limit,
            percent,
            reset_at: period_end.clone(),
            reset_in: None,
        });
    }
}

fn plan_item_label(name: &str) -> String {
    match name {
        "compensation_total_token" => "补偿额度".to_string(),
        "total_token" => "套餐额度".to_string(),
        other => other.replace('_', " "),
    }
}

// ---------------------------------------------------------------------------
// 累计用量
// ---------------------------------------------------------------------------

fn apply_summary(snap: &mut UsageSnapshot, d: &Value, currency: &str) {
    let cost = d
        .get("costUsage")
        .and_then(|c| pick_f64(c, &["totalCost", "total_cost"]));
    let token_usage = d.get("tokenUsage").cloned().unwrap_or(Value::Null);
    let tokens = pick_f64(&token_usage, &["totalToken", "total_token"]);
    let cache = pick_f64(&token_usage, &["cacheToken", "cache_token"]);
    let input = pick_f64(&token_usage, &["inputToken", "input_token"]);
    let output = pick_f64(&token_usage, &["outputToken", "output_token"]);
    let batch = pick_f64(
        &token_usage,
        &["batchInputToken", "batch_input_token", "batchOutputToken"],
    );
    let audio = pick_f64(
        &token_usage,
        &["inputAudioDuration", "input_audio_duration"],
    );

    if let Some(c) = cost {
        snap.used = Some(c);
        snap.metrics.push(Metric {
            label: format!("平台累计消费（{currency}）"),
            value: format!("{c:.2}"),
            hint: Some("控制台「账单明细」累计值".to_string()),
        });
    }
    if let Some(t) = tokens {
        snap.tokens_total = Some(t);
        snap.metrics.push(Metric {
            label: "平台累计 Token".to_string(),
            value: format!("{}", t.round() as i64),
            hint: Some("含缓存输入、批量推理与 ASR".to_string()),
        });
    }
    if let (Some(inp), Some(cache)) = (input, cache) {
        snap.metrics.push(Metric {
            label: "累计缓存输入".to_string(),
            value: format!("{}", cache.round() as i64),
            hint: Some(format!("输入合计 {}", inp.round() as i64)),
        });
    }
    if let Some(o) = output {
        snap.metrics.push(Metric {
            label: "累计输出 Token".to_string(),
            value: format!("{}", o.round() as i64),
            hint: None,
        });
    }
    if let Some(b) = batch {
        snap.metrics.push(Metric {
            label: "累计批量推理 Token".to_string(),
            value: format!("{}", b.round() as i64),
            hint: None,
        });
    }
    if let Some(sec) = audio {
        snap.metrics.push(Metric {
            label: "累计 ASR 时长".to_string(),
            value: format!("{:.1} 小时", sec / 3600.0),
            hint: None,
        });
    }
    let plugin = d.get("pluginUsage").cloned().unwrap_or(Value::Null);
    let plugin_total = pick_f64(&plugin, &["totalRequestCount", "total_request_count"]);
    let web_search = pick_f64(
        &plugin,
        &["webSearchRequestCount", "web_search_request_count"],
    );
    if let Some(p) = plugin_total {
        snap.metrics.push(Metric {
            label: "联网服务调用".to_string(),
            value: format!("{} 次", p.round() as i64),
            hint: web_search.map(|w| format!("其中网页搜索 {} 次", w.round() as i64)),
        });
    }
}

// ---------------------------------------------------------------------------
// 逐日逐模型明细
// ---------------------------------------------------------------------------

async fn fetch_detail(
    http: &Http,
    cookie: &str,
    headers: &[(&'static str, &'static str)],
    year: i32,
    month: u32,
) -> Result<Vec<Value>, String> {
    let body = json!({ "year": year, "month": month });
    let (v, _) = http
        .post_json_h(
            &console_url("/usage/detail/list"),
            &body,
            None,
            Some(cookie),
            headers,
        )
        .await
        .map_err(friendly)?;
    ensure_ok(&v)?;
    Ok(rows(&v).into_iter().cloned().collect())
}

fn detail_date(r: &Value) -> Option<String> {
    match r.get("date") {
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.len() >= 10 {
                Some(s[..10].to_string())
            } else if s.len() >= 7 {
                Some(format!("{}-01", &s[..7]))
            } else {
                None
            }
        }
        Some(Value::Number(n)) => n.as_i64().and_then(ts_to_date),
        _ => record_date(r),
    }
}

fn apply_detail(snap: &mut UsageSnapshot, list: &[Value]) {
    let window_start = recent_dates(crate::providers::WINDOW_DAYS)
        .first()
        .cloned()
        .unwrap_or_default();

    let mut days: BTreeMap<String, DailyUsage> = BTreeMap::new();
    let mut models: BTreeMap<String, ModelUsage> = BTreeMap::new();
    let mut month_cost = 0.0;
    let mut month_tokens = 0.0;
    let mut month_requests = 0.0;
    let month_key = today_str()[..7].to_string();

    for r in list {
        let Some(date) = detail_date(r) else { continue };
        let cost = pick_f64(r, &["consumedAmount", "consumed_amount"]).unwrap_or(0.0);
        let tokens = pick_f64(r, &["totalToken", "total_token"])
            .or_else(|| {
                let hit = pick_f64(r, &["inputHitToken", "input_hit_token"]).unwrap_or(0.0);
                let miss = pick_f64(r, &["inputMissToken", "input_miss_token"]).unwrap_or(0.0);
                let out = pick_f64(r, &["outputToken", "output_token"]).unwrap_or(0.0);
                if hit + miss + out > 0.0 {
                    Some(hit + miss + out)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let requests = pick_f64(r, &["requestCount", "request_count"]).unwrap_or(0.0);

        if !window_start.is_empty() && date < window_start {
            continue;
        }

        let day = days.entry(date.clone()).or_insert(DailyUsage {
            date: date.clone(),
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        day.cost += cost;
        day.tokens += tokens;
        day.requests += requests;

        if date.starts_with(&month_key) {
            month_cost += cost;
            month_tokens += tokens;
            month_requests += requests;
        }

        if let Some(name) = pick_string(r, &["model", "modelName", "model_name"]) {
            let m = models.entry(name.clone()).or_insert(ModelUsage {
                model: name,
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
            m.requests += requests;
            m.tokens += tokens;
            m.cost += cost;
        }
    }

    snap.days = days.into_values().collect();
    snap.models = models.into_values().collect();
    snap.models.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 仅当平台账单不可用时，才用明细合计兜底本月
    if snap.month.is_none() && (month_cost > 0.0 || month_tokens > 0.0) {
        snap.month = Some(PeriodUsage {
            requests: month_requests,
            tokens: month_tokens,
            cost: month_cost,
            from: format!("{month_key}-01"),
            to: today_str(),
        });
    }

    let today = today_str();
    snap.today = snap
        .days
        .iter()
        .find(|d| d.date == today)
        .map(|d| PeriodUsage {
            requests: d.requests,
            tokens: d.tokens,
            cost: d.cost,
            from: today.clone(),
            to: today,
        });
}

// ---------------------------------------------------------------------------
// 月度账单与插件用量
// ---------------------------------------------------------------------------

fn month_key(v: &Value) -> Option<String> {
    let s = pick_string(v, &["reportMonth", "report_month"])?;
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 6 {
        Some(digits[..6].to_string())
    } else {
        None
    }
}

fn apply_monthly_bill(snap: &mut UsageSnapshot, v: &Value, year_month: (i32, u32), currency: &str) {
    let key = format!("{:04}{:02}", year_month.0, year_month.1);
    let Some(row) = rows(v).into_iter().find(|r| month_key(r).as_deref() == Some(&key)) else {
        return;
    };
    let cost = pick_f64(row, &["consumptionAmount", "consumption_amount"]).unwrap_or(0.0);
    let gift = pick_f64(row, &["giftConsumption", "gift_consumption"]).unwrap_or(0.0);
    let cash = pick_f64(row, &["cashConsumption", "cash_consumption"]).unwrap_or(0.0);

    snap.month = Some(PeriodUsage {
        requests: snap.month.as_ref().map(|m| m.requests).unwrap_or(0.0),
        tokens: snap.month.as_ref().map(|m| m.tokens).unwrap_or(0.0),
        cost,
        from: format!("{:04}-{:02}-01", year_month.0, year_month.1),
        to: today_str(),
    });
    snap.metrics.push(Metric {
        label: format!("本月账单（{currency}）"),
        value: format!("{cost:.2}"),
        hint: Some(format!("赠送抵扣 {gift:.2} · 现金 {cash:.2}")),
    });
}

async fn fetch_plugin(
    http: &Http,
    cookie: &str,
    headers: &[(&'static str, &'static str)],
    year: i32,
    month: u32,
) -> Result<Vec<Value>, String> {
    let body = json!({ "year": year, "month": month });
    let (v, _) = http
        .post_json_h(
            &console_url("/usage/plugin/list"),
            &body,
            None,
            Some(cookie),
            headers,
        )
        .await
        .map_err(friendly)?;
    ensure_ok(&v)?;
    Ok(rows(&v).into_iter().cloned().collect())
}

fn apply_plugin(snap: &mut UsageSnapshot, list: &[Value], currency: &str) {
    let mut cost = 0.0;
    let mut requests = 0.0;
    let mut web_search = 0.0;
    for r in list {
        cost += pick_f64(r, &["consumedAmount", "consumed_amount"]).unwrap_or(0.0);
        requests += pick_f64(r, &["requestCount", "request_count"]).unwrap_or(0.0);
        if pick_string(r, &["plugin"]).as_deref() == Some("web_search") {
            web_search += pick_f64(r, &["requestCount", "request_count"]).unwrap_or(0.0);
        }
    }
    if cost > 0.0 || requests > 0.0 {
        snap.metrics.push(Metric {
            label: format!("本月联网服务（{currency}）"),
            value: format!("{cost:.2}"),
            hint: Some(format!(
                "{} 次调用，其中网页搜索 {} 次",
                requests.round() as i64,
                web_search.round() as i64
            )),
        });
    }
}

// ---------------------------------------------------------------------------
// API Keys
// ---------------------------------------------------------------------------

fn parse_keys(v: &Value) -> Vec<ApiKeyInfo> {
    rows(v)
        .into_iter()
        .filter_map(|r| {
            let name = pick_string(r, &["name", "keyName", "key_name", "title", "remark"])?;
            let id = pick_string(r, &["id", "keyId", "key_id"]).unwrap_or_else(|| name.clone());
            let raw = pick_string(
                r,
                &["maskedKey", "masked_key", "key", "apiKey", "api_key", "prefix"],
            );
            Some(ApiKeyInfo {
                id,
                name,
                key_type: Some("api".to_string()),
                masked: raw.as_deref().map(mask_secret),
                secret: None,
                created_at: pick_string(r, &["createdAt", "created_at", "createTime"]),
                last_used_at: pick_string(r, &["lastUsedAt", "last_used_at", "lastUseTime"]),
                status: pick_string(r, &["status", "state"]).or(Some("正常".to_string())),
                source: "api".to_string(),
            })
        })
        .collect()
}

fn mask_secret(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return "—".to_string();
    }
    if s.contains('*') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}****{tail}")
}

fn merge_keys(remote: Vec<ApiKeyInfo>, cfg: &ProviderConfig) -> Vec<ApiKeyInfo> {
    let mut out: Vec<ApiKeyInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for k in cfg.api_keys.clone().unwrap_or_default() {
        let name = k.name.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        let mut k = k;
        k.name = name;
        k.source = "manual".to_string();
        if k.masked.clone().unwrap_or_default().trim().is_empty() {
            k.masked = k.secret.as_deref().map(mask_secret);
        }
        k.secret = None;
        out.push(k);
    }
    for k in remote {
        let name = k.name.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name);
        out.push(k);
    }
    out
}

// ---------------------------------------------------------------------------
// 连通性测试 / 登录校验
// ---------------------------------------------------------------------------

/// 登录窗口回传 Cookie 后校验：能读回账户余额才算有效。
pub async fn verify_cookie(http: &Http, cookie: &str) -> Result<(), String> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err("Cookie 为空".to_string());
    }
    let v = http
        .get_json_h(
            &console_url("/balance"),
            None,
            Some(cookie),
            &console_headers(),
        )
        .await
        .map_err(friendly)?;
    ensure_ok(&v)
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    if !matches!(cfg.mode(), AccessMode::Login) {
        return Err("小米 MiMo 仅支持「登录获取」接入".to_string());
    }
    let cookie = cookie_of(cfg)?;
    let v = http
        .get_json_h(
            &console_url("/balance"),
            None,
            Some(&cookie),
            &console_headers(),
        )
        .await
        .map_err(friendly)?;
    ensure_ok(&v)?;
    let d = payload(&v);
    let balance = pick_f64(d, &["balance", "totalBalance"]).unwrap_or(0.0);
    let currency = cfg.currency.trim();
    let currency = if currency.is_empty() { "CNY" } else { currency };
    Ok(format!("连接成功，账户余额 {currency} {balance:.2}"))
}
