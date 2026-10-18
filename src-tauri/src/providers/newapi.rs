//! New API 中转站适配器。
//!
//! 只走「API 接入方式」一条链路：手动粘贴的站点访问令牌（Bearer）→
//! `/api/user/self` 读额度与累计量；读不到时退回令牌级 `/api/usage/token`。
//! 不再有「打开登录窗口抓网页会话、再调控制台内部接口」那条链路。
//!
//! 用量序列按候选接口逐个探测，最终以「字段无关」的方式归一化成日维度 / 模型维度的统计。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::http::Http;
use crate::models::{
    DailyUsage, Metric, ModelUsage, PeriodUsage, ProviderConfig, RequestLog, UsageSnapshot,
};
use crate::providers::util::*;

pub struct Session {
    pub token: Option<String>,
    /// 部分版本的 New API 校验令牌时要求携带 `New-Api-User`（用户 ID）头
    pub uid: Option<String>,
}

impl Session {
    fn extra(&self) -> Vec<(&str, &str)> {
        self.uid
            .as_deref()
            .map(|u| vec![("New-Api-User", u)])
            .unwrap_or_default()
    }
}

/// 取本账号用于请求站点接口的凭证：站点访问令牌（Bearer）。
///
/// 站点要求 `New-Api-User` 时可在令牌后追加 `#uid=<用户 ID>`，
/// 用户 ID 见站点「个人设置」页；这里把它解析出来，取数时带上。
pub fn login(cfg: &ProviderConfig) -> Result<Session, String> {
    let raw = cfg.token.clone().unwrap_or_default().trim().to_string();
    if raw.is_empty() {
        return Err("未填写 API Token，请在站点「个人设置 - 系统访问令牌」生成后粘贴".to_string());
    }
    // 需要用户 ID 的站点会写成 `<令牌>#uid=<用户 ID>`
    let (token, uid) = match raw.rsplit_once("#uid=") {
        Some((v, u)) if !u.is_empty() && u.chars().all(|d| d.is_ascii_digit()) => {
            (v.to_string(), Some(u.to_string()))
        }
        _ => (raw, None),
    };
    Ok(Session {
        token: Some(token),
        uid,
    })
}

/// 连通性测试：只做最小代价的一次调用。
pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let mut session = login(cfg)?;
    let base = base_url(cfg)?;
    let money = money_settings(cfg, http, &base, session.token.as_deref()).await;

    // 优先账户级接口；凭证是站点令牌（API 密钥）时会因缺 `New-Api-User` 被拒，
    // 这时退回令牌级额度接口（不需要用户 ID）。
    if let Ok(v) = fetch_self(http, &base, &mut session).await {
        let data = v.get("data").cloned().unwrap_or(v);
        let name = pick_string(&data, &["username", "display_name", "email"])
            .unwrap_or_else(|| "未知用户".to_string());
        let quota = pick_f64(&data, &["quota"]).unwrap_or(0.0);
        return Ok(format!(
            "连接成功，用户 {}，剩余额度 {:.2}（积分 {}）",
            name,
            quota / money.rate,
            quota.round() as i64
        ));
    }

    let token = session.token.clone().unwrap_or_default();
    let v = fetch_token_usage(http, &base, &token).await?;
    let data = v.get("data").cloned().unwrap_or(v);
    if pick(&data, &["unlimited_quota", "unlimitedQuota"])
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
    {
        return Ok("连接成功（API 密钥），该令牌为无限额度".to_string());
    }
    let available = pick_f64(&data, &["total_available", "totalAvailable"]).unwrap_or(0.0);
    Ok(format!(
        "连接成功（API 密钥），令牌剩余额度 {:.2}（积分 {}）",
        available / money.rate,
        available.round() as i64
    ))
}

fn base_url(cfg: &ProviderConfig) -> Result<String, String> {
    let base = cfg.base_url.clone().unwrap_or_default().trim().to_string();
    if base.is_empty() {
        return Err("未填写站点地址".to_string());
    }
    Ok(base)
}

/// 从接口返回里取用户 ID：New API 校验令牌时要求 `New-Api-User` 头与凭证配套。
fn pick_uid(v: &Value) -> Option<String> {
    let data = v.get("data").unwrap_or(v);
    let id = pick_string(data, &["id", "user_id", "userId", "uid"])?;
    let id = id.trim().to_string();
    if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
        Some(id)
    } else {
        None
    }
}

/// New API 约定：`success` 为真（或缺省但有 `data`）才算请求成功。
fn ensure_ok(v: &Value) -> Result<(), String> {
    let ok = v
        .get("success")
        .and_then(|b| b.as_bool())
        .unwrap_or_else(|| v.get("data").is_some());
    if !ok {
        let msg = v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("凭证不可用");
        return Err(hint_auth_error(msg.to_string()));
    }
    Ok(())
}

/// 把站点返回的英文鉴权报错翻译成可操作的提示：
/// 新版 New API 的用户级接口要求 `New-Api-User` 头与凭证配套。
fn hint_auth_error(msg: String) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("new-api-user") || lower.contains("user id") {
        return format!(
            "{msg}（该站点要求 New-Api-User / 用户 ID 头；若自动识别失败，可在访问令牌后追加 #uid=<用户 ID>，\
用户 ID 见站点「个人设置」页，或浏览器开发者工具里任意 /api/user/self 请求的 New-Api-User 请求头）"
        );
    }
    if lower.contains("invalid access token") {
        return format!("{msg}（该凭证不是站点的「系统访问令牌」，请在站点「个人设置」页复制后重试）");
    }
    msg
}

// ---------------------------------------------------------------------------
// 用户 ID（New-Api-User）
// ---------------------------------------------------------------------------

/// 自动识别用户 ID 时的最大候选范围：接口只在 ID 与令牌归属一致时返回 200。
const UID_SCAN_MAX: u32 = 100;

/// 按「站点 + 凭证」缓存识别结果（`None` 表示已识别过但没找到），
/// 避免每次刷新都重复探测；凭证变了 key 也就变了。
fn uid_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cred_key(base: &str, s: &Session) -> String {
    let cred = s.token.clone().filter(|t| !t.trim().is_empty()).unwrap_or_default();
    format!("{}|{}", base.trim_end_matches('/'), cred)
}

fn cached_uid(base: &str, s: &Session) -> Option<Option<String>> {
    uid_cache().lock().ok()?.get(&cred_key(base, s)).cloned()
}

fn store_uid(base: &str, s: &Session, uid: Option<String>) {
    if let Ok(mut m) = uid_cache().lock() {
        m.insert(cred_key(base, s), uid);
    }
}

/// 站点是否在索要 `New-Api-User` 头
fn needs_user_id(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("new-api-user") || lower.contains("user id not provided")
}

/// 自动识别令牌归属的用户 ID：逐个候选 ID 请求 `/api/user/self`，命中即缓存。
/// 站点用户数超过 [`UID_SCAN_MAX`] 时可能识别不到，此时回退到手动 `#uid=`。
async fn resolve_uid(http: &Http, base: &str, s: &Session) -> Option<String> {
    if let Some(hit) = cached_uid(base, s) {
        return hit;
    }
    let url = crate::http::join_url(base, "/api/user/self");
    let mut found: Option<String> = None;
    for id in 1..=UID_SCAN_MAX {
        let uid = id.to_string();
        let extra = vec![("New-Api-User", uid.as_str())];
        if let Ok(v) = http.get_json_h(&url, s.token.as_deref(), None, &extra).await {
            if ensure_ok(&v).is_ok() && pick_uid(&v).as_deref() == Some(uid.as_str()) {
                found = Some(uid);
                break;
            }
        }
    }
    store_uid(base, s, found.clone());
    found
}

/// 读取 `/api/user/self`。站点要求 `New-Api-User` 而凭证里没带用户 ID 时，
/// 先自动识别用户 ID 再重试一次。
async fn fetch_self(http: &Http, base: &str, session: &mut Session) -> Result<Value, String> {
    let url = crate::http::join_url(base, "/api/user/self");
    async fn call(http: &Http, url: &str, s: &Session) -> Result<Value, String> {
        http.get_json_h(url, s.token.as_deref(), None, &s.extra()).await
    }

    match call(http, &url, session).await {
        Ok(v) => Ok(v),
        Err(e) => {
            if session.uid.is_none() && needs_user_id(&e) {
                if let Some(uid) = resolve_uid(http, base, session).await {
                    session.uid = Some(uid);
                    return call(http, &url, session).await.map_err(hint_auth_error);
                }
            }
            Err(hint_auth_error(e))
        }
    }
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http, days: usize) -> UsageSnapshot {
    fetch_by_key(cfg, http, days).await
}

/// 手动填写的站点访问令牌（Bearer），只调令牌能访问的 `/api/*`。
///
/// 新版 New API 的用户级接口还要求 `New-Api-User: <用户 ID>`，因此 API Token 字段
/// 支持写 `令牌#uid=<用户 ID>`（见 `login()`）。
async fn fetch_by_key(cfg: &ProviderConfig, http: &Http, days: usize) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();
    snap.supports_usage_api = true;

    let mut session = match login(cfg) {
        Ok(s) => s,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };
    let base = match base_url(cfg) {
        Ok(b) => b,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };

    let money = money_settings(cfg, http, &base, session.token.as_deref()).await;
    snap.currency = money.currency.clone();
    let rate = money.rate;
    // 账户级接口：站点要求 New-Api-User 时，这里会自动识别用户 ID 后重试
    match fetch_self(http, &base, &mut session).await {
        Ok(v) => apply_account(&mut snap, &v, rate, cfg),
        Err(e) => {
            // 凭证是站点令牌（API 密钥）时，账户级接口需要 `New-Api-User` 而用户 ID 未知，
            // 读不到；退回令牌级额度接口 —— 它由 `TokenAuthReadOnly` 鉴权，只认令牌本身，
            // 不需要用户 ID，因此「只用 API 密钥」也能拿到额度。
            let token = session.token.clone().unwrap_or_default();
            match fetch_token_usage(http, &base, &token).await {
                Ok(v) => apply_token_usage(&mut snap, &v, rate),
                Err(_) => {
                    snap.error = Some(format!("读取账户信息失败: {e}"));
                    return snap;
                }
            }
        }
    }
    let bearer = session.token.as_deref();
    let extra = session.extra();

    // 用量明细：只探测令牌能访问的统计接口（New API 的用户级接口同样认
    // 「系统访问令牌」，因此控制台看板数据源也一并探测，失败不影响主流程）
    let start = unixtime_days_ago(days as i64);
    let end = now_ts();
    let (rpm, tpm) = apply_dashboard(&mut snap, http, &base, bearer, &extra, rate, start, end).await;
    let candidates = vec![
        format!("/api/usage/token?type=1&start_timestamp={start}&end_timestamp={end}"),
        format!("/api/usage/token?type=2&start_timestamp={start}&end_timestamp={end}"),
        "/api/usage/token?type=1".to_string(),
        "/api/usage/token".to_string(),
        "/api/user/self/usage".to_string(),
        format!("/api/log/self/stat?start_timestamp={start}&end_timestamp={end}"),
        "/api/log/self/stat".to_string(),
    ];
    // 控制台看板已经给出日 / 模型维度时不再用别的接口覆盖，避免同一份用量
    // 因为来源不同而与站点对不上
    if snap.days.is_empty() {
        apply_records(&mut snap, http, &base, bearer, &extra, &candidates, rate).await;
    }
    apply_dashboard_metrics(&mut snap, rpm, tpm);
    snap.month = month_period(&snap.days);

    snap.ok = snap.error.is_none();
    snap
}

/// 站点控制台首页的数据源：`/api/data/self`（按模型 + 日期聚合）。
///
/// 返回数组时按通用归一化落到日 / 模型维度；返回对象（部分版本直接给今日汇总）时
/// 取今日的请求数 / 额度 / Tokens 与平均 RPM / TPM。
async fn apply_dashboard(
    snap: &mut UsageSnapshot,
    http: &Http,
    base: &str,
    bearer: Option<&str>,
    extra: &[(&str, &str)],
    rate: f64,
    start: i64,
    end: i64,
) -> (Option<f64>, Option<f64>) {
    let url = crate::http::join_url(
        base,
        &format!("/api/data/self?start_timestamp={start}&end_timestamp={end}"),
    );
    let Ok(v) = http.get_json_h(&url, bearer, None, extra).await else {
        return (None, None);
    };
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());

    if data.is_array() {
        let records = find_usage_records(&data);
        if !records.is_empty() {
            apply_record_set(snap, &records, false, rate);
        }
        // 控制台首页即使今天没有用量也会展示「今日」那几张卡片，这里同样补成 0
        snap.today.get_or_insert_with(today_zero);
        return (None, None);
    }

    let today_requests = pick_f64(
        &data,
        &[
            "today_request_count",
            "todayRequestCount",
            "today_requests",
            "request_count",
        ],
    );
    let today_tokens = pick_f64(
        &data,
        &["today_tokens", "todayTokens", "today_token_used", "token_used"],
    )
    .or_else(|| {
        let p = pick_f64(&data, &["today_prompt_tokens", "todayPromptTokens"]).unwrap_or(0.0);
        let c = pick_f64(&data, &["today_completion_tokens", "todayCompletionTokens"]).unwrap_or(0.0);
        if p + c > 0.0 {
            Some(p + c)
        } else {
            None
        }
    });
    let today_cost = pick_f64(&data, &["today_quota", "todayQuota"]).map(|q| q / rate);

    let today = today_str();
    snap.today.get_or_insert_with(|| PeriodUsage {
        requests: today_requests.unwrap_or(0.0),
        tokens: today_tokens.unwrap_or(0.0),
        cost: today_cost.unwrap_or(0.0),
        from: today.clone(),
        to: today.clone(),
    });

    (
        pick_f64(&data, &["avg_rpm", "avgRpm", "today_rpm", "todayRpm", "rpm"]),
        pick_f64(&data, &["avg_tpm", "avgTpm", "today_tpm", "todayTpm", "tpm"]),
    )
}

/// 今日汇总的 0 值（站点控制台在当天没有用量时也会展示这张卡片）。
fn today_zero() -> PeriodUsage {
    let today = today_str();
    PeriodUsage {
        requests: 0.0,
        tokens: 0.0,
        cost: 0.0,
        from: today.clone(),
        to: today,
    }
}

/// 站点控制台首页「使用统计 / 资源消耗 / 性能指标」卡片对应的补充指标。
fn apply_dashboard_metrics(snap: &mut UsageSnapshot, rpm: Option<f64>, tpm: Option<f64>) {
    if let Some(t) = snap.today.clone() {
        snap.metrics.push(Metric {
            label: "统计次数".to_string(),
            value: format!("{}", t.requests.round() as i64),
            hint: Some("今日请求次数".to_string()),
        });
        snap.metrics.push(Metric {
            label: "统计额度".to_string(),
            value: format!("{:.2}", t.cost),
            hint: Some(format!("今日消耗（{}）", snap.currency)),
        });
        snap.metrics.push(Metric {
            label: "统计Tokens".to_string(),
            value: format!("{}", t.tokens.round() as i64),
            hint: Some("今日".to_string()),
        });
    }
    if let Some(v) = rpm {
        snap.metrics.push(Metric {
            label: "平均RPM".to_string(),
            value: format!("{v:.3}"),
            hint: None,
        });
    }
    if let Some(v) = tpm {
        snap.metrics.push(Metric {
            label: "平均TPM".to_string(),
            value: format!("{v:.3}"),
            hint: None,
        });
    }
}

/// 展示换算：`货币金额 = 原始积分 / rate`，以及要展示的货币。
struct Money {
    rate: f64,
    currency: String,
}

/// 与站点控制台完全一致的「积分 → 货币」换算。
///
/// 站点前端（`or()`）的规则是：`积分 / quota_per_unit` 之后再按展示类型换算 ——
/// `CNY` 乘 `usd_exchange_rate`、`CUSTOM` 乘 `custom_currency_exchange_rate`、
/// `USD` 原样、`TOKENS` 直接展示积分。少了这步换算，站点显示 ¥88.92 而本地按
/// 500000 直接除会算成 ¥13.00（差一个汇率倍数）。
///
/// 站点设置优先于账号里的本地配置，站点改过汇率 / 展示货币后无需用户改配置。
async fn money_settings(
    cfg: &ProviderConfig,
    http: &Http,
    base: &str,
    bearer: Option<&str>,
) -> Money {
    let mut per_unit = if cfg.quota_per_unit > 0.0 {
        cfg.quota_per_unit
    } else {
        500_000.0
    };
    let mut in_currency = cfg.display_in_currency;
    let mut factor = 1.0_f64;
    let mut currency = cfg.currency.clone();

    if let Ok(v) = http
        .get_json(&crate::http::join_url(base, "/api/status"), bearer, None)
        .await
    {
        // `/api/status` 把设置放在 `data` 里（`{data:{…},success:true}`），
        // 不脱壳就会读不到 quota_per_unit / usd_exchange_rate，退回按 1:1 换算。
        let status = v.get("data").cloned().unwrap_or_else(|| v.clone());
        if let Some(v) = pick_f64(&status, &["quota_per_unit", "quotaPerUnit"]) {
            if v > 0.0 {
                per_unit = v;
            }
        }
        if let Some(b) = pick(&status, &["display_in_currency", "displayInCurrency"])
            .and_then(|v| v.as_bool())
        {
            in_currency = b;
        }
        let display = pick_string(&status, &["quota_display_type", "quotaDisplayType"])
            .unwrap_or_default()
            .to_uppercase();
        match display.as_str() {
            "CNY" => {
                factor = pick_f64(&status, &["usd_exchange_rate", "usdExchangeRate"])
                    .filter(|v| *v > 0.0)
                    .unwrap_or(1.0);
                currency = "CNY".to_string();
            }
            "CUSTOM" => {
                factor = pick_f64(
                    &status,
                    &["custom_currency_exchange_rate", "customCurrencyExchangeRate"],
                )
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
                currency = pick_string(&status, &["custom_currency_symbol", "customCurrencySymbol"])
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "¤".to_string());
            }
            "USD" => currency = "USD".to_string(),
            // 站点直接展示原始积分，不做货币换算
            "TOKENS" => {
                return Money {
                    rate: 1.0,
                    currency: "POINT".to_string(),
                }
            }
            _ => {}
        }
    }

    if in_currency {
        Money {
            rate: per_unit / factor,
            currency,
        }
    } else {
        Money {
            rate: 1.0,
            currency: "POINT".to_string(),
        }
    }
}

/// `GET /api/user/self` → 当前余额 / 累计请求数 / 累计（历史）消耗。
fn apply_account(snap: &mut UsageSnapshot, v: &Value, rate: f64, cfg: &ProviderConfig) {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let quota = pick_f64(&data, &["quota", "remain_quota", "remainQuota"]);
    let used = pick_f64(&data, &["used_quota", "usedQuota"]);
    let requests = pick_f64(&data, &["request_count", "requestCount", "requests"]);
    let name = pick_string(&data, &["display_name", "displayName", "username", "email"])
        .or_else(|| cfg.username.clone());

    snap.balance_raw = quota;
    snap.balance = quota.map(|q| q / rate);
    snap.used = used.map(|q| q / rate);
    snap.requests_total = requests;
    snap.account = name;

    if let Some(q) = snap.balance {
        snap.metrics.push(Metric {
            label: format!("当前余额（{}）", snap.currency),
            value: format!("{q:.2}"),
            hint: Some(format!("原始积分 {}", quota.unwrap_or(0.0).round())),
        });
    }
    if let Some(u) = snap.used {
        snap.metrics.push(Metric {
            label: "历史消耗".to_string(),
            value: format!("{u:.2}"),
            hint: None,
        });
    }
    if let Some(r) = requests {
        snap.metrics.push(Metric {
            label: "请求次数".to_string(),
            value: format!("{}", r.round() as i64),
            hint: None,
        });
    }
    if let Some(g) = pick_string(&data, &["group"]) {
        snap.metrics.push(Metric {
            label: "分组".to_string(),
            value: g,
            hint: None,
        });
    }
}

/// `GET /api/usage/token`：站点令牌（API 密钥）自身的额度。
///
/// 该接口由 `TokenAuthReadOnly` 保护，只校验令牌本身，**不要求 `New-Api-User`**；
/// 因此凭证是 API 密钥时，靠它就能取到额度，不必知道用户 ID。
async fn fetch_token_usage(http: &Http, base: &str, token: &str) -> Result<Value, String> {
    if token.trim().is_empty() {
        return Err("未填写 API 密钥".to_string());
    }
    let url = crate::http::join_url(base, "/api/usage/token");
    let v = http.get_json_h(&url, Some(token), None, &[]).await?;
    ensure_ok(&v)?;
    Ok(v)
}

/// 把 `/api/usage/token` 的结果落到「令牌额度」指标。
///
/// 口径与账户级 `/api/user/self` 不同（这里是单个令牌的额度），因此文案统一标注「令牌」，
/// 避免和账户余额混淆。
fn apply_token_usage(snap: &mut UsageSnapshot, v: &Value, rate: f64) {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let available = pick_f64(&data, &["total_available", "totalAvailable"]);
    let used = pick_f64(&data, &["total_used", "totalUsed"]);
    let granted = pick_f64(&data, &["total_granted", "totalGranted"]);
    let unlimited = pick(&data, &["unlimited_quota", "unlimitedQuota"])
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    snap.balance_raw = available;
    snap.balance = available.map(|q| q / rate);
    snap.used = used.map(|q| q / rate);
    snap.granted = granted.map(|q| q / rate);

    if let Some(n) = pick_string(&data, &["name"]).filter(|s| !s.trim().is_empty()) {
        snap.metrics.push(Metric {
            label: "令牌".to_string(),
            value: n,
            hint: None,
        });
    }
    if unlimited {
        snap.metrics.push(Metric {
            label: "配额".to_string(),
            value: "无限".to_string(),
            hint: None,
        });
    } else if let Some(a) = snap.balance {
        snap.metrics.push(Metric {
            label: format!("令牌剩余额度（{}）", snap.currency),
            value: format!("{a:.2}"),
            hint: Some(format!("原始积分 {}", available.unwrap_or(0.0).round())),
        });
    }
    if let Some(u) = snap.used {
        snap.metrics.push(Metric {
            label: "令牌已用".to_string(),
            value: format!("{u:.2}"),
            hint: None,
        });
    }
    if let Some(g) = snap.granted {
        snap.metrics.push(Metric {
            label: "令牌总量".to_string(),
            value: format!("{g:.2}"),
            hint: None,
        });
    }
}

/// 按候选接口逐个探测，取第一个返回可用记录的接口并归一化成日 / 模型维度。
///
/// 探测失败只意味着这一路补充数据读不到，不设置 `snap.error`。
async fn apply_records(
    snap: &mut UsageSnapshot,
    http: &Http,
    base: &str,
    bearer: Option<&str>,
    extra: &[(&str, &str)],
    candidates: &[String],
    rate: f64,
) {
    for c in candidates {
        let url = crate::http::join_url(base, c);
        let Ok(v) = http.get_json_h(&url, bearer, None, extra).await else {
            continue;
        };
        let records = find_usage_records(&v);
        if records.is_empty() {
            continue;
        }
        let per_record_is_request = c.contains("/api/log/self?") || c.ends_with("/api/log/self/stat");
        apply_record_set(snap, &records, per_record_is_request, rate);
        break;
    }
}

/// 把一组用量记录归一化成日 / 模型维度并落进快照。
fn apply_record_set(snap: &mut UsageSnapshot, records: &[Value], per_record_is_request: bool, rate: f64) {
    let (day_map, model_map, logs, tokens, reqs, cost) =
        normalize(records, rate, per_record_is_request);
    snap.days = day_map.into_values().collect();
    snap.models = model_map.into_values().collect();
    snap.models.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    snap.logs = logs;
    snap.tokens_total = Some(tokens);
    if snap.requests_total.is_none() {
        snap.requests_total = Some(reqs);
    }
    // 服务端只给区间用量时，累计消耗以接口返回为准，这里不覆盖
    if snap.used.is_none() {
        snap.used = Some(cost);
    }
    // 今日汇总以数据看板为准：日志类兜底拿不到「今天」时会把已有的汇总清空，
    // 详情页「统计次数 / 统计额度 / 统计Tokens」这几条也就跟着消失了
    if snap.today.is_none() {
        snap.today = today_period(&snap.days);
    }
}

fn now_ts() -> i64 {
    chrono::Local::now().timestamp()
}

type Normalized = (
    BTreeMap<String, DailyUsage>,
    BTreeMap<String, ModelUsage>,
    Vec<RequestLog>,
    f64,
    f64,
    f64,
);

fn normalize(records: &[Value], rate: f64, per_record_is_request: bool) -> Normalized {
    let mut day_map: BTreeMap<String, DailyUsage> = BTreeMap::new();
    let mut model_map: BTreeMap<String, ModelUsage> = BTreeMap::new();
    let mut logs: Vec<RequestLog> = Vec::new();
    let mut tokens_total = 0.0;
    let mut requests_total = 0.0;
    let mut cost_total = 0.0;

    for (idx, r) in records.iter().enumerate() {
        let date = record_date(r).unwrap_or_else(|| "未知".to_string());
        let tokens = pick_f64(
            r,
            &[
                "total_tokens",
                "totalTokens",
                "tokens",
                "token_count",
                // /api/data/self 的字段名
                "token_used",
                "tokens_used",
            ],
        )
        .or_else(|| {
            let p = pick_f64(r, &["prompt_tokens", "promptTokens"]).unwrap_or(0.0);
            let c = pick_f64(r, &["completion_tokens", "completionTokens"]).unwrap_or(0.0);
            if p + c > 0.0 {
                Some(p + c)
            } else {
                None
            }
        })
        .unwrap_or(0.0);

        let requests = pick_f64(r, &["request_count", "requestCount", "requests", "count"])
            .unwrap_or(if per_record_is_request { 1.0 } else { 0.0 });

        let cost_raw = pick_f64(
            r,
            &[
                "quota",
                "used_quota",
                "usedQuota",
                "cost",
                "total_cost",
                "amount",
                "money",
            ],
        )
        .unwrap_or(0.0);
        let cost = cost_raw / rate;

        let model = pick_string(r, &["model_name", "modelName", "model", "name"])
            .unwrap_or_else(|| "未标注".to_string());

        let d = day_map.entry(date.clone()).or_insert(DailyUsage {
            date: date.clone(),
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        d.requests += requests;
        d.tokens += tokens;
        d.cost += cost;

        let m = model_map.entry(model.clone()).or_insert(ModelUsage {
            model: model.clone(),
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        m.requests += requests;
        m.tokens += tokens;
        m.cost += cost;

        tokens_total += tokens;
        requests_total += requests;
        cost_total += cost;

        if per_record_is_request && logs.len() < 30 {
            logs.push(RequestLog {
                id: pick_string(r, &["id", "log_id"]).unwrap_or_else(|| idx.to_string()),
                time: pick_string(r, &["created_at", "createdAt", "time", "timestamp", "date"])
                    .unwrap_or_default(),
                model,
                tokens,
                cost,
                duration_ms: pick_f64(r, &["use_time", "useTime", "duration", "total_time"])
                    .unwrap_or(0.0),
                status: pick_string(r, &["status", "type", "result"])
                    .unwrap_or_else(|| "成功".to_string()),
            });
        }
    }

    (day_map, model_map, logs, tokens_total, requests_total, cost_total)
}

/// 补齐最近 N 天（缺失填 0），让趋势图连续。
pub fn fill_days(days: Vec<DailyUsage>, n: usize) -> Vec<DailyUsage> {
    let mut map: BTreeMap<String, DailyUsage> = days.into_iter().map(|d| (d.date.clone(), d)).collect();
    for date in recent_dates(n) {
        map.entry(date.clone()).or_insert(DailyUsage {
            date,
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
    }
    map.into_values().collect()
}

pub fn today_period(days: &[DailyUsage]) -> Option<PeriodUsage> {
    let today = today_str();
    days.iter()
        .find(|d| d.date == today)
        .map(|d| PeriodUsage {
            requests: d.requests,
            tokens: d.tokens,
            cost: d.cost,
            from: today.clone(),
            to: today,
        })
}

/// 当前自然月的汇总（与站点控制台按自然月统计的口径一致），供详情页「本月」卡片使用。
fn month_period(days: &[DailyUsage]) -> Option<PeriodUsage> {
    let today = today_str();
    let ym = today.get(..7)?.to_string();
    let mut period = PeriodUsage {
        requests: 0.0,
        tokens: 0.0,
        cost: 0.0,
        from: format!("{ym}-01"),
        to: today,
    };
    let mut hit = false;
    for d in days.iter().filter(|d| d.date.starts_with(&ym)) {
        period.requests += d.requests;
        period.tokens += d.tokens;
        period.cost += d.cost;
        hit = true;
    }
    if hit {
        Some(period)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 调用模型（价格智能获取等场景）
// ---------------------------------------------------------------------------

/// 调一次中转站（OpenAI 兼容）对话接口，使用站点访问令牌。
pub async fn chat(
    cfg: &ProviderConfig,
    http: &Http,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let key = api_token(cfg)?;
    let url = crate::http::join_url(&api_base(cfg), "/v1/chat/completions");
    openai_chat(http, &url, &key, model, system, user).await
}

/// 读取中转站 `/v1/models` 的可用模型列表。
pub async fn list_models(cfg: &ProviderConfig, http: &Http) -> Vec<String> {
    match api_token(cfg) {
        Ok(key) => fetch_model_ids(http, &api_base(cfg), &key, "/v1/models").await,
        Err(_) => Vec::new(),
    }
}
