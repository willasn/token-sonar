//! 腾讯云 TokenHub 管控面（2026-03-22）与费用中心（2018-07-09）。
//! 公开 API 仅列出企业版 Token Plan；不使用控制台私有接口推断个人套餐。
//! https://cloud.tencent.com/document/api/1823/132289
//! https://cloud.tencent.com/document/api/555/20253

use chrono::{Datelike, FixedOffset, TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::util::{now_iso, pick_f64, pick_string};
use crate::http::Http;
use crate::models::{Metric, ProviderConfig, QuotaWindow, UsageSnapshot};

const CONTENT_TYPE: &str = "application/json; charset=utf-8";
const SIGNED_HEADERS: &str = "content-type;host;x-tc-action";
const REGION: &str = "ap-guangzhou";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn authorization(
    id: &str,
    secret: &str,
    service: &str,
    action: &str,
    payload: &str,
    timestamp: i64,
) -> String {
    let date = chrono::DateTime::from_timestamp(timestamp, 0)
        .expect("current timestamp")
        .format("%Y-%m-%d")
        .to_string();
    let host = format!("{service}.tencentcloudapi.com");
    let headers = format!(
        "content-type:{CONTENT_TYPE}\nhost:{host}\nx-tc-action:{}\n",
        action.to_ascii_lowercase()
    );
    let canonical = format!(
        "POST\n/\n\n{headers}\n{SIGNED_HEADERS}\n{}",
        hex(&Sha256::digest(payload.as_bytes()))
    );
    let scope = format!("{date}/{service}/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let date_key = hmac(format!("TC3{secret}").as_bytes(), &date);
    let service_key = hmac(&date_key, service);
    let signing_key = hmac(&service_key, "tc3_request");
    format!(
        "TC3-HMAC-SHA256 Credential={id}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={}",
        hex(&hmac(&signing_key, &string_to_sign))
    )
}

fn credentials(cfg: &ProviderConfig) -> Result<(&str, &str), String> {
    let id = cfg.access_key_id.as_deref().unwrap_or_default().trim();
    let secret = cfg.access_key_secret.as_deref().unwrap_or_default().trim();
    if id.is_empty() || secret.is_empty() {
        return Err("请填写腾讯云 SecretId（AK）与 SecretKey（SK），不是模型 API Key".into());
    }
    if !id.is_ascii() || id.chars().any(char::is_whitespace) {
        return Err("SecretId 格式无效".into());
    }
    Ok((id, secret))
}

fn response(status: u16, value: Value, action: &str) -> Result<Value, String> {
    let r = value
        .get("Response")
        .ok_or_else(|| format!("{action}: 响应缺少 Response（HTTP {status}）"))?;
    if let Some(error) = r.get("Error").filter(|v| !v.is_null()) {
        // 不回显服务端 Message，避免第三方响应包含凭证或账号信息。
        let code = pick_string(error, &["Code"]).unwrap_or_else(|| "UnknownError".into());
        return Err(format!(
            "{action}: {code}（请检查 CAM 权限、密钥及系统时间）"
        ));
    }
    if !(200..300).contains(&status) {
        return Err(format!("{action}: HTTP {status}"));
    }
    Ok(r.clone())
}

async fn call(
    http: &Http,
    creds: (&str, &str),
    service: &str,
    action: &str,
    body: &Value,
) -> Result<Value, String> {
    let version = if service == "billing" {
        "2018-07-09"
    } else {
        "2026-03-22"
    };
    let host = format!("{service}.tencentcloudapi.com");
    let timestamp = Utc::now().timestamp();
    let payload = serde_json::to_string(body).map_err(|e| format!("请求序列化失败: {e}"))?;
    let auth = authorization(creds.0, creds.1, service, action, &payload, timestamp);
    let timestamp = timestamp.to_string();
    let mut headers = vec![
        ("Content-Type", CONTENT_TYPE),
        ("Host", host.as_str()),
        ("X-TC-Action", action),
        ("X-TC-Version", version),
        ("X-TC-Timestamp", timestamp.as_str()),
        ("Authorization", auth.as_str()),
    ];
    if service == "tokenhub" {
        headers.push(("X-TC-Region", REGION));
    }
    // 固定官方域名，避免把云密钥签名发送到用户填写的任意站点。
    let (status, value) = http
        .request_json(
            reqwest::Method::POST,
            &format!("https://{host}/"),
            Some(body),
            &headers,
        )
        .await?;
    response(status, value, action)
}

fn metric(
    snap: &mut UsageSnapshot,
    label: impl Into<String>,
    value: impl Into<String>,
    hint: &str,
) {
    snap.metrics.push(Metric {
        label: label.into(),
        value: value.into(),
        hint: Some(hint.into()),
    });
}

fn apply_balance(snap: &mut UsageSnapshot, r: &Value) -> Result<(), String> {
    let raw = pick_f64(r, &["RealBalance", "Balance"])
        .ok_or("DescribeAccountBalance: 未返回真实可用余额")?;
    snap.balance_raw = Some(raw);
    snap.balance = Some(raw / 100.0);
    snap.currency = "CNY".into();
    metric(
        snap,
        "腾讯云整体余额",
        format!("¥{:.2}", raw / 100.0),
        "费用中心账户整体余额，非混元或 TokenHub 专属余额；原始单位为分",
    );
    Ok(())
}

fn apply_plan(snap: &mut UsageSnapshot, r: &Value) {
    let name = pick_string(r, &["Name"]).unwrap_or_else(|| "Token Plan".into());
    let product = pick_string(r, &["ProductType"]).unwrap_or_default();
    let unit = match product.as_str() {
        "enterprise" => "credits",
        "enterprise-auto" => "tokens",
        _ => "单位未公开",
    };
    let status = pick_string(r, &["Status"]).unwrap_or_else(|| "未知".into());
    metric(
        snap,
        format!("套餐 · {name}"),
        format!("{product} / {status}"),
        "来自 DescribeTokenPlan；仅为接口可见的企业版套餐，不能代表个人订阅状态",
    );
    let p = &r["PackageInfo"];
    if let (Some(quota), Some(used)) = (pick_f64(p, &["TotalQuota"]), pick_f64(p, &["TotalUsed"])) {
        snap.quotas.push(QuotaWindow {
            label: format!("{name} · 总额度（{unit}）"),
            used,
            quota,
            percent: if quota > 0.0 {
                used / quota * 100.0
            } else {
                0.0
            },
            // ExpireTime 是到期时间，不是滚动窗口重置时间。
            reset_at: None,
            reset_in: None,
        });
    } else {
        metric(
            snap,
            format!("{name} · 额度"),
            "不可读取",
            "接口未返回完整 TotalQuota / TotalUsed，不以零额度兜底",
        );
    }
    for (field, label) in [
        ("CycleQuota", "当期额度"),
        ("SharedPool", "共享池额度"),
        ("SharedUsed", "共享已用"),
        ("ExclusiveUsed", "独占已用"),
    ] {
        if let Some(value) = pick_f64(p, &[field]) {
            metric(
                snap,
                format!("{name} · {label}"),
                format!("{value} {unit}"),
                "PackageInfo 官方返回值；credits 与 tokens 不做换算",
            );
        }
    }
    if let Some(expire) = pick_string(p, &["ExpireTime"]) {
        metric(
            snap,
            format!("{name} · 到期"),
            expire,
            "套餐包到期时间，不是滚动额度重置时间",
        );
    }
    if let Some(items) = r["TokenSummary"]["BillingItems"].as_array() {
        for item in items {
            if let (Some(kind), Some(qty)) = (
                pick_string(item, &["BillingItem"]),
                pick_f64(item, &["TotalQty"]),
            ) {
                let unit = if kind == "call_count" {
                    "次"
                } else {
                    "tokens"
                };
                metric(
                    snap,
                    format!("{name} · 本周期 {kind}"),
                    format!("{qty} {unit}"),
                    "TokenSummary 周期统计；各计费项单独展示，避免缓存重复计数",
                );
            }
        }
    }
}

async fn fetch_plans(
    http: &Http,
    creds: (&str, &str),
    snap: &mut UsageSnapshot,
) -> Result<(), String> {
    let mut offset = 0usize;
    loop {
        let r = call(
            http,
            creds,
            "tokenhub",
            "DescribeTokenPlanList",
            &json!({"Limit": 100, "Offset": offset}),
        )
        .await?;
        let items = r["TokenPlanSet"]
            .as_array()
            .ok_or("DescribeTokenPlanList: 未返回套餐列表")?;
        let total = r["TotalCount"]
            .as_u64()
            .ok_or("DescribeTokenPlanList: 未返回套餐总数")? as usize;
        for item in items {
            if let Some(id) = pick_string(item, &["TeamId"]) {
                match call(
                    http,
                    creds,
                    "tokenhub",
                    "DescribeTokenPlan",
                    &json!({"TeamId": id}),
                )
                .await
                {
                    Ok(detail) => apply_plan(snap, &detail),
                    Err(e) => {
                        apply_plan(snap, item);
                        metric(
                            snap,
                            "套餐详情不可读取",
                            e,
                            "保留列表中已返回的数据，不推算缺失用量",
                        );
                    }
                }
            } else {
                return Err("DescribeTokenPlanList: 套餐缺少 TeamId".into());
            }
        }
        offset += items.len();
        if offset >= total {
            metric(
                snap,
                "可见企业版套餐",
                total.to_string(),
                "主账号可查看全部，子账号仅可查看自己创建的；不包含已验证不可公开读取的个人套餐",
            );
            return Ok(());
        }
        if items.is_empty() {
            return Err("DescribeTokenPlanList: 分页数据不完整".into());
        }
    }
}

fn apply_usage(snap: &mut UsageSnapshot, r: &Value, label: &str) -> Result<(), String> {
    let tokens = pick_f64(&r["TotalStats"], &["TotalToken"])
        .ok_or("DescribeUsageRankList: 未返回 TotalStats.TotalToken，不能视为零用量")?;
    snap.supports_usage_api = true;
    metric(snap, format!("TokenHub · {label} Token"), tokens.to_string(), "广州地域 TokenHub 账户 Token 统计；包含其他模型，接口不提供独立按量账单费用，不能认定全部为混元付费用量");
    if let Some(items) = r["TopList"].as_array() {
        for item in items {
            if let (Some(model), Some(tokens)) = (
                pick_string(item, &["Key"]),
                pick_f64(&item["Stats"], &["TotalToken"]),
            ) {
                metric(
                    snap,
                    format!("{label} · {model}"),
                    format!("{tokens} tokens"),
                    "TokenHub 模型维度真实返回值；总 Token 已包含缓存输入，不再次累加缓存",
                );
            }
        }
    }
    Ok(())
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();
    snap.currency = "CNY".into();
    metric(
        &mut snap,
        "统计范围",
        "TokenHub / 广州",
        "固定官方管控接口，非推理 Base URL；新加坡及传统混元历史数据未合并",
    );
    metric(&mut snap, "个人 Coding / Token Plan", "公开接口不可读取", "当前公开套餐接口只定义 enterprise / enterprise-auto；无法读取个人订阅状态、5小时/周/月额度，不能以企业版套餐为空判断未购买");
    metric(
        &mut snap,
        "费用与请求数",
        "不可读取",
        "Token 用量统计不等于付费消耗；未读取账单费用、全账户请求数及历史累计量，不以零值兜底",
    );
    let creds = match credentials(cfg) {
        Ok(creds) => creds,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };
    let mut notices = Vec::new();
    match call(http, creds, "billing", "DescribeAccountBalance", &json!({})).await {
        Ok(r) => match apply_balance(&mut snap, &r) {
            Ok(()) => snap.ok = true,
            Err(e) => notices.push(e),
        },
        Err(e) => notices.push(e),
    }
    match fetch_plans(http, creds, &mut snap).await {
        Ok(()) => snap.ok = true,
        Err(e) => notices.push(e),
    }
    let tz = FixedOffset::east_opt(8 * 3600).expect("UTC+8");
    let now = Utc::now().with_timezone(&tz);
    let today = tz
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .expect("fixed timezone");
    let month = tz
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("fixed timezone");
    for (label, start) in [("今日", today), ("本月", month)] {
        let body = json!({"Dimension": "model", "MetricType": "tokens", "StartTime": start.to_rfc3339(), "EndTime": now.to_rfc3339(), "ShowAll": true});
        match call(http, creds, "tokenhub", "DescribeUsageRankList", &body).await {
            Ok(r) => match apply_usage(&mut snap, &r, label) {
                Ok(()) => snap.ok = true,
                Err(e) => notices.push(format!("{label}: {e}")),
            },
            Err(e) => notices.push(format!("{label}: {e}")),
        }
    }
    if !notices.is_empty() {
        snap.error = Some(notices.join("；"));
    }
    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let creds = credentials(cfg)?;
    let r = call(
        http,
        creds,
        "tokenhub",
        "DescribeTokenPlanList",
        &json!({"Limit": 1, "Offset": 0}),
    )
    .await?;
    if !r["TokenPlanSet"].is_array() {
        return Err("DescribeTokenPlanList: 未返回套餐列表".into());
    }
    Ok(
        "TokenHub AK/SK 签名验证通过（广州）；余额及用量需要对应 CAM 权限，个人套餐额度无公开接口"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProviderKind;

    fn snapshot() -> UsageSnapshot {
        let cfg: ProviderConfig =
            serde_json::from_value(json!({"id": "demo", "name": "Demo", "kind": "hunyuan"}))
                .unwrap();
        UsageSnapshot::empty(&cfg)
    }

    #[test]
    fn tc3_signature_matches_independent_fixture_and_utc_date() {
        let auth = authorization(
            "example-id",
            "example-secret",
            "tokenhub",
            "DescribeTokenPlanList",
            "{\"Limit\":100,\"Offset\":0}",
            1551113065,
        );
        assert_eq!(auth, "TC3-HMAC-SHA256 Credential=example-id/2019-02-25/tokenhub/tc3_request, SignedHeaders=content-type;host;x-tc-action, Signature=a464658b464de87e9eb76e1cca01503c335b609906696843a08c10e39ddaab09");
        assert_ne!(
            auth,
            authorization(
                "example-id",
                "example-secret",
                "tokenhub",
                "DescribeTokenPlanList",
                "{}",
                1551113065
            )
        );
    }

    #[test]
    fn kind_defaults_and_balance_cents() {
        assert_eq!(
            serde_json::to_value(ProviderKind::Hunyuan).unwrap(),
            json!("hunyuan")
        );
        assert_eq!(
            ProviderKind::Hunyuan.default_base_url(),
            "https://tokenhub.tencentcloudapi.com"
        );
        let mut s = snapshot();
        apply_balance(&mut s, &json!({"RealBalance": -125.5, "Balance": 0})).unwrap();
        assert_eq!(s.balance, Some(-1.255));
        assert_eq!(s.balance_raw, Some(-125.5));
        assert!(s.metrics[0].hint.as_ref().unwrap().contains("非混元"));
        assert!(apply_balance(&mut snapshot(), &json!({})).is_err());
    }

    #[test]
    fn plan_units_and_missing_quota_are_not_invented() {
        let mut s = snapshot();
        apply_plan(
            &mut s,
            &json!({"Name": "Demo plan", "ProductType": "enterprise", "PackageInfo": {"TotalQuota": "1000", "TotalUsed": "250", "CycleQuota": "500", "ExpireTime": "2026-11-01T00:00:00Z"}}),
        );
        assert_eq!(s.quotas[0].quota, 1000.0);
        assert_eq!(s.quotas[0].percent, 25.0);
        assert!(s.quotas[0].label.contains("credits"));
        assert_eq!(s.quotas[0].reset_at, None);
        apply_plan(
            &mut s,
            &json!({"ProductType": "enterprise-auto", "PackageInfo": {"TotalQuota": "10", "TotalUsed": "0"}}),
        );
        assert!(s.quotas[1].label.contains("tokens"));
        let mut missing = snapshot();
        apply_plan(&mut missing, &json!({"PackageInfo": {"TotalQuota": "10"}}));
        assert!(missing.quotas.is_empty());
    }

    #[test]
    fn usage_null_is_unknown_but_explicit_zero_is_valid() {
        let mut s = snapshot();
        assert!(apply_usage(&mut s, &json!({"TotalStats": null}), "本月").is_err());
        assert!(!s.supports_usage_api);
        apply_usage(&mut s, &json!({"TotalStats": {"TotalToken": 12, "InputTotalToken": 7, "OutputTotalToken": 5, "CacheTotalToken": 3}, "TopList": [{"Key": "demo-model", "Stats": {"TotalToken": 12}}]}), "本月").unwrap();
        assert_eq!(s.metrics[0].value, "12");
        assert!(s.today.is_none() && s.month.is_none() && s.tokens_total.is_none());
        assert!(s.days.is_empty());
        apply_usage(&mut s, &json!({"TotalStats": {"TotalToken": 0}}), "今日").unwrap();
        assert_eq!(s.metrics.last().unwrap().value, "0");
    }

    #[test]
    fn api_errors_never_echo_server_message() {
        let e = response(200, json!({"Response": {"Error": {"Code": "AuthFailure.SignatureFailure", "Message": "sensitive-placeholder"}}}), "DescribeTokenPlanList").unwrap_err();
        assert!(e.contains("AuthFailure.SignatureFailure"));
        assert!(!e.contains("sensitive-placeholder"));
        assert!(response(200, json!({}), "Test").is_err());
        assert!(response(403, json!({"Response": {}}), "Test").is_err());
    }
}
