//! 火山方舟（火山引擎）适配器。
//!
//! 只有 `api` 一种接入方式：填入火山引擎的 Access Key（AK/SK），
//! 用 Signature V4（HMAC-SHA256）签名调用**管控面** OpenAPI，读取：
//!
//! | 数据 | 服务 | 接口 |
//! |---|---|---|
//! | 账户余额（可用 / 现金 / 冻结 / 欠费 / 信控） | 费用中心 `billing` | `QueryBalanceAcct` |
//! | Coding Plan / Agent Plan 套餐档位与有效期 | 方舟 `ark` | `GetPersonalPlan` |
//! | 套餐用量限额（近 5 小时 / 近一周 / 近一月） | 方舟 `ark` | `GetAFPUsage`（Agent Plan）/ `GetCodingPlanUsage`（Coding Plan） |
//! | 已开通模型 | 方舟 `ark` | `ListModelActivations` |
//! | Coding Plan 支持的模型 | 方舟 `ark` | `ListArkCodingPlanModel` |
//!
//! 管控面接口**只支持 AK/SK 签名鉴权**，方舟的 API Key 走不通；
//! 签名与服务端一致的部分是「派生密钥链用原始二进制摘要」，而非十六进制串。
//! 默认项目按量推理用量来自 GetInferenceUsage；费用独立来自费用中心已出账账单。
//!
//! 站点地址只用来推导签名地域（`https://ark.<region>.volcengineapi.com`），
//! 不带地域时按 `cn-beijing` 处理。

use std::collections::BTreeMap;

use chrono::{FixedOffset, Utc};
use serde_json::{json, Value};

use crate::http::Http;
use crate::models::{
    DailyUsage, Metric, ModelUsage, PeriodUsage, ProviderConfig, QuotaWindow, UsageSnapshot,
};
use crate::providers::util::*;

/// 方舟管控面（套餐 / 模型）服务地址，签名 service = `ark`
const ARK_HOST: &str = "ark.cn-beijing.volcengineapi.com";
/// 费用中心（账户余额）服务地址，签名 service = `billing`
const BILLING_HOST: &str = "billing.volcengineapi.com";
/// 方舟管控面版本
const ARK_VERSION: &str = "2024-01-01";
/// 费用中心版本
const BILLING_VERSION: &str = "2022-01-01";
/// 默认签名地域
const DEFAULT_REGION: &str = "cn-beijing";

// ---------------------------------------------------------------------------
// Signature V4
// ---------------------------------------------------------------------------

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

fn hmac_sha256(key: &[u8], msg: &str) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(msg.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `X-Date`：ISO 8601 的 UTC 时间，精度到秒。
fn utc_stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// 计算 `Authorization` 头。请求体为空时 `body_hash` 为 SHA256("")。
#[allow(clippy::too_many_arguments)]
fn authorization(
    method: &str,
    host: &str,
    query: &str,
    body_hash: &str,
    x_date: &str,
    region: &str,
    service: &str,
    ak: &str,
    sk: &str,
) -> String {
    // 参与签名的头必须与 CanonicalHeaders 中的顺序一致（按名称 ASCII 升序）
    const SIGNED: &str = "content-type;host;x-content-sha256;x-date";

    let canonical_headers = format!(
        "content-type:application/json\nhost:{host}\nx-content-sha256:{body_hash}\nx-date:{x_date}\n"
    );
    let canonical_request =
        format!("{method}\n/\n{query}\n{canonical_headers}\n{SIGNED}\n{body_hash}");

    // X-Date 前 8 位即日期（YYYYMMDD）
    let date = &x_date[..8];
    let scope = format!("{date}/{region}/{service}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    // 派生签名密钥：链路上使用原始二进制摘要
    let k_date = hmac_sha256(sk.as_bytes(), date);
    let k_region = hmac_sha256(&k_date, region);
    let k_service = hmac_sha256(&k_region, service);
    let k_signing = hmac_sha256(&k_service, "request");
    let signature = hex(&hmac_sha256(&k_signing, &string_to_sign));

    format!("HMAC-SHA256 Credential={ak}/{scope}, SignedHeaders={SIGNED}, Signature={signature}")
}

struct Creds<'a> {
    ak: &'a str,
    sk: &'a str,
    region: String,
}

/// 调用一次 OpenAPI：`service` 决定签名用途，`body` 为 `None` 时走 GET。
async fn call(
    http: &Http,
    creds: &Creds<'_>,
    host: &str,
    service: &str,
    action: &str,
    version: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let query = format!("Action={action}&Version={version}");
    let url = format!("https://{host}/?{query}");
    let payload = match body {
        Some(b) => serde_json::to_string(b).map_err(|e| format!("序列化请求体失败: {e}"))?,
        None => String::new(),
    };
    let body_hash = sha256_hex(payload.as_bytes());
    let x_date = utc_stamp();
    let post = body.is_some();
    let method = if post { "POST" } else { "GET" };

    let auth = authorization(
        method,
        host,
        &query,
        &body_hash,
        &x_date,
        &creds.region,
        service,
        creds.ak,
        creds.sk,
    );

    let extra = [
        ("x-date", x_date.as_str()),
        ("x-content-sha256", body_hash.as_str()),
        ("authorization", auth.as_str()),
    ];
    let req_method = if post {
        reqwest::Method::POST
    } else {
        reqwest::Method::GET
    };

    let (status, v) = http.request_json(req_method, &url, body, &extra).await?;
    ensure_ok(status, &v)?;
    Ok(v)
}

/// 业务错误放在 `ResponseMetadata.Error` 里（配合 4xx / 5xx 状态码返回）。
fn ensure_ok(status: u16, v: &Value) -> Result<(), String> {
    let Some(err) = v.get("ResponseMetadata").and_then(|m| m.get("Error")) else {
        return if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(format!("HTTP {status}"))
        };
    };
    let code = pick_string(err, &["Code"]).unwrap_or_else(|| "Error".to_string());
    let message = pick_string(err, &["Message"]).unwrap_or_default();
    Err(if message.is_empty() {
        format!("HTTP {status} [{code}]")
    } else {
        format!("HTTP {status} [{code}] {message}")
    })
}

/// 未订阅套餐：接口返回 404 `ResourceNotFound.Plan`，凭证本身是有效的。
fn is_missing_plan(e: &str) -> bool {
    e.contains("ResourceNotFound.Plan")
        || e.contains("does not have an active or existing personal plan")
}

fn is_auth_error(e: &str) -> bool {
    e.contains("AccessDenied")
        || e.contains("InvalidAccessKey")
        || e.contains("SignatureDoesNotMatch")
        || e.contains("MissingAuthenticationToken")
        || e.contains("Authentication")
        || e.contains("HTTP 401")
        || e.contains("HTTP 403")
}

/// 鉴权 / 权限类错误翻译成可操作的中文提示。
fn auth_hint(e: String) -> String {
    if is_auth_error(&e) {
        format!("AK/SK 校验失败（{e}），请确认填的是火山引擎访问密钥，且该密钥具备费用中心与方舟的只读权限")
    } else {
        e
    }
}

// ---------------------------------------------------------------------------
// 站点地址 → host / 地域
// ---------------------------------------------------------------------------

fn base_or_default(cfg: &ProviderConfig) -> String {
    cfg.base_url
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url().to_string())
}

fn host_of(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let b = b.split("://").last().unwrap_or(b);
    b.split('/').next().unwrap_or(b).to_string()
}

/// `https://ark.cn-beijing.volcengineapi.com` → `cn-beijing`；识别不出时用默认地域。
fn region_of(base: &str) -> String {
    let host = host_of(base);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 3 && parts[0] == "ark" && !parts[1].is_empty() {
        parts[1].to_string()
    } else {
        DEFAULT_REGION.to_string()
    }
}

// ---------------------------------------------------------------------------
// 各数据来源 → 指标
// ---------------------------------------------------------------------------

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn short_date(s: Option<String>) -> Option<String> {
    let s = s?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(10).collect())
    }
}

/// Unix 毫秒时间戳 → 本地时间字符串（yyyy-mm-dd HH:MM）。
fn ms_to_datetime(ms: i64) -> Option<String> {
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(ms / 1000, 0)?;
    Some(
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    )
}

/// 距离重置的相对时间，例如「4小时26分后重置」。
fn reset_in_text(ms: i64) -> String {
    let diff = ms - chrono::Utc::now().timestamp_millis();
    if diff <= 0 {
        return "即将重置".to_string();
    }
    let mins = diff / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    if h >= 24 {
        format!("{}天{}小时后重置", h / 24, h % 24)
    } else if h > 0 {
        format!("{h}小时{m}分后重置")
    } else {
        format!("{m}分后重置")
    }
}

fn result_of(v: &Value) -> Value {
    v.get("Result").cloned().unwrap_or(Value::Null)
}

/// `QueryBalanceAcct` → 可用余额 / 现金 / 冻结 / 欠费 / 信控额度。
fn apply_balance(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let r = result_of(v);
    let Some(available) = pick_f64(&r, &["AvailableBalance"]) else {
        return false;
    };

    snap.currency = "CNY".to_string();
    snap.balance = Some(available);
    if let Some(id) = pick_f64(&r, &["AccountID"]) {
        snap.account = Some((id as i64).to_string());
    }

    let mut hint: Vec<String> = Vec::new();
    if let Some(x) = pick_f64(&r, &["CashBalance"]) {
        hint.push(format!("现金 {x:.2}"));
    }
    if let Some(x) = pick_f64(&r, &["ArrearsBalance"]) {
        if x > 0.0 {
            hint.push(format!("欠费 {x:.2}"));
        }
    }
    if let Some(x) = pick_f64(&r, &["FreezeAmount"]) {
        if x > 0.0 {
            hint.push(format!("冻结 {x:.2}"));
        }
    }
    if let Some(x) = pick_f64(&r, &["CreditLimit"]) {
        if x > 0.0 {
            hint.push(format!("信控额度 {x:.2}"));
        }
    }

    snap.metrics.insert(
        0,
        Metric {
            label: "账户余额（CNY）".to_string(),
            value: format!("{available:.2}"),
            hint: if hint.is_empty() {
                None
            } else {
                Some(hint.join(" · "))
            },
        },
    );
    true
}

/// `GetPersonalPlan` → 套餐档位 / 状态 / 有效期 / 自动续费。
fn apply_plan(snap: &mut UsageSnapshot, plan: &str, v: &Value) -> bool {
    let r = result_of(v);
    let Some(tier) = pick_string(&r, &["PlanType"]) else {
        return false;
    };

    let status = match pick_string(&r, &["Status"]).unwrap_or_default().as_str() {
        "Running" => "生效中".to_string(),
        "Expired" => "已过期".to_string(),
        "" => "状态未知".to_string(),
        other => other.to_string(),
    };

    let mut hint: Vec<String> = Vec::new();
    if pick(&r, &["AutoRenew"]).and_then(|x| x.as_bool()) == Some(true) {
        hint.push("已开启自动续费".to_string());
    }

    let label = if plan == "CodingPlan" {
        "Coding Plan"
    } else {
        "Agent Plan"
    };
    snap.metrics.push(Metric {
        label: label.to_string(),
        value: format!("{tier} · {status}"),
        hint: if hint.is_empty() {
            None
        } else {
            Some(hint.join(" · "))
        },
    });

    // 开始时间 / 结束时间单独成列，方便一眼看清订阅周期
    snap.metrics.push(Metric {
        label: format!("{label} 开始时间"),
        value: short_date(pick_string(&r, &["StartTime"])).unwrap_or_else(|| "—".to_string()),
        hint: None,
    });
    snap.metrics.push(Metric {
        label: format!("{label} 结束时间"),
        value: short_date(pick_string(&r, &["EndTime"])).unwrap_or_else(|| "—".to_string()),
        hint: None,
    });
    true
}

/// `GetAFPUsage` → 套餐用量限额（近 5 小时 / 近一周 / 近一月滚动窗口）。
///
/// 返回体里每个窗口含 `Quota` / `Used`（AFP 单位）与 `ResetTime`（Unix 毫秒），
/// 这里换算成使用率与「多久后重置」两项展示信息。
fn apply_afp_usage(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let r = result_of(v);
    let windows = [
        ("AFPFiveHour", "近 5 小时用量"),
        ("AFPWeekly", "近一周用量"),
        ("AFPMonthly", "近一月用量"),
    ];

    let mut found = false;
    for (key, label) in windows {
        let Some(w) = r.get(key) else { continue };
        let Some(quota) = pick_f64(w, &["Quota"]) else {
            continue;
        };
        if quota <= 0.0 {
            continue;
        }
        let used = pick_f64(w, &["Used"]).unwrap_or(0.0);
        let reset_ms = pick_f64(w, &["ResetTime"]).map(|x| x as i64);

        snap.quotas.push(QuotaWindow {
            label: label.to_string(),
            used,
            quota,
            percent: (used / quota * 100.0).clamp(0.0, 100.0),
            reset_at: reset_ms.and_then(ms_to_datetime),
            reset_in: reset_ms.map(reset_in_text),
        });
        found = true;
    }
    found
}

/// `GetCodingPlanUsage` → Coding Plan（个人版）套餐用量：近 5 小时 / 近一周 / 近一月使用率。
///
/// Coding Plan 没有 AFP 额度（`GetAFPUsage` 只适用 Agent Plan，对 Coding Plan 账号
/// 返回全 0 窗口），该接口返回 `Result.QuotaUsage[]`，每项：
/// `Level`（`session` / `weekly` / `monthly`）、`Percent`（0 ~ 100）、`ResetTimestamp`（Unix 秒）。
fn apply_coding_plan_usage(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let items = result_of(v)
        .get("QuotaUsage")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let mut found = false;
    for item in items {
        let Some(level) = pick_string(&item, &["Level"]) else {
            continue;
        };
        let label = match level.as_str() {
            "session" => "近 5 小时用量",
            "weekly" => "近一周用量",
            "monthly" => "近一月用量",
            _ => continue,
        };
        let Some(pct) = pick_f64(&item, &["Percent"]) else {
            continue;
        };
        // ResetTimestamp 是 Unix 秒，统一换算成毫秒
        let reset_ms = pick_f64(&item, &["ResetTimestamp"])
            .map(|x| (x * 1000.0) as i64)
            .filter(|x| *x > 0);

        snap.quotas.push(QuotaWindow {
            label: label.to_string(),
            used: pct,
            quota: 100.0,
            percent: pct.clamp(0.0, 100.0),
            reset_at: reset_ms.and_then(ms_to_datetime),
            reset_in: reset_ms.map(reset_in_text),
        });
        found = true;
    }
    found
}

/// `ListModelActivations` → 已开通模型数量与名称。
fn apply_activated_models(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let r = result_of(v);
    let items = r
        .get("Items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let total = pick_f64(&r, &["TotalCount"]).unwrap_or(items.len() as f64);
    if items.is_empty() && total <= 0.0 {
        return false;
    }

    let names: Vec<String> = items
        .iter()
        .filter_map(|i| pick_string(i, &["DisplayName", "FoundationModelName"]))
        .collect();

    snap.metrics.push(Metric {
        label: "已开通模型".to_string(),
        value: format!("{} 个", total as i64),
        hint: if names.is_empty() {
            None
        } else {
            Some(format!("含 {}", names.join("、")))
        },
    });
    true
}

/// `ListArkCodingPlanModel` → Coding Plan 支持的模型数量。
fn apply_coding_models(snap: &mut UsageSnapshot, v: &Value) -> bool {
    let datas = result_of(v)
        .get("Datas")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    if datas.is_empty() {
        return false;
    }

    let names: Vec<String> = datas
        .iter()
        .filter_map(|d| pick_string(d, &["ModelID"]))
        .collect();

    snap.metrics.push(Metric {
        label: "Coding Plan 可用模型".to_string(),
        value: format!("{} 个", names.len()),
        hint: Some(names.join("、")),
    });
    true
}

// ---------------------------------------------------------------------------
// 按量推理与已出账费用（独立数据源）
// ---------------------------------------------------------------------------

const BILL_DETAIL_HOST: &str = "open.volcengineapi.com";
const BILL_DETAIL_REGION: &str = "cn-north-1";
const BILL_PAGE_SIZE: usize = 300;
const BILL_MAX_PAGES: usize = 100;
const UNKNOWN: f64 = -1.0;

struct UsageRange {
    start: String,
    month: String,
    today: String,
}

fn usage_range(today: chrono::NaiveDate) -> UsageRange {
    let month = today.format("%Y-%m-01").to_string();
    let recent = (today - chrono::Duration::days(29)).to_string();
    UsageRange {
        start: recent.min(month.clone()),
        month,
        today: today.to_string(),
    }
}

fn inference_body(range: &UsageRange) -> Value {
    json!({"StartTime":range.start,"EndTime":range.today,"QueryInterval":"Day",
        "ProjectName":"default","Filters":[
            {"Key":"BillingStatus","Values":["normal"]},
            {"Key":"ModelName","Values":[]}]})
}

fn finite_number(v: Option<&Value>, nonnegative: bool) -> Result<f64, String> {
    let n = v
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|n| n.is_finite() && (!nonnegative || *n >= 0.0));
    n.ok_or_else(|| "缺失或无效数值字段".to_string())
}

fn checked_date(value: Option<&Value>) -> Result<String, String> {
    let s = value.and_then(Value::as_str).ok_or("缺失日期")?;
    let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "日期格式错误")?;
    if date.to_string() != s {
        return Err("日期格式错误".to_string());
    }
    Ok(s.to_string())
}

#[derive(Default)]
struct InferenceUsage {
    days: BTreeMap<String, DailyUsage>,
    models: BTreeMap<String, ModelUsage>,
    input: f64,
    output: f64,
}

fn parse_inference(v: &Value, range: &UsageRange) -> Result<InferenceUsage, String> {
    let r = v.get("Result").ok_or("缺失 Result")?;
    let fields = r
        .get("Fields")
        .and_then(Value::as_array)
        .ok_or("缺失 Fields 列定义")?;
    let data = r
        .get("Data")
        .and_then(Value::as_array)
        .ok_or("缺失 Data 行数组")?;
    let mut indices = BTreeMap::new();
    for (i, f) in fields.iter().enumerate() {
        let name = f.get("Name").and_then(Value::as_str).ok_or("无效列名")?;
        if indices.insert(name, i).is_some() {
            return Err("重复列名".to_string());
        }
    }
    if finite_number(r.get("DataCount"), true)? != data.len() as f64 {
        return Err("DataCount 与返回行数不一致".to_string());
    }
    let mut usage = InferenceUsage::default();
    for row in data {
        let row = row
            .as_array()
            .filter(|row| row.len() == fields.len())
            .ok_or("用量行与列定义不匹配")?;
        let get = |name: &str| indices.get(name).and_then(|i| row.get(*i));
        let day = checked_date(get("Day"))?;
        if day < range.start || day > range.today {
            return Err("用量日期超出查询范围".to_string());
        }
        if let Some(status) = get("BillingStatus") {
            if status.as_str() != Some("normal") {
                return Err("返回非正常按量计费用量".to_string());
            }
        }
        let input = finite_number(get("InputTokens"), true)?;
        let output = finite_number(get("OutputTokens"), true)?;
        let tokens = if indices.contains_key("TotalTokens") {
            finite_number(get("TotalTokens"), true)?
        } else {
            input + output
        };
        let requests = finite_number(get("ReqCnt"), true)?;
        let model = get("ModelName")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("缺失模型名称")?;
        let d = usage.days.entry(day.clone()).or_insert(DailyUsage {
            date: day.clone(),
            requests: 0.0,
            tokens: 0.0,
            cost: UNKNOWN,
        });
        d.requests += requests;
        d.tokens += tokens;
        if day >= range.month {
            usage.input += input;
            usage.output += output;
            let m = usage.models.entry(model.to_string()).or_insert(ModelUsage {
                model: model.to_string(),
                requests: 0.0,
                tokens: 0.0,
                cost: UNKNOWN,
            });
            m.requests += requests;
            m.tokens += tokens;
        }
        if !tokens.is_finite()
            || !d.tokens.is_finite()
            || !d.requests.is_finite()
            || !usage.input.is_finite()
            || !usage.output.is_finite()
        {
            return Err("用量合计超出有效范围".to_string());
        }
    }
    Ok(usage)
}

#[derive(Default)]
struct BillUsage {
    days: BTreeMap<String, f64>,
    seen: std::collections::BTreeSet<String>,
    pages: std::collections::BTreeSet<String>,
    currency: Option<String>,
    total: Option<usize>,
    complete: bool,
    readable: bool,
}

fn bill_body(period: &str, offset: usize) -> Value {
    json!({"BillPeriod":period,"Limit":BILL_PAGE_SIZE,"Offset":offset,"NeedRecordNum":1,
        "IgnoreZero":0,"GroupTerm":0,"GroupPeriod":1,"BillingMode":["2"],
        "BillCategory":["consume-use"]})
}

fn parse_bill_page(
    v: &Value,
    period: &str,
    offset: usize,
    usage: &mut BillUsage,
) -> Result<bool, String> {
    let r = v.get("Result").ok_or("缺失账单 Result")?;
    let rows = r
        .get("List")
        .and_then(Value::as_array)
        .ok_or("缺失账单 List")?;
    let total = r
        .get("Total")
        .and_then(Value::as_u64)
        .ok_or("缺失账单总数，无法确认分页完整性")? as usize;
    if r.get("Offset").and_then(Value::as_u64) != Some(offset as u64) || rows.len() > BILL_PAGE_SIZE
    {
        return Err("账单分页边界不一致".to_string());
    }
    if usage.total.is_some_and(|previous| previous != total) {
        return Err("分页期间账单总数变化".to_string());
    }
    if offset + rows.len() > total || (rows.len() < BILL_PAGE_SIZE && offset + rows.len() < total) {
        return Err("账单分页提前结束".to_string());
    }
    if !rows.is_empty()
        && !usage.pages.insert(sha256_hex(
            Value::Array(rows.clone()).to_string().as_bytes(),
        ))
    {
        return Err("账单重复分页，已停止读取".to_string());
    }
    usage.total = Some(total);
    for row in rows {
        if !row.is_object() {
            return Err("无效账单行".to_string());
        }
        let product = row
            .get("ProductZh")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("账单缺失产品名称，无法筛选方舟")?;
        if product != "火山方舟" {
            continue;
        }
        let mode = Some(
            row.get("BillingMode")
                .and_then(Value::as_str)
                .ok_or("方舟账单缺失计费模式")?,
        );
        if !matches!(mode, Some("2" | "按量计费")) {
            continue;
        }
        let category = Some(
            row.get("BillCategory")
                .and_then(Value::as_str)
                .ok_or("方舟账单缺失账单类型")?,
        );
        if !matches!(category, Some("consume-use" | "消费-使用")) {
            continue;
        }
        let details = ["ConfigName", "Element", "Product"]
            .iter()
            .filter_map(|key| row.get(*key).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let plan_name: String = details
            .chars()
            .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '_'))
            .collect();
        if ["codingplan", "agentplan", "套餐"]
            .iter()
            .any(|term| plan_name.contains(term))
        {
            continue;
        }
        // 单次按天、计费项聚合结果按明细 ID 去重；官方未定义结算类型为重复层。
        let day = checked_date(row.get("ExpenseDate"))?;
        if !day.starts_with(period) {
            return Err("账单日期与账期不一致".to_string());
        }
        let currency = row
            .get("Currency")
            .and_then(Value::as_str)
            .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or("账单缺失有效定价币种")?;
        if currency != "CNY" {
            return Err("定价币种不是 CNY，无法与账户余额共用货币显示".to_string());
        }
        if usage.currency.as_deref().is_some_and(|c| c != currency) {
            return Err("账单存在混合币种，不能合计".to_string());
        }
        let amount = finite_number(row.get("PayableAmount"), true)?;
        let key = row
            .get("BillDetailId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("方舟账单缺失明细 ID，无法可靠去重")?
            .to_string();
        if !usage.seen.insert(key) {
            continue;
        }
        usage.currency = Some(currency.to_string());
        let cost = usage.days.entry(day).or_default();
        *cost += amount;
        if !cost.is_finite() {
            return Err("账单合计超出有效范围".to_string());
        }
    }
    usage.readable = true;
    Ok(offset + rows.len() == total)
}

async fn fetch_bills(http: &Http, creds: &Creds<'_>, period: &str) -> (BillUsage, Option<String>) {
    let billing_creds = Creds {
        ak: creds.ak,
        sk: creds.sk,
        region: BILL_DETAIL_REGION.to_string(),
    };
    let mut usage = BillUsage::default();
    let mut offset = 0;
    for _ in 0..BILL_MAX_PAGES {
        if offset > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(210)).await;
        }
        let result = call(
            http,
            &billing_creds,
            BILL_DETAIL_HOST,
            "billing",
            "ListBillDetail",
            BILLING_VERSION,
            Some(&bill_body(period, offset)),
        )
        .await;
        let value = match result {
            Ok(v) => v,
            Err(e) => return (usage, Some(auth_hint(e))),
        };
        match parse_bill_page(&value, period, offset, &mut usage) {
            Ok(true) => {
                usage.complete = true;
                return (usage, None);
            }
            Ok(false) => {
                offset += BILL_PAGE_SIZE;
            }
            Err(e) => return (usage, Some(e)),
        }
    }
    (
        usage,
        Some("账单分页达到安全上限，仅保留已读取部分".to_string()),
    )
}

fn apply_payg(
    snap: &mut UsageSnapshot,
    inference: Option<InferenceUsage>,
    bills: &BillUsage,
    range: &UsageRange,
) {
    let inference_ok = inference.is_some();
    let mut usage = inference.unwrap_or_default();
    snap.supports_usage_api = inference_ok;
    let mut date = chrono::NaiveDate::parse_from_str(&range.start, "%Y-%m-%d").unwrap();
    let end = chrono::NaiveDate::parse_from_str(&range.today, "%Y-%m-%d").unwrap();
    while date <= end {
        let key = date.to_string();
        if inference_ok || (bills.complete && key >= range.month) {
            usage.days.entry(key.clone()).or_insert(DailyUsage {
                date: key,
                requests: if inference_ok { 0.0 } else { UNKNOWN },
                tokens: if inference_ok { 0.0 } else { UNKNOWN },
                cost: UNKNOWN,
            });
        }
        date += chrono::Duration::days(1);
    }
    let month_cost = if bills.complete {
        bills.days.values().sum()
    } else {
        UNKNOWN
    };
    let mut month = PeriodUsage {
        requests: if inference_ok { 0.0 } else { UNKNOWN },
        tokens: if inference_ok { 0.0 } else { UNKNOWN },
        cost: month_cost,
        from: range.month.clone(),
        to: range.today.clone(),
    };
    let mut today = PeriodUsage {
        requests: if inference_ok { 0.0 } else { UNKNOWN },
        tokens: if inference_ok { 0.0 } else { UNKNOWN },
        cost: if bills.complete {
            *bills.days.get(&range.today).unwrap_or(&0.0)
        } else {
            UNKNOWN
        },
        from: range.today.clone(),
        to: range.today.clone(),
    };
    for day in usage.days.values() {
        if inference_ok && day.date >= range.month {
            month.tokens += day.tokens;
            month.requests += day.requests;
        }
        if inference_ok && day.date == range.today {
            today.tokens = day.tokens;
            today.requests = day.requests;
        }
    }
    for (date, cost) in &bills.days {
        if date > &range.today {
            continue;
        }
        let day = usage.days.entry(date.clone()).or_insert(DailyUsage {
            date: date.clone(),
            requests: if inference_ok { 0.0 } else { UNKNOWN },
            tokens: if inference_ok { 0.0 } else { UNKNOWN },
            cost: UNKNOWN,
        });
        day.cost = if bills.complete { *cost } else { UNKNOWN };
    }
    if bills.complete {
        for day in usage.days.values_mut() {
            if day.date >= range.month {
                day.cost = *bills.days.get(&day.date).unwrap_or(&0.0);
            }
        }
    }
    if inference_ok {
        for (label, value) in [
            ("当月输入 Token", usage.input),
            ("当月输出 Token", usage.output),
        ] {
            snap.metrics.push(Metric {
                label: label.to_string(),
                value: value.to_string(),
                hint: Some("北京时间；default 项目；normal 正常后付费按量推理".to_string()),
            });
        }
    }
    if bills.complete {
        snap.metrics.push(Metric {
            label: "当月按量已出账应付金额".to_string(),
            value: month_cost.to_string(),
            hint: Some("账户级火山方舟账单；非 default 项目专属；非实时且可能延迟".to_string()),
        });
    }
    if let Some(currency) = &bills.currency {
        snap.currency = currency.clone();
    }
    if !bills.days.is_empty() && !bills.complete {
        snap.metrics.push(Metric {
            label: "已读取部分账单费用".to_string(),
            value: bills.days.values().sum::<f64>().to_string(),
            hint: Some("分页不完整，不作为当月完整消费；费用单位见定价币种".to_string()),
        });
    }
    snap.metrics.push(Metric { label: "按量数据口径".to_string(), value: "推理用量与账单双源".to_string(), hint: Some("Token/请求：北京时间 default 项目正常后付费推理；费用：账户火山方舟当月按量消费已出账应付金额（非实时，存在延迟），两者范围不同；不包含 Coding/Agent 套餐。不关联模型费用。账单按天、计费项聚合，以明细 ID 去重汇总 PayableAmount，不按结算类型筛除；官方未说明结算类型间的重复关系，金额口径仍需真实账单核对。负数为未知标记，不代表真实零。".to_string()) });
    snap.month = Some(month);
    snap.today = Some(today);
    snap.days = usage.days.into_values().collect();
    snap.models = usage.models.into_values().collect();
    snap.models.sort_by(|a, b| b.tokens.total_cmp(&a.tokens));
}

// ---------------------------------------------------------------------------
// 抓取入口
// ---------------------------------------------------------------------------

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    let ak = cfg
        .access_key_id
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    let sk = cfg
        .access_key_secret
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut notices: Vec<String> = Vec::new();
    let mut balance_ok = false;
    let mut plan_ok = false;
    let mut coding_plan_ok = false;
    let mut quota_ok = false;
    let mut model_ok = false;
    let mut payg_ok = false;
    let today = Utc::now()
        .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive();
    let range = usage_range(today);

    if ak.is_empty() || sk.is_empty() {
        notices.push("未填写 Access Key / Secret Key".to_string());
    } else {
        let creds = Creds {
            ak: &ak,
            sk: &sk,
            region: region_of(&base_or_default(cfg)),
        };

        let inference_creds = Creds {
            ak: &ak,
            sk: &sk,
            region: "cn-beijing".to_string(),
        };
        let inference = match call(
            http,
            &inference_creds,
            ARK_HOST,
            "ark",
            "GetInferenceUsage",
            ARK_VERSION,
            Some(&inference_body(&range)),
        )
        .await
        .and_then(|v| parse_inference(&v, &range))
        {
            Ok(usage) => Some(usage),
            Err(e) => {
                snap.metrics.push(Metric {
                    label: "推理用量读取提示".to_string(),
                    value: "Token / 请求数未知".to_string(),
                    hint: Some(auth_hint(e)),
                });
                None
            }
        };
        let (bills, bill_error) = fetch_bills(http, &creds, &range.month[..7]).await;
        if let Some(error) = bill_error {
            snap.metrics.push(Metric {
                label: "按量费用读取提示".to_string(),
                value: "完整账期费用未知".to_string(),
                hint: Some(error),
            });
        }
        payg_ok = inference.is_some() || bills.readable;
        apply_payg(&mut snap, inference, &bills, &range);

        // 1) 账户余额（费用中心，跨产品通用）
        match call(
            http,
            &creds,
            BILLING_HOST,
            "billing",
            "QueryBalanceAcct",
            BILLING_VERSION,
            None,
        )
        .await
        {
            Ok(v) => balance_ok = apply_balance(&mut snap, &v),
            Err(e) => notices.push(auth_hint(e)),
        }

        // 2) Coding Plan / Agent Plan 套餐；未订阅返回 404，不算失败
        for plan in ["CodingPlan", "AgentPlan"] {
            match call(
                http,
                &creds,
                ARK_HOST,
                "ark",
                "GetPersonalPlan",
                ARK_VERSION,
                Some(&json!({ "Plan": plan })),
            )
            .await
            {
                Ok(v) => {
                    let applied = apply_plan(&mut snap, plan, &v);
                    if applied && plan == "CodingPlan" {
                        coding_plan_ok = true;
                    }
                    plan_ok |= applied;
                }
                Err(e) if is_missing_plan(&e) => {}
                Err(e) => notices.push(auth_hint(e)),
            }
        }

        if plan_ok {
            snap.metrics.push(Metric {
                label: "套餐数据来源".to_string(),
                value: "方舟管控面接口".to_string(),
                hint: Some("POST /?Action=GetPersonalPlan&Version=2024-01-01".to_string()),
            });
        }

        // 3) 套餐用量限额（近 5 小时 / 近一周 / 近一月）；仅 Agent Plan 个人版可读，未订阅不算失败
        match call(
            http,
            &creds,
            ARK_HOST,
            "ark",
            "GetAFPUsage",
            ARK_VERSION,
            Some(&json!({})),
        )
        .await
        {
            Ok(v) => quota_ok = apply_afp_usage(&mut snap, &v),
            Err(e) if is_missing_plan(&e) => {}
            Err(e) => notices.push(auth_hint(e)),
        }

        if quota_ok {
            snap.metrics.push(Metric {
                label: "套餐用量数据来源".to_string(),
                value: "方舟管控面接口".to_string(),
                hint: Some("POST /?Action=GetAFPUsage&Version=2024-01-01".to_string()),
            });
        } else if coding_plan_ok {
            // Coding Plan（个人版）没有 AFP 额度，改读 GetCodingPlanUsage 的 5 小时 / 周 / 月使用率
            match call(
                http,
                &creds,
                ARK_HOST,
                "ark",
                "GetCodingPlanUsage",
                ARK_VERSION,
                Some(&json!({})),
            )
            .await
            {
                Ok(v) if apply_coding_plan_usage(&mut snap, &v) => {
                    quota_ok = true;
                    snap.metrics.push(Metric {
                        label: "套餐用量数据来源".to_string(),
                        value: "方舟 Coding Plan 接口".to_string(),
                        hint: Some(
                            "POST /?Action=GetCodingPlanUsage&Version=2024-01-01".to_string(),
                        ),
                    });
                }
                // 接口通了但字段没认出来：把原始返回带出来，便于按实际结构校正字段名
                Ok(v) => snap.metrics.push(Metric {
                    label: "套餐用量读取提示".to_string(),
                    value: "未识别到用量字段".to_string(),
                    hint: Some(format!(
                        "GetCodingPlanUsage 返回：{}",
                        truncate(&result_of(&v).to_string(), 400)
                    )),
                }),
                // 未订阅套餐时该接口无数据，按无套餐静默降级
                Err(e) if is_missing_plan(&e) => {}
                Err(e) => snap.metrics.push(Metric {
                    label: "套餐用量读取提示".to_string(),
                    value: truncate(&e, 60),
                    hint: Some(format!(
                        "POST /?Action=GetCodingPlanUsage&Version=2024-01-01 · {e}"
                    )),
                }),
            }
        }

        // 4) 已开通模型
        match call(
            http,
            &creds,
            ARK_HOST,
            "ark",
            "ListModelActivations",
            ARK_VERSION,
            Some(&json!({ "PageNumber": 1, "PageSize": 50 })),
        )
        .await
        {
            Ok(v) => model_ok = apply_activated_models(&mut snap, &v),
            Err(e) => notices.push(auth_hint(e)),
        }

        // 5) Coding Plan 支持的模型；读不到只降级
        if let Ok(v) = call(
            http,
            &creds,
            ARK_HOST,
            "ark",
            "ListArkCodingPlanModel",
            ARK_VERSION,
            Some(&json!({})),
        )
        .await
        {
            apply_coding_models(&mut snap, &v);
        }
    }

    let failure = notices.into_iter().next();
    let has_data = balance_ok || plan_ok || quota_ok || model_ok || payg_ok;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> UsageRange {
        usage_range(chrono::NaiveDate::from_ymd_opt(2026, 10, 9).unwrap())
    }

    fn inference(data: Value) -> Value {
        json!({"Result":{"Fields":[{"Name":"ModelName"},{"Name":"OutputTokens"},
            {"Name":"Day"},{"Name":"ReqCnt"},{"Name":"InputTokens"}],
            "DataCount":data.as_array().unwrap().len(),"Data":data}})
    }

    fn bill() -> Value {
        json!({"ProductZh":"火山方舟","BillingMode":"按量计费","BillCategory":"消费-使用",
            "ExpenseDate":"2026-10-09","Currency":"CNY","PayableAmount":"2.50",
            "PaidAmount":"0.10","DiscountBillAmount":"9.90","SettlementType":"非结算",
            "BillDetailId":"demo-detail","ConfigName":"按量推理"})
    }

    fn page(rows: Vec<Value>, total: usize, offset: usize) -> Value {
        json!({"Result":{"List":rows,"Total":total,"Offset":offset,"Limit":300}})
    }

    fn snapshot() -> UsageSnapshot {
        let cfg: ProviderConfig = serde_json::from_value(
            json!({"id":"demo","name":"demo","kind":"volcengine","createdAt":"","updatedAt":""}),
        )
        .unwrap();
        UsageSnapshot::empty(&cfg)
    }

    #[test]
    fn inference_dynamic_fields_and_month_boundary() {
        let v = inference(json!([
            ["demo-model", "20", "2026-09-30", 1, "100"],
            ["demo-model", 30, "2026-10-09", "2", 200],
            ["demo-model", "40", "2026-10-09", 3, "300"]
        ]));
        let usage = parse_inference(&v, &range()).unwrap();
        assert_eq!(usage.days["2026-09-30"].tokens, 120.0);
        assert_eq!(usage.days["2026-10-09"].tokens, 570.0);
        assert_eq!(usage.models["demo-model"].tokens, 570.0);
        assert_eq!(usage.input, 500.0);
        assert_eq!(usage.output, 70.0);
        let mut snap = snapshot();
        apply_payg(&mut snap, Some(usage), &BillUsage::default(), &range());
        assert_eq!(snap.month.as_ref().unwrap().tokens, 570.0);
        assert_eq!(snap.today.as_ref().unwrap().requests, 5.0);
        assert_eq!(snap.month.as_ref().unwrap().from, "2026-10-01");
        assert_eq!(snap.month.as_ref().unwrap().cost, UNKNOWN);
    }

    #[test]
    fn inference_total_column_reordered() {
        let v = json!({"Result":{"Fields":[{"Name":"TotalTokens"},{"Name":"ReqCnt"},
            {"Name":"Day"},{"Name":"ModelName"},{"Name":"OutputTokens"},{"Name":"InputTokens"}],
            "Data":[[999,"2","2026-10-09","demo",4,"5"]],"DataCount":1}});
        assert_eq!(
            parse_inference(&v, &range()).unwrap().days["2026-10-09"].tokens,
            999.0
        );
    }

    #[test]
    fn inference_rejects_mixed_bad_rows_and_nonfinite() {
        for value in [
            json!("NaN"),
            json!("inf"),
            json!(-1),
            Value::Null,
            json!(true),
            json!(""),
        ] {
            let v = inference(json!([
                ["demo", 1, "2026-10-09", 1, 10],
                ["demo", 1, "2026-10-09", 1, value]
            ]));
            assert!(parse_inference(&v, &range()).is_err());
        }
        for row in [
            json!(["demo", 1, "2026-10-09"]),
            json!(["demo", 1, "2026-09-01", 1, 10]),
            json!(["demo", 1, "2026-02-30", 1, 10]),
            json!(["", 1, "2026-10-09", 1, 10]),
        ] {
            assert!(parse_inference(&inference(json!([row])), &range()).is_err());
        }
        assert!(parse_inference(&json!({"Result":{"Data":[]}}), &range()).is_err());
        let mut v = inference(json!([]));
        v["Result"]["DataCount"] = json!(1);
        assert!(parse_inference(&v, &range()).is_err());
    }

    #[test]
    fn empty_inference_is_valid_zero_not_failure() {
        let usage = parse_inference(&inference(json!([])), &range()).unwrap();
        let mut snap = snapshot();
        apply_payg(&mut snap, Some(usage), &BillUsage::default(), &range());
        assert!(snap.supports_usage_api);
        assert!(snap.tokens_total.is_none());
        assert_eq!(snap.month.as_ref().unwrap().tokens, 0.0);
        assert_eq!(snap.days.len(), 30);
        assert!(snap
            .days
            .iter()
            .all(|day| day.tokens == 0.0 && day.cost == UNKNOWN));
        assert_eq!(snap.month.as_ref().unwrap().cost, UNKNOWN);
    }

    #[test]
    fn beijing_range_and_normal_filter_request() {
        let date = chrono::DateTime::parse_from_rfc3339("2026-09-30T16:01:00Z")
            .unwrap()
            .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
            .date_naive();
        let r = usage_range(date);
        assert_eq!(r.today, "2026-10-01");
        assert_eq!(r.start, "2026-09-02");
        let body = inference_body(&r);
        assert_eq!(body["ProjectName"], "default");
        assert_eq!(body["Filters"][0]["Values"], json!(["normal"]));
        assert_eq!(body["Filters"][1]["Values"], json!([]));
        assert_eq!(body["QueryInterval"], "Day");
        assert_eq!(
            usage_range(chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap()).start,
            "2026-10-01"
        );
        assert_eq!(bill_body("2026-10", 300)["Offset"], 300);
        assert_eq!(bill_body("2026-10", 0)["BillingMode"], json!(["2"]));
        assert_eq!(BILL_DETAIL_REGION, "cn-north-1");
    }

    #[test]
    fn bills_exclude_other_products_plans_and_deduplicate() {
        let mut other = bill();
        other["ProductZh"] = json!("云服务器");
        let mut prepaid = bill();
        prepaid["BillingMode"] = json!("1");
        let mut coding = bill();
        coding["ConfigName"] = json!("Coding Plan");
        let mut agent = bill();
        agent["Element"] = json!("Agent Plan");
        let mut agent_model = bill();
        agent_model["ConfigName"] = json!("doubao-agent-model");
        agent_model["BillDetailId"] = json!("agent-model-detail");
        let mut settlement = bill();
        settlement["SettlementType"] = json!("结算");
        settlement["BillDetailId"] = json!("settlement-detail");
        let rows = vec![other, prepaid, coding, agent, agent_model, settlement, bill(), bill()];
        let mut usage = BillUsage::default();
        assert!(parse_bill_page(&page(rows, 8, 0), "2026-10", 0, &mut usage).unwrap());
        assert_eq!(usage.days["2026-10-09"], 7.5);
        let mut settlement_only = bill();
        settlement_only["SettlementType"] = json!("settle");
        assert!(parse_bill_page(
            &page(vec![settlement_only], 1, 0),
            "2026-10",
            0,
            &mut BillUsage::default()
        )
        .unwrap());
    }

    #[test]
    fn bills_keep_settlement_variants_and_only_deduplicate_detail_ids() {
        let mut rows = Vec::new();
        for (i, settlement) in ["settle", "quota-settle", "non-settle", "结算", "Quota结算", "非结算", "unknown"].iter().enumerate() {
            let mut row = bill();
            row["BillDetailId"] = json!(format!("demo-detail-{i}"));
            row["SettlementType"] = json!(settlement);
            rows.push(row);
        }
        let mut no_settlement = bill();
        no_settlement.as_object_mut().unwrap().remove("SettlementType");
        rows.push(no_settlement.clone());
        no_settlement["SettlementType"] = json!("settle");
        rows.push(no_settlement);
        let mut usage = BillUsage::default();
        assert!(parse_bill_page(&page(rows, 9, 0), "2026-10", 0, &mut usage).unwrap());
        assert_eq!(usage.days["2026-10-09"], 20.0);
        usage.complete = true;
        let mut snap = snapshot();
        apply_payg(&mut snap, None, &usage, &range());
        assert_eq!(snap.month.as_ref().unwrap().cost, 20.0);
        assert!(snap.metrics.iter().any(|m| m.hint.as_deref().is_some_and(|hint| hint.contains("官方未说明"))));
    }

    #[test]
    fn bills_reject_missing_currency_amount_and_bad_dates() {
        for field in [
            "Currency",
            "PayableAmount",
            "ExpenseDate",
            "ProductZh",
            "BillingMode",
            "BillCategory",
            "BillDetailId",
        ] {
            let mut row = bill();
            row.as_object_mut().unwrap().remove(field);
            assert!(parse_bill_page(
                &page(vec![row], 1, 0),
                "2026-10",
                0,
                &mut BillUsage::default()
            )
            .is_err());
        }
        for (key, value) in [
            ("Currency", "USD"),
            ("PayableAmount", "NaN"),
            ("ExpenseDate", "2026-09-30"),
        ] {
            let mut row = bill();
            row[key] = json!(value);
            assert!(parse_bill_page(
                &page(vec![row], 1, 0),
                "2026-10",
                0,
                &mut BillUsage::default()
            )
            .is_err());
        }
    }

    #[test]
    fn bills_incomplete_repeated_and_empty_pages() {
        let mut usage = BillUsage::default();
        assert!(parse_bill_page(&page(vec![bill()], 2, 0), "2026-10", 0, &mut usage).is_err());
        let mut rows = vec![bill(); BILL_PAGE_SIZE];
        rows[0]["BillDetailId"] = json!("demo-first");
        assert!(!parse_bill_page(&page(rows.clone(), 600, 0), "2026-10", 0, &mut usage).unwrap());
        assert!(parse_bill_page(&page(rows, 600, 300), "2026-10", 300, &mut usage).is_err());
        assert!(parse_bill_page(&page(vec![], 600, 300), "2026-10", 300, &mut usage).is_err());
        let mut snap = snapshot();
        apply_payg(&mut snap, None, &usage, &range());
        assert_eq!(snap.month.as_ref().unwrap().cost, UNKNOWN);
        assert!(snap.used.is_none());
        assert!(snap.metrics.iter().any(|m| m.label == "已读取部分账单费用"));
        assert!(
            parse_bill_page(&page(vec![], 0, 0), "2026-10", 0, &mut BillUsage::default()).unwrap()
        );
        assert!(parse_bill_page(
            &page(vec![bill()], 1, 10),
            "2026-10",
            0,
            &mut BillUsage::default()
        )
        .is_err());
    }

    #[test]
    fn independent_source_success_failure_combinations() {
        for inference_ok in [true, false] {
            for bills_ok in [true, false] {
                let mut snap = snapshot();
                let inference = inference_ok.then(|| {
                    parse_inference(
                        &inference(json!([["demo", 20, "2026-10-09", 2, 100]])),
                        &range(),
                    )
                    .unwrap()
                });
                let bills = BillUsage {
                    days: BTreeMap::from([("2026-10-09".to_string(), 2.5)]),
                    complete: bills_ok,
                    readable: bills_ok,
                    ..Default::default()
                };
                apply_payg(&mut snap, inference, &bills, &range());
                assert_eq!(snap.supports_usage_api, inference_ok);
                assert_eq!(
                    snap.month.as_ref().unwrap().tokens,
                    if inference_ok { 120.0 } else { UNKNOWN }
                );
                assert_eq!(
                    snap.month.as_ref().unwrap().cost,
                    if bills_ok { 2.5 } else { UNKNOWN }
                );
                assert!(snap.used.is_none());
                assert!(snap.tokens_total.is_none());
                assert!(snap.requests_total.is_none());
                assert_eq!(snap.today.as_ref().unwrap().requests, if inference_ok { 2.0 } else { UNKNOWN });
                assert_eq!(snap.today.as_ref().unwrap().cost, if bills_ok { 2.5 } else { UNKNOWN });
                assert_eq!(snap.metrics.iter().any(|m| m.label == "当月按量已出账应付金额"), bills_ok);
                assert!(snap.models.iter().all(|m| m.cost == UNKNOWN));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 连通性测试
// ---------------------------------------------------------------------------

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let ak = cfg
        .access_key_id
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    let sk = cfg
        .access_key_secret
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();
    if ak.is_empty() || sk.is_empty() {
        return Err("请先填写 Access Key 与 Secret Key".to_string());
    }

    let creds = Creds {
        ak: &ak,
        sk: &sk,
        region: region_of(&base_or_default(cfg)),
    };
    let mut parts: Vec<String> = Vec::new();
    // 余额与套餐的权限范围不同：任一项读到数据即视为 AK/SK 有效
    let mut failure: Option<String> = None;
    let mut no_plan = false;

    match call(
        http,
        &creds,
        BILLING_HOST,
        "billing",
        "QueryBalanceAcct",
        BILLING_VERSION,
        None,
    )
    .await
    {
        Ok(v) => {
            if let Some(b) = pick_f64(&result_of(&v), &["AvailableBalance"]) {
                parts.push(format!("账户余额 ¥{b:.2}"));
            }
        }
        Err(e) => failure = Some(auth_hint(e)),
    }

    match call(
        http,
        &creds,
        ARK_HOST,
        "ark",
        "GetPersonalPlan",
        ARK_VERSION,
        Some(&json!({ "Plan": "CodingPlan" })),
    )
    .await
    {
        Ok(v) => {
            let r = result_of(&v);
            if let Some(tier) = pick_string(&r, &["PlanType"]) {
                let status = pick_string(&r, &["Status"]).unwrap_or_default();
                parts.push(format!("Coding Plan {tier}（{status}）"));
            }
        }
        Err(e) if is_missing_plan(&e) => no_plan = true,
        Err(e) => failure = Some(auth_hint(e)),
    }

    if !parts.is_empty() {
        return Ok(format!("连接成功，{}", parts.join("，")));
    }
    if let Some(e) = failure {
        return Err(e);
    }
    if no_plan {
        return Ok("连接成功，账号未订阅 Coding Plan（按量付费）".to_string());
    }
    Err("AK/SK 校验通过，但未识别到余额或套餐字段".to_string())
}
