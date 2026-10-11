//! DeepSeek 官方平台适配器。
//!
//! 两种接入方式**互斥**，只走所选的那一条链路，结果不做合并：
//!
//! | 接入方式 | 凭证 | 域名 | 能读到什么 |
//! |---|---|---|---|
//! | `api` | API Key（`sk-…`） | `api.deepseek.com` | 官方公开接口：余额、可用模型 |
//! | `login` | 平台会话 Token | `platform.deepseek.com` | 内部接口：tokens 用量、请求数、消费金额、API Keys 列表、平台余额 |
//!
//! `login` 的会话凭证由外置登录窗口抓取。
//! 另外保留「账号 + 密码」直连换取会话的能力（`/api/v0/users/login`，UI 未暴露）作为兜底，
//! 也支持在浏览器中登录 platform.deepseek.com 后手动粘贴
//! （控制台执行 `JSON.parse(localStorage.userToken).value`）。
//!
//! ⚠️ `/api/v0/usage/*` 是平台前端自用接口，无公开文档、无 SLA，
//! 因此这里的所有解析都做容错处理；接口不可用或登录失效时不提供用量数据。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{Datelike, TimeZone};
use serde_json::{json, Value};

use crate::http::Http;
use crate::models::{
    AccessMode, ApiKeyInfo, AuthType, DailyUsage, Metric, ModelUsage, PeriodUsage, ProviderConfig,
    UsageSnapshot,
};
use crate::providers::util::*;

const API_BASE: &str = "https://api.deepseek.com";
const PLATFORM_BASE: &str = "https://platform.deepseek.com";
const CHAT_BASE: &str = "https://chat.deepseek.com";

const DEVICE_ID: &str = "tokensonar";
const APP_VERSION: &str = "1.0.0";
const UA_PLATFORM: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36";

/// 网页端 Token 短期有效，缓存一段时间避免每次刷新都重新登录。
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

fn platform_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("x-app-version", APP_VERSION),
        ("accept", "*/*"),
        ("user-agent", UA_PLATFORM),
    ]
}

fn login_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("x-app-version", APP_VERSION),
        ("accept", "*/*"),
        ("user-agent", UA_PLATFORM),
        ("x-client-platform", "web"),
        ("x-client-version", APP_VERSION),
        ("x-client-locale", "zh_CN"),
    ]
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Session {
    pub token: Option<String>,
    pub cookie: Option<String>,
    /// 登录账号（邮箱 / 手机号 / 昵称）
    pub account: Option<String>,
}

impl Session {
    fn is_empty(&self) -> bool {
        self.token.is_none() && self.cookie.is_none()
    }
}

#[derive(Clone)]
struct Cached {
    session: Session,
    at: Instant,
}

fn sessions() -> &'static Mutex<HashMap<String, Cached>> {
    static S: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_get(id: &str) -> Option<Session> {
    let map = sessions().lock().ok()?;
    let c = map.get(id)?;
    if c.at.elapsed() > SESSION_TTL {
        return None;
    }
    Some(c.session.clone())
}

fn session_put(id: &str, s: &Session) {
    if let Ok(mut map) = sessions().lock() {
        map.insert(
            id.to_string(),
            Cached {
                session: s.clone(),
                at: Instant::now(),
            },
        );
    }
}

fn session_drop(id: &str) {
    if let Ok(mut map) = sessions().lock() {
        map.remove(id);
    }
}

/// `login` 接入方式下的会话来源：账号密码直连（能力保留，UI 未暴露）优先，其次登录窗口抓回的凭证。
async fn resolve_session(
    cfg: &ProviderConfig,
    http: &Http,
    force: bool,
) -> Result<Option<Session>, String> {
    if !force {
        if let Some(s) = session_get(&cfg.id) {
            if !s.is_empty() {
                return Ok(Some(s));
            }
        }
    }

    let user = cfg.username.clone().unwrap_or_default().trim().to_string();
    let pwd = cfg.password.clone().unwrap_or_default();
    if matches!(cfg.auth_type, Some(AuthType::Password)) && !user.is_empty() && !pwd.is_empty() {
        let s = login(&user, &pwd, http).await?;
        session_put(&cfg.id, &s);
        return Ok(Some(s));
    }

    if let Some(t) = cfg.session() {
        let s = Session {
            token: Some(t),
            cookie: None,
            account: None,
        };
        session_put(&cfg.id, &s);
        return Ok(Some(s));
    }

    Ok(None)
}

/// 账号 + 密码换取平台用量 Token。平台域失败时回退到 chat 域（两者账号体系相同）。
async fn login(user: &str, pwd: &str, http: &Http) -> Result<Session, String> {
    let body = if user.contains('@') {
        json!({ "email": user, "password": pwd, "device_id": DEVICE_ID, "os": "web" })
    } else {
        json!({ "mobile": user, "area_code": null, "password": pwd, "device_id": DEVICE_ID, "os": "web" })
    };

    let mut last = String::new();
    for host in [PLATFORM_BASE, CHAT_BASE] {
        let url = format!("{host}/api/v0/users/login");
        match http
            .post_json_h(&url, &body, None, None, &login_headers())
            .await
        {
            Ok((res, cookie)) => {
                if let Some(token) = extract_token(&res) {
                    let account = extract_account(&res).or_else(|| Some(user.to_string()));
                    return Ok(Session {
                        token: Some(token),
                        cookie,
                        account,
                    });
                }
                let msg = pick_string(&res, &["msg", "message", "error"])
                    .or_else(|| {
                        res.get("data")
                            .and_then(|d| pick_string(d, &["msg", "message"]))
                    })
                    .unwrap_or_default();
                last = if msg.is_empty() {
                    "登录成功但未返回 Token".to_string()
                } else {
                    format!("登录失败: {msg}")
                };
            }
            Err(e) => last = e,
        }
    }

    Err(if last.is_empty() {
        "登录失败".to_string()
    } else {
        last
    })
}

fn extract_token(v: &Value) -> Option<String> {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let biz = data.get("biz_data").cloned().unwrap_or(Value::Null);
    let user = biz
        .get("user")
        .cloned()
        .or_else(|| data.get("user").cloned())
        .unwrap_or(Value::Null);

    pick_string(&user, &["token", "access_token", "accessToken"])
        .or_else(|| pick_string(&biz, &["token", "user_token", "userToken"]))
        .or_else(|| pick_string(&data, &["token", "access_token", "accessToken"]))
        .or_else(|| pick_string(v, &["token"]))
}

fn extract_account(v: &Value) -> Option<String> {
    let data = v.get("data").cloned().unwrap_or_else(|| v.clone());
    let biz = data.get("biz_data").cloned().unwrap_or(Value::Null);
    let user = biz
        .get("user")
        .cloned()
        .or_else(|| data.get("user").cloned())
        .unwrap_or(Value::Null);
    pick_string(&user, &["email", "mobile", "phone", "nickname", "name"])
}

// ---------------------------------------------------------------------------
// 抓取入口
// ---------------------------------------------------------------------------

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    let api_base = cfg
        .base_url
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url().to_string());
    let api_key = cfg.token.clone().unwrap_or_default();
    let api_key = api_key.trim().to_string();

    // 次要错误：不阻断其它维度的数据读取
    let mut notices: Vec<String> = Vec::new();

    // 两种接入方式互斥：只走所选的那一条链路，两边读到的数据不做合并
    let mut session: Option<Session> = None;
    let mut usage: Option<UsageBundle> = None;

    if matches!(cfg.mode(), AccessMode::Login) {
        // ---- 登录授权：平台会话能读到页面上的全部信息 ----

        // 1) 会话（登录窗口抓回的凭证 / 账号密码直连）
        match resolve_session(cfg, http, false).await {
            Ok(s) => session = s,
            Err(e) => notices.push(e),
        }
        if session.is_none() {
            notices.push("未获取到平台登录凭证，请在编辑账号里点击「登录获取」重新授权".to_string());
        } else if let Some(s) = &session {
            snap.account = s.account.clone();
        }

        // 2) 用量：Token 可能过期，失败且看起来像鉴权问题时强制重登一次
        if let Some(s) = &session {
            match fetch_usage(http, s).await {
                Ok(u) => usage = Some(u),
                Err(e) => {
                    let expired = looks_expired(&e);
                    notices.push(e);
                    if expired {
                        session_drop(&cfg.id);
                        if let Ok(Some(s2)) = resolve_session(cfg, http, true).await {
                            match fetch_usage(http, &s2).await {
                                Ok(u) => {
                                    notices.clear();
                                    usage = Some(u);
                                    session = Some(s2);
                                }
                                Err(e2) => notices.push(e2),
                            }
                        }
                    }
                }
            }
        }

        // 3) 平台余额与 API Keys（这两个接口只有会话能读）
        if let Some(s) = &session {
            // 平台改版后余额在「账户汇总」接口里，旧接口列表作为兜底
            if let Some(biz) = try_platform_summary(http, s).await {
                apply_summary_balance(&mut snap, &biz, &cfg.currency);
            } else if let Some(v) = try_platform_balance(http, s).await {
                apply_balance(&mut snap, &v, &cfg.currency);
            }
            snap.api_keys = merge_keys(fetch_api_keys(http, s).await, cfg);
        } else {
            snap.api_keys = merge_keys(Vec::new(), cfg);
        }
    } else {
        // ---- API Key：官方公开接口只有余额与模型列表 ----
        if api_key.is_empty() {
            notices.push("未填写 API Key".to_string());
        } else {
            let url = crate::http::join_url(&api_base, "/user/balance");
            match http.get_json(&url, Some(&api_key), None).await {
                Ok(v) => apply_balance(&mut snap, &v, &cfg.currency),
                Err(e) => notices.push(e),
            }
            if snap.error.is_none() {
                let models_url = crate::http::join_url(&api_base, "/models");
                if let Ok(v) = http.get_json(&models_url, Some(&api_key), None).await {
                    if let Some(list) = v.get("data").and_then(|d| d.as_array()) {
                        snap.metrics.push(Metric {
                            label: "可用模型".to_string(),
                            value: format!("{} 个", list.len()),
                            hint: None,
                        });
                    }
                }
            }
        }
        snap.api_keys = merge_keys(Vec::new(), cfg);
    }

    // 4) 仅展示平台返回的真实用量；API Key 方式及平台接口不可用时没有用量数据
    if let Some(u) = usage {
        apply_usage(&mut snap, u, &cfg.currency);
        snap.supports_usage_api = true;
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
    } else if matches!(cfg.mode(), AccessMode::Login) {
        snap.metrics.push(Metric {
            label: "API Keys".to_string(),
            value: "未读取到".to_string(),
            hint: Some("平台接口未返回，可在下方「手动录入」补登".to_string()),
        });
    }

    // 只要有一个维度读到了数据就算成功，失败原因降级成「读取提示」而不是错误横幅
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

fn looks_expired(e: &str) -> bool {
    e.contains("401")
        || e.contains("40003")
        || e.contains("无效")
        || e.contains("过期")
        || e.contains("失效")
        || e.contains("未登录")
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---------------------------------------------------------------------------
// 余额
// ---------------------------------------------------------------------------

fn apply_balance(snap: &mut UsageSnapshot, v: &Value, preferred_currency: &str) {
    let infos = v
        .get("balance_infos")
        .or_else(|| v.get("balanceInfos"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let chosen = infos
        .iter()
        .find(|i| {
            pick_string(i, &["currency"])
                .map(|c| c.eq_ignore_ascii_case(preferred_currency))
                .unwrap_or(false)
        })
        .or_else(|| infos.first())
        .cloned();

    let record = match chosen {
        Some(r) => r,
        None => {
            if pick_f64(
                v,
                &[
                    "total_balance",
                    "balance",
                    "totalBalance",
                    "remain_balance",
                    "remainBalance",
                ],
            )
            .is_some()
            {
                v.clone()
            } else {
                snap.error = Some("余额接口未返回 balance_infos".to_string());
                return;
            }
        }
    };

    let currency =
        pick_string(&record, &["currency"]).unwrap_or_else(|| preferred_currency.to_string());
    snap.currency = currency.clone();

    let total = pick_f64(&record, &[
        "total_balance",
        "totalBalance",
        "balance",
        "remain_balance",
        "remainBalance",
    ]);
    let granted = pick_f64(&record, &["granted_balance", "grantedBalance"]);
    let topped_up = pick_f64(&record, &["topped_up_balance", "toppedUpBalance"]);

    snap.balance = total;
    snap.granted = granted;

    let source = if snap.supports_usage_api {
        "DeepSeek 平台接口"
    } else {
        "DeepSeek 官方余额接口"
    };

    if let Some(t) = total {
        snap.metrics.insert(
            0,
            Metric {
                label: format!("余额（{currency}）"),
                value: format!("{t:.2}"),
                hint: Some(source.to_string()),
            },
        );
    }
    if let Some(g) = granted {
        snap.metrics.push(Metric {
            label: "赠送余额".to_string(),
            value: format!("{g:.2}"),
            hint: None,
        });
    }
    if let Some(t) = topped_up {
        snap.metrics.push(Metric {
            label: "充值余额".to_string(),
            value: format!("{t:.2}"),
            hint: None,
        });
    }
    if let Some(available) = v.get("is_available").and_then(|x| x.as_bool()) {
        snap.metrics.push(Metric {
            label: "账户可用状态".to_string(),
            value: if available { "正常" } else { "已停用" }.to_string(),
            hint: None,
        });
    }
}

/// 只有平台会话、没有 API Key 时，尝试平台内部的余额接口（该接口未公开，失败静默）。
async fn try_platform_balance(http: &Http, s: &Session) -> Option<Value> {
    let token = s.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return None;
    }
    let paths = [
        "/api/v0/user/balance",
        "/api/v0/users/balance",
        "/api/v0/balance",
        "/api/v0/account/balance",
        "/api/v0/users/self",
    ];
    for p in paths {
        let url = format!("{PLATFORM_BASE}{p}");
        if let Ok(v) = http
            .get_json_h(&url, Some(token), s.cookie.as_deref(), &platform_headers())
            .await
        {
            if let Some(biz) = v.get("data").and_then(|d| d.get("biz_data")).cloned() {
                if looks_like_balance(&biz) {
                    return Some(biz);
                }
            }
            if looks_like_balance(&v) {
                return Some(v);
            }
        }
    }
    None
}

/// 平台改版后的账户汇总：余额在 `normal_wallets`（充值）/ `bonus_wallets`（赠送）里，
/// 金额是字符串（如 `"88.92"`）。这是目前唯一能读到余额的会话接口。
async fn try_platform_summary(http: &Http, s: &Session) -> Option<Value> {
    let token = s.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return None;
    }
    let url = format!("{PLATFORM_BASE}/api/v0/users/get_user_summary");
    let v = http
        .get_json_h(&url, Some(token), s.cookie.as_deref(), &platform_headers())
        .await
        .ok()?;
    let biz = extract_biz(&v);
    if wallet_sum(biz.get("normal_wallets")).is_some() || wallet_sum(biz.get("bonus_wallets")).is_some()
    {
        Some(biz)
    } else {
        None
    }
}

/// 钱包数组求和；空数组视为「接口没给这个字段」
fn wallet_sum(wallets: Option<&Value>) -> Option<f64> {
    let list = wallets?.as_array()?;
    if list.is_empty() {
        return None;
    }
    Some(
        list.iter()
            .filter_map(|w| pick_f64(w, &["balance", "amount"]))
            .sum(),
    )
}

fn wallet_currency(wallets: Option<&Value>) -> Option<String> {
    arr(wallets).iter().find_map(|w| pick_string(w, &["currency"]))
}

/// 账户汇总 → 余额（充值 + 赠送）/ 赠送余额。
fn apply_summary_balance(snap: &mut UsageSnapshot, biz: &Value, preferred_currency: &str) {
    let normal = wallet_sum(biz.get("normal_wallets"));
    let bonus = wallet_sum(biz.get("bonus_wallets"));
    let currency = wallet_currency(biz.get("normal_wallets"))
        .or_else(|| wallet_currency(biz.get("bonus_wallets")))
        .unwrap_or_else(|| preferred_currency.to_string());

    let total = match (normal, bonus) {
        (Some(n), Some(b)) => Some(n + b),
        (Some(n), None) => Some(n),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    snap.currency = currency.clone();
    snap.balance = total;
    snap.granted = bonus;

    if let Some(t) = total {
        let hint = match (normal, bonus) {
            (Some(n), Some(b)) => format!("充值 {n:.2} + 赠送 {b:.2}"),
            _ => "DeepSeek 平台账户汇总".to_string(),
        };
        snap.metrics.insert(
            0,
            Metric {
                label: format!("余额（{currency}）"),
                value: format!("{t:.2}"),
                hint: Some(hint),
            },
        );
    }
    if let Some(b) = bonus {
        snap.metrics.push(Metric {
            label: "赠送余额".to_string(),
            value: format!("{b:.2}"),
            hint: None,
        });
    }
    if let Some(n) = normal {
        snap.metrics.push(Metric {
            label: "充值余额".to_string(),
            value: format!("{n:.2}"),
            hint: None,
        });
    }
}

fn looks_like_balance(v: &Value) -> bool {
    v.get("balance_infos").is_some()
        || v.get("balanceInfos").is_some()
        || pick_f64(
            v,
            &[
                "total_balance",
                "totalBalance",
                "balance",
                "remain_balance",
                "remainBalance",
            ],
        )
        .is_some()
}

// ---------------------------------------------------------------------------
// 用量（tokens / 请求数 / 消费金额）
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MonthData {
    days: BTreeMap<String, DailyUsage>,
    models: BTreeMap<String, ModelUsage>,
    requests: f64,
    tokens: f64,
    cost: f64,
    cache_hit: f64,
    cache_miss: f64,
    response_tokens: f64,
}

struct UsageBundle {
    current: MonthData,
    previous: MonthData,
    current_label: String,
    previous_label: String,
}

async fn fetch_usage(http: &Http, s: &Session) -> Result<UsageBundle, String> {
    let token = s.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return Err("平台用量 Token 为空".to_string());
    }

    let now = chrono::Local::now();
    let (y1, m1) = (now.year(), now.month());
    let (y0, m0) = prev_month(y1, m1);

    // 当月必须成功；上月失败只影响趋势图的长度，不阻断
    let current = fetch_month(http, s, y1, m1).await?;
    let previous = fetch_month(http, s, y0, m0).await.unwrap_or_default();

    Ok(UsageBundle {
        current,
        previous,
        current_label: format!("{y1}-{m1:02}"),
        previous_label: format!("{y0}-{m0:02}"),
    })
}

/// 验证一个候选「平台用量 Token」是否可用：能取回当月用量即视为有效。
pub async fn verify_platform_token(http: &Http, token: &str) -> Result<(), String> {
    let t = token.trim();
    if t.is_empty() {
        return Err("Token 为空".to_string());
    }
    let session = Session {
        token: Some(t.to_string()),
        cookie: None,
        account: None,
    };
    let now = chrono::Local::now();
    fetch_month(http, &session, now.year(), now.month()).await?;
    Ok(())
}

fn prev_month(y: i32, m: u32) -> (i32, u32) {
    if m <= 1 {
        (y - 1, 12)
    } else {
        (y, m - 1)
    }
}

/// 某个月的用量。2026-08 平台改版后改为「区间查询」（unix 秒 + 本地时区），
/// 旧的 `?month=&year=` 接口已废弃；这里先走新接口，失败再退回旧接口。
async fn fetch_month(http: &Http, s: &Session, year: i32, month: u32) -> Result<MonthData, String> {
    let start = local_midnight(year, month, 1)
        .ok_or_else(|| format!("月份非法: {year}-{month:02}"))?;
    let now = chrono::Local::now();
    let end = if (year, month) == (now.year(), now.month()) {
        // 当月查到「明天 00:00」，与平台前端一致
        let t = now.date_naive() + chrono::Duration::days(1);
        local_midnight(t.year(), t.month(), t.day()).unwrap_or(start + 86400)
    } else {
        let (ny, nm) = next_month(year, month);
        local_midnight(ny, nm, 1).unwrap_or(start + 31 * 86400)
    };

    match fetch_range(http, s, start, end).await {
        Ok(m) => Ok(m),
        Err(e) => match fetch_legacy_month(http, s, year, month).await {
            Ok(m) => Ok(m),
            Err(_) => Err(e),
        },
    }
}

fn next_month(y: i32, m: u32) -> (i32, u32) {
    if m >= 12 {
        (y + 1, 1)
    } else {
        (y, m + 1)
    }
}

/// 本地时间某天 00:00 的 unix 秒
fn local_midnight(y: i32, m: u32, d: u32) -> Option<i64> {
    chrono::Local
        .with_ymd_and_hms(y, m, d, 0, 0, 0)
        .single()
        .map(|dt| dt.timestamp())
}

/// 时区偏移（秒）：接口按它把时间桶切成本地自然日
fn tz_offset() -> i32 {
    chrono::Local::now().offset().local_minus_utc()
}

fn ts_to_date(ts: i64) -> String {
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn in_range(ts: i64, start: i64, end: i64) -> bool {
    ts >= start && ts < end
}

fn bucket_time(b: &Value) -> Option<i64> {
    pick_f64(b, &["time", "timestamp", "start"]).map(|t| t as i64)
}

/// `biz_data`：新版直接在顶层，旧版在 `data.biz_data`
fn extract_biz(v: &Value) -> Value {
    let nested = biz_data(v);
    if !nested.is_null() {
        return nested;
    }
    v.get("biz_data").cloned().unwrap_or(Value::Null)
}

/// 401 统一翻译成「重新授权」提示
fn auth_error(e: String) -> String {
    if e.contains("HTTP 401")
        || e.contains("40003")
        || e.contains("Authorization Failed")
        || e.contains("invalid token")
    {
        "平台用量 Token 已失效，请在编辑账号里点击「登录获取」重新授权".to_string()
    } else {
        e
    }
}

/// 平台把业务失败也包在 HTTP 200 里（如
/// `{"code":40003,"msg":"Authorization Failed (invalid token)","data":null}`），
/// 必须按 `code` 判定失败；否则「Token 无效」会被当成「成功但没数据」，
/// 快照会显示成一切正常、登录校验也会被无效凭证骗过。
fn ensure_ok(v: &Value) -> Result<(), String> {
    match v.get("code").and_then(|c| c.as_i64()) {
        None | Some(0) => Ok(()),
        Some(code) => {
            let msg = pick_string(v, &["msg", "message"]).unwrap_or_default();
            Err(if msg.is_empty() {
                format!("用量接口返回 code {code}")
            } else {
                format!("用量接口 code {code}: {msg}")
            })
        }
    }
}

/// 响应里连 `data` 都没有（缺失或为 `null`），说明没有可解析的载荷
fn has_payload(v: &Value) -> bool {
    !extract_biz(v).is_null() || v.get("data").map_or(false, |d| !d.is_null())
}

/// 改版后的用量接口：按 API Key 维度返回 `series[].buckets[]`（时间桶为本地日 00:00 的 unix 秒）
async fn fetch_range(http: &Http, s: &Session, start: i64, end: i64) -> Result<MonthData, String> {
    let token = s.token.as_deref().unwrap_or("");
    if token.is_empty() {
        return Err("平台用量 Token 为空".to_string());
    }
    let headers = platform_headers();
    let query = format!("start={start}&end={end}&tz={}", tz_offset());
    let amount_url = format!("{PLATFORM_BASE}/api/v0/usage/by_api_key/amount?{query}");
    let cost_url = format!("{PLATFORM_BASE}/api/v0/usage/by_api_key/cost?{query}");

    let amount = http
        .get_json_h(&amount_url, Some(token), s.cookie.as_deref(), &headers)
        .await
        .map_err(auth_error)?;
    ensure_ok(&amount).map_err(auth_error)?;
    if !has_payload(&amount) {
        return Err("用量接口未返回数据（接口可能已再次改版）".to_string());
    }
    let cost = http
        .get_json_h(&cost_url, Some(token), s.cookie.as_deref(), &headers)
        .await
        .unwrap_or(Value::Null);

    Ok(parse_range(&amount, &cost, start, end))
}

/// 改版前（`?month=&year=`）的用量接口，仅作兜底
async fn fetch_legacy_month(
    http: &Http,
    s: &Session,
    year: i32,
    month: u32,
) -> Result<MonthData, String> {
    let token = s.token.as_deref().unwrap_or("");
    let headers = platform_headers();
    let amount_url = format!("{PLATFORM_BASE}/api/v0/usage/amount?month={month}&year={year}");
    let cost_url = format!("{PLATFORM_BASE}/api/v0/usage/cost?month={month}&year={year}");

    let amount = http
        .get_json_h(&amount_url, Some(token), s.cookie.as_deref(), &headers)
        .await
        .map_err(auth_error)?;
    ensure_ok(&amount).map_err(auth_error)?;
    if !has_payload(&amount) {
        return Err("用量接口未返回数据（接口可能已再次改版）".to_string());
    }
    let cost = http
        .get_json_h(&cost_url, Some(token), s.cookie.as_deref(), &headers)
        .await
        .unwrap_or(Value::Null);

    Ok(parse_month(&amount, &cost))
}

/// 新版结构的归一化：`series[].buckets[]` → 日 / 模型维度
fn parse_range(amount: &Value, cost: &Value, start: i64, end: i64) -> MonthData {
    let mut out = MonthData::default();
    let amount_biz = extract_biz(amount);
    let cost_biz = extract_biz(cost);

    // 消费：`data[].series[].buckets[].cost`，按币种分开聚合
    let mut cost_by_day: BTreeMap<String, f64> = BTreeMap::new();
    let mut cost_by_currency: BTreeMap<String, f64> = BTreeMap::new();
    for cur in arr(cost_biz.get("data")) {
        let currency = pick_string(cur, &["currency"]).unwrap_or_default();
        let mut subtotal = 0.0;
        for series in arr(cur.get("series")) {
            for b in arr(series.get("buckets")) {
                let Some(ts) = bucket_time(b) else { continue };
                if !in_range(ts, start, end) {
                    continue;
                }
                let c = pick_f64(b, &["cost", "amount", "value"]).unwrap_or(0.0);
                subtotal += c;
                *cost_by_day.entry(ts_to_date(ts)).or_insert(0.0) += c;
            }
        }
        *cost_by_currency.entry(currency).or_insert(0.0) += subtotal;
    }
    // 账户通常只有一种币种；有人民币就优先人民币
    out.cost = cost_by_currency
        .get("CNY")
        .copied()
        .unwrap_or_else(|| cost_by_currency.values().sum());

    // 用量：按模型聚合，同时切到日维度
    for series in arr(amount_biz.get("series")) {
        let model =
            pick_string(series, &["model", "model_name", "modelName"]).unwrap_or_else(|| "未标注".to_string());
        let (mut m_req, mut m_tok) = (0.0, 0.0);
        for b in arr(series.get("buckets")) {
            let Some(ts) = bucket_time(b) else { continue };
            if !in_range(ts, start, end) {
                continue;
            }
            let (tokens, requests, hit, miss, resp) = usage_object(b.get("usage"));
            let date = ts_to_date(ts);
            let day = out.days.entry(date.clone()).or_insert(DailyUsage {
                date,
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
            day.requests += requests;
            day.tokens += tokens;

            m_req += requests;
            m_tok += tokens;
            out.requests += requests;
            out.tokens += tokens;
            out.cache_hit += hit;
            out.cache_miss += miss;
            out.response_tokens += resp;
        }
        let e = out.models.entry(model.clone()).or_insert(ModelUsage {
            model,
            requests: 0.0,
            tokens: 0.0,
            cost: 0.0,
        });
        e.requests += m_req;
        e.tokens += m_tok;
    }

    // 消费按天归位（amount 与 cost 的时间桶对齐）
    for (date, c) in &cost_by_day {
        match out.days.get_mut(date) {
            Some(d) => d.cost = *c,
            None => {
                out.days.insert(
                    date.clone(),
                    DailyUsage {
                        date: date.clone(),
                        requests: 0.0,
                        tokens: 0.0,
                        cost: *c,
                    },
                );
            }
        }
    }

    out
}

/// `usage` 既可能是「类型 → 数值」的对象（新版），也可能是 `{type, amount}` 数组（旧版）
fn usage_object(usage: Option<&Value>) -> (f64, f64, f64, f64, f64) {
    let Some(u) = usage else {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    };
    if u.is_array() {
        return token_breakdown(Some(u));
    }
    let get = |k: &str| pick_f64(u, &[k]).unwrap_or(0.0);
    let requests = get("REQUEST");
    let hit = get("PROMPT_CACHE_HIT_TOKEN");
    let miss = get("PROMPT_CACHE_MISS_TOKEN");
    let resp = get("RESPONSE_TOKEN");
    let prompt = get("PROMPT_TOKEN");
    let tokens = if hit + miss + resp > 0.0 {
        hit + miss + resp
    } else {
        prompt
    };
    (tokens, requests, hit, miss, resp)
}

fn parse_month(amount: &Value, cost: &Value) -> MonthData {
    let mut out = MonthData::default();
    let amount_biz = biz_data(amount);
    let cost_biz = cost_biz_data(cost);

    // 成本：按模型
    let mut cost_by_model: BTreeMap<String, f64> = BTreeMap::new();
    for b in array_at(&cost_biz, "total") {
        let model = pick_string(b, &["model", "model_name", "modelName"]).unwrap_or_default();
        *cost_by_model.entry(model).or_insert(0.0) += cost_sum(b.get("usage"));
    }
    // 成本：按天
    let mut cost_by_day: BTreeMap<String, f64> = BTreeMap::new();
    for d in array_at(&cost_biz, "days") {
        let date = pick_string(d, &["date"]).unwrap_or_default();
        if date.is_empty() {
            continue;
        }
        let mut sum = 0.0;
        for b in arr(d.get("data")) {
            sum += cost_sum(b.get("usage"));
        }
        cost_by_day.insert(date, sum);
    }

    // tokens / 请求数：按模型
    for b in array_at(&amount_biz, "total") {
        let model = pick_string(b, &["model", "model_name", "modelName"])
            .unwrap_or_else(|| "未标注".to_string());
        let (tokens, requests, hit, miss, resp) = token_breakdown(b.get("usage"));
        let m = out
            .models
            .entry(model.clone())
            .or_insert(ModelUsage {
                model: model.clone(),
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
        m.requests += requests;
        m.tokens += tokens;
        m.cost += cost_by_model.get(&model).copied().unwrap_or(0.0);

        out.requests += requests;
        out.tokens += tokens;
        out.cache_hit += hit;
        out.cache_miss += miss;
        out.response_tokens += resp;
    }

    out.cost = if !cost_by_day.is_empty() {
        cost_by_day.values().sum()
    } else {
        cost_by_model.values().sum()
    };

    // tokens / 请求数：按天
    for d in array_at(&amount_biz, "days") {
        let date = pick_string(d, &["date"]).unwrap_or_default();
        if date.is_empty() {
            continue;
        }
        let day = out
            .days
            .entry(date.clone())
            .or_insert(DailyUsage {
                date,
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
        for b in arr(d.get("data")) {
            let (tokens, requests, _, _, _) = token_breakdown(b.get("usage"));
            day.tokens += tokens;
            day.requests += requests;
        }
        day.cost = cost_by_day.get(&day.date).copied().unwrap_or(0.0);
    }

    out
}

fn biz_data(v: &Value) -> Value {
    v.get("data")
        .and_then(|d| d.get("biz_data"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// cost 接口的 `biz_data` 是数组，取第一项。
fn cost_biz_data(v: &Value) -> Value {
    let b = biz_data(v);
    match b.as_array() {
        Some(a) => a.first().cloned().unwrap_or(Value::Null),
        None => b,
    }
}

fn array_at<'a>(obj: &'a Value, key: &str) -> Vec<&'a Value> {
    pick(obj, &[key])
        .and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

fn arr<'a>(v: Option<&'a Value>) -> Vec<&'a Value> {
    v.and_then(|v| v.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// 汇总一个 usage 块：(tokens, requests, cache_hit, cache_miss, response_tokens)
fn token_breakdown(usage: Option<&Value>) -> (f64, f64, f64, f64, f64) {
    let (mut tokens, mut requests, mut hit, mut miss, mut resp) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for e in arr(usage) {
        let kind = pick_string(e, &["type"]).unwrap_or_default();
        let amount = pick_f64(e, &["amount", "value", "count"]).unwrap_or(0.0);
        match kind.as_str() {
            "REQUEST" => requests += amount,
            "PROMPT_CACHE_HIT_TOKEN" => {
                hit += amount;
                tokens += amount;
            }
            "PROMPT_CACHE_MISS_TOKEN" => {
                miss += amount;
                tokens += amount;
            }
            "RESPONSE_TOKEN" => {
                resp += amount;
                tokens += amount;
            }
            "PROMPT_TOKEN" => tokens += amount,
            other => {
                if other.contains("TOKEN") {
                    tokens += amount;
                }
            }
        }
    }
    (tokens, requests, hit, miss, resp)
}

/// 金额求和（REQUEST 是次数，不是金额）
fn cost_sum(usage: Option<&Value>) -> f64 {
    arr(usage)
        .iter()
        .filter(|e| pick_string(e, &["type"]).unwrap_or_default() != "REQUEST")
        .map(|e| pick_f64(e, &["amount", "value"]).unwrap_or(0.0))
        .sum()
}

fn apply_usage(snap: &mut UsageSnapshot, bundle: UsageBundle, currency: &str) {
    let mut days: BTreeMap<String, DailyUsage> = BTreeMap::new();
    let mut models: BTreeMap<String, ModelUsage> = BTreeMap::new();

    for m in [&bundle.previous, &bundle.current] {
        for (date, d) in &m.days {
            let e = days.entry(date.clone()).or_insert(DailyUsage {
                date: date.clone(),
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
            e.requests += d.requests;
            e.tokens += d.tokens;
            e.cost += d.cost;
        }
        for (name, mdl) in &m.models {
            let e = models.entry(name.clone()).or_insert(ModelUsage {
                model: name.clone(),
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
            e.requests += mdl.requests;
            e.tokens += mdl.tokens;
            e.cost += mdl.cost;
        }
    }

    // 趋势图按「近 30 天」展示，跨月取到的多余天数裁掉
    let all: Vec<DailyUsage> = days.into_values().collect();
    let start = all.len().saturating_sub(30);
    snap.days = all[start..].to_vec();

    snap.models = models.into_values().collect();
    snap.models
        .sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));

    let cur = &bundle.current;
    let prev = &bundle.previous;
    let today = today_str();
    let (month_from, month_to) = month_range();

    snap.month = Some(PeriodUsage {
        requests: cur.requests,
        tokens: cur.tokens,
        cost: cur.cost,
        from: month_from,
        to: month_to,
    });
    snap.used = Some(cur.cost + prev.cost);
    snap.requests_total = Some(cur.requests + prev.requests);
    snap.tokens_total = Some(cur.tokens + prev.tokens);
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

    let money = |v: f64| format!("{v:.2} {currency}");
    let int = |v: f64| format!("{}", v.round() as i64);

    snap.metrics.push(Metric {
        label: format!("累计消费金额（{} ~ {}）", bundle.previous_label, bundle.current_label),
        value: money(cur.cost + prev.cost),
        hint: Some("平台按自然月返回，这里合计最近两个自然月".to_string()),
    });
    snap.metrics.push(Metric {
        label: format!("本月消费（{}）", bundle.current_label),
        value: money(cur.cost),
        hint: Some(format!("{} ~ {}", month_range().0, month_range().1)),
    });
    snap.metrics.push(Metric {
        label: format!("上月消费（{}）", bundle.previous_label),
        value: money(prev.cost),
        hint: None,
    });
    snap.metrics.push(Metric {
        label: "本月 API 请求".to_string(),
        value: int(cur.requests),
        hint: Some(format!("上月 {}", int(prev.requests))),
    });
    snap.metrics.push(Metric {
        label: "本月 Token 用量".to_string(),
        value: int(cur.tokens),
        hint: Some(format!("上月 {}", int(prev.tokens))),
    });
    if let Some(t) = &snap.today {
        snap.metrics.push(Metric {
            label: "今日消费".to_string(),
            value: money(t.cost),
            hint: Some(format!("{} 次请求 · {} tokens", int(t.requests), int(t.tokens))),
        });
    }
    if cur.cache_hit + cur.cache_miss > 0.0 {
        let rate = cur.cache_hit / (cur.cache_hit + cur.cache_miss) * 100.0;
        snap.metrics.push(Metric {
            label: "输入缓存命中率".to_string(),
            value: format!("{rate:.1}%"),
            hint: Some(format!(
                "命中 {} / 未命中 {} / 输出 {}",
                int(cur.cache_hit),
                int(cur.cache_miss),
                int(cur.response_tokens)
            )),
        });
    }
    snap.metrics.push(Metric {
        label: "用量数据来源".to_string(),
        value: "DeepSeek 平台内部接口".to_string(),
        hint: Some("接口为平台前端自用，若失效则无法读取用量".to_string()),
    });
}

fn month_range() -> (String, String) {
    let now = chrono::Local::now();
    let start = format!("{:04}-{:02}-01", now.year(), now.month());
    (start, today_str())
}

// ---------------------------------------------------------------------------
// API Keys
// ---------------------------------------------------------------------------

async fn fetch_api_keys(http: &Http, s: &Session) -> Vec<ApiKeyInfo> {
    let token = match s.token.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return Vec::new(),
    };
    let paths = [
        "/api/v0/api_keys",
        "/api/v0/api_key/list",
        "/api/v0/keys",
        "/api/v0/user/api_keys",
        "/api/v0/users/api_keys",
    ];
    for p in paths {
        let url = format!("{PLATFORM_BASE}{p}");
        if let Ok(v) = http
            .get_json_h(&url, Some(token), s.cookie.as_deref(), &platform_headers())
            .await
        {
            if let Some(list) = extract_key_list(&v) {
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }
    Vec::new()
}

/// 平台接口结构未公开，这里在任意层级里找「最像 API Key 列表」的对象数组。
fn extract_key_list(v: &Value) -> Option<Vec<ApiKeyInfo>> {
    let mut arrays: Vec<&Vec<Value>> = Vec::new();
    record_arrays(v, &mut arrays, 0);

    let mut best: Option<&Vec<Value>> = None;
    let mut best_score = 0usize;
    for a in arrays {
        let score = a.iter().take(20).filter(|r| is_key_like(r)).count();
        if score > 0 && score > best_score {
            best_score = score;
            best = Some(a);
        }
    }
    best.map(|a| a.iter().map(to_key_info).collect())
}

fn is_key_like(r: &Value) -> bool {
    let has_name = pick_string(r, &[
        "name",
        "key_name",
        "keyName",
        "label",
        "title",
        "remark",
        "alias",
    ])
    .is_some();
    let has_key = pick_string(r, &[
        "key",
        "api_key",
        "apiKey",
        "secret",
        "value",
        "masked_key",
        "maskedKey",
        "token",
    ])
    .is_some();
    let has_meta = pick_string(r, &["created_at", "createdAt", "created", "create_time", "id"])
        .is_some();
    has_name && (has_key || has_meta)
}

fn to_key_info(r: &Value) -> ApiKeyInfo {
    let name = pick_string(r, &[
        "name",
        "key_name",
        "keyName",
        "label",
        "title",
        "remark",
        "alias",
    ])
    .unwrap_or_else(|| "未命名".to_string());
    let raw = pick_string(r, &[
        "key",
        "api_key",
        "apiKey",
        "secret",
        "value",
        "masked_key",
        "maskedKey",
    ]);
    ApiKeyInfo {
        key_type: Some("api".to_string()),
        id: pick_string(r, &["id", "key_id", "keyId"]).unwrap_or_else(|| name.clone()),
        name,
        masked: raw.as_deref().map(mask_secret),
        secret: None,
        created_at: short_ts(pick_string(r, &[
            "created_at",
            "createdAt",
            "created",
            "create_time",
            "createTime",
        ])),
        last_used_at: short_ts(pick_string(r, &[
            "last_used_at",
            "lastUsedAt",
            "last_used",
            "last_use_time",
            "lastUseTime",
        ])),
        status: pick_string(r, &["status", "state", "enable", "enabled"]).or(Some("正常".to_string())),
        source: "api".to_string(),
    }
}

/// 平台返回的完整 Key 只在创建时出现一次；列表里通常是脱敏值，统一再脱一次敏。
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

fn short_ts(s: Option<String>) -> Option<String> {
    s.map(|v| {
        if v.len() > 19 {
            v[..19].to_string()
        } else {
            v
        }
    })
    .filter(|v| !v.trim().is_empty())
}

/// 平台读到的 keys + 本地手动录入的 keys（按名称去重，手动录入优先补全）。
fn merge_keys(remote: Vec<ApiKeyInfo>, cfg: &ProviderConfig) -> Vec<ApiKeyInfo> {
    let mut out: Vec<ApiKeyInfo> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

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
        // 完整密钥只留在 providers.json，不写进快照
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
// 连通性测试
// ---------------------------------------------------------------------------

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    match cfg.mode() {
        AccessMode::Api => {
            let api_key = cfg.token.clone().unwrap_or_default();
            let api_key = api_key.trim().to_string();
            if api_key.is_empty() {
                return Err("请先填写 API Key".to_string());
            }
            let base = cfg
                .base_url
                .clone()
                .unwrap_or_else(|| API_BASE.to_string());
            let url = crate::http::join_url(&base, "/user/balance");
            let v = http.get_json(&url, Some(&api_key), None).await?;
            let infos = v
                .get("balance_infos")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            if infos.is_empty() && pick_f64(&v, &["total_balance", "balance"]).is_none() {
                return Err("接口连通，但未识别到余额字段".to_string());
            }
            let total = infos
                .first()
                .and_then(|i| pick_f64(i, &["total_balance"]))
                .or_else(|| pick_f64(&v, &["total_balance", "balance"]))
                .unwrap_or(0.0);
            let currency = infos
                .first()
                .and_then(|i| pick_string(i, &["currency"]))
                .unwrap_or_else(|| "CNY".to_string());
            parts.push(format!("余额 {total:.2} {currency}"));
        }
        AccessMode::Login => {
            let s = resolve_session(cfg, http, true)
                .await?
                .ok_or_else(|| "未获取到登录凭证，请点击「登录获取」重新授权".to_string())?;
            let u = fetch_usage(http, &s).await?;
            parts.push(format!(
                "本月消费 {:.2}，请求 {} 次，Token {}",
                u.current.cost,
                u.current.requests.round() as i64,
                u.current.tokens.round() as i64
            ));
            let keys = fetch_api_keys(http, &s).await;
            parts.push(format!("读取到 {} 个 API Key", keys.len()));
            if let Some(a) = &s.account {
                parts.insert(0, format!("账号 {a}"));
            }
        }
    }

    if parts.is_empty() {
        return Err("未能读取到任何数据".to_string());
    }
    Ok(format!("连接成功，{}", parts.join("，")))
}
