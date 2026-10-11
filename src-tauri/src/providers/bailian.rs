//! 百炼的账户余额与当月已出账计费项；不代表实时模型用量。

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{FixedOffset, Utc};
use hmac::{Hmac, Mac};
use reqwest::Method;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::util::{now_iso, pick_f64};
use crate::http::Http;
use crate::models::{
    BailianPlan, Metric, ModelUsage, PeriodUsage, ProviderConfig, QuotaWindow, UsageSnapshot,
};

const HOST: &str = "business.aliyuncs.com";
const ENDPOINT: &str = "https://business.aliyuncs.com";
const VERSION: &str = "2017-12-14";
const INFERENCE: &str = "sfm_inference_public_cn";
const DEPLOYMENT: &str = "sfm_deployment_public_cn";
const TRAINING: &str = "sfm_training_public_cn";
static NONCE: AtomicU64 = AtomicU64::new(0);

fn credentials(cfg: &ProviderConfig) -> Result<(&str, &str), String> {
    let endpoint = if cfg.plan_type == Some(BailianPlan::TokenPlanTeam) {
        "https://modelstudio.cn-beijing.aliyuncs.com"
    } else {
        ENDPOINT
    };
    let base = cfg
        .base_url
        .as_deref()
        .unwrap_or(endpoint)
        .trim()
        .trim_end_matches('/');
    if !base.is_empty() && base != endpoint && !(endpoint == ENDPOINT && base == HOST) {
        return Err(format!("此模式仅允许官方地址 {endpoint}"));
    }
    let ak = cfg.access_key_id.as_deref().unwrap_or("").trim();
    let sk = cfg.access_key_secret.as_deref().unwrap_or("").trim();
    if ak.is_empty() || sk.is_empty() {
        return Err("请填写阿里云 AccessKey ID 和 AccessKey Secret（不是百炼 API Key）".into());
    }
    if ak.bytes().any(|c| !c.is_ascii_alphanumeric()) || sk.bytes().any(|c| c.is_ascii_control()) {
        return Err("AccessKey 凭证格式无效".into());
    }
    Ok((ak, sk))
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for c in s.bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

fn query_string(params: &BTreeMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// 官方 V3 规范：RPC 路径为 /，所有 x-acs-*、host、content-type 参与签名。
fn authorization(ak: &str, sk: &str, query: &str, headers: &BTreeMap<String, String>) -> String {
    sign_request(ak, sk, "POST", "/", query, headers)
}

fn sign_request(
    ak: &str,
    sk: &str,
    method: &str,
    path: &str,
    query: &str,
    headers: &BTreeMap<String, String>,
) -> String {
    let canonical_headers = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect::<String>();
    let signed = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let payload_hash = hex(&Sha256::digest(b""));
    let canonical =
        format!("{method}\n{path}\n{query}\n{canonical_headers}\n{signed}\n{payload_hash}");
    let string_to_sign = format!(
        "ACS3-HMAC-SHA256\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let mut mac =
        Hmac::<Sha256>::new_from_slice(sk.as_bytes()).expect("HMAC accepts any key length");
    mac.update(string_to_sign.as_bytes());
    format!(
        "ACS3-HMAC-SHA256 Credential={ak},SignedHeaders={signed},Signature={}",
        hex(&mac.finalize().into_bytes())
    )
}

fn api_result(status: u16, value: Value) -> Result<Value, String> {
    if (200..300).contains(&status) && value["Success"].as_bool() == Some(true) {
        return value
            .get("Data")
            .filter(|d| d.is_object())
            .cloned()
            .ok_or_else(|| "BSS 响应缺少 Data".into());
    }
    // 不回显 Message、RequestId、账户字段或原始响应；仅接受有界的错误码。
    let code = value["Code"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
        .unwrap_or("UnknownError");
    let hint = match code {
        "NoPermission" | "NotAuthorized" | "Forbidden" | "Forbidden.RAM" => {
            "请检查 RAM 的账户余额和账单只读权限"
        }
        "InvalidAccessKeyId.NotFound" | "SignatureDoesNotMatch" | "InvalidAccessKeyId" => {
            "请检查 AK/SK 和系统时间"
        }
        "Throttling" | "Throttling.User" => "接口限流，请稍后刷新",
        _ => "请检查凭证、权限及 BSS 服务状态",
    };
    Err(format!("BSS HTTP {status} / {code}：{hint}"))
}

async fn call(
    http: &Http,
    ak: &str,
    sk: &str,
    action: &str,
    params: BTreeMap<String, String>,
) -> Result<Value, String> {
    let query = query_string(&params);
    let mut headers = BTreeMap::from([
        ("content-type".into(), "application/json".into()),
        ("host".into(), HOST.into()),
        ("x-acs-action".into(), action.into()),
        ("x-acs-content-sha256".into(), hex(&Sha256::digest(b""))),
        (
            "x-acs-date".into(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ),
        (
            "x-acs-signature-nonce".into(),
            format!(
                "{}-{}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                NONCE.fetch_add(1, Ordering::Relaxed)
            ),
        ),
        ("x-acs-version".into(), VERSION.into()),
    ]);
    let auth = authorization(ak, sk, &query, &headers);
    headers.insert("authorization".into(), auth);
    let extra = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let url = if query.is_empty() {
        format!("{ENDPOINT}/")
    } else {
        format!("{ENDPOINT}/?{query}")
    };
    // Http 的错误可能含 URL / 响应片段，因此在适配器边界统一替换。
    let (status, value) = http
        .request_json(Method::POST, &url, None, &extra)
        .await
        .map_err(|_| "BSS 网络请求失败或响应不是合法 JSON，请检查网络后重试".to_string())?;
    api_result(status, value)
}

fn billing_cycle() -> String {
    Utc::now()
        .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y-%m")
        .to_string()
}

fn bill_params(cycle: &str, cursor: Option<&str>) -> BTreeMap<String, String> {
    let mut params = BTreeMap::from([
        ("BillingCycle".into(), cycle.into()),
        ("Granularity".into(), "MONTHLY".into()),
        ("IsBillingItem".into(), "true".into()),
        ("IsHideZeroCharge".into(), "false".into()),
        ("MaxResults".into(), "300".into()),
    ]);
    if let Some(cursor) = cursor {
        params.insert("NextToken".into(), cursor.into());
    }
    params
}

fn items(data: &Value) -> Result<&Vec<Value>, String> {
    // 兼容 BSS RPC 的历史 Items.Item 封装与当前文档的 Items 数组。
    data["Items"]
        .as_array()
        .or_else(|| data["Items"]["Item"].as_array())
        .ok_or_else(|| "DescribeInstanceBill 响应缺少账单列表".into())
}

fn next_cursor(data: &Value, seen: &mut HashSet<String>) -> Result<Option<String>, String> {
    let cursor = match data.get("NextToken") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) if s.is_empty() => return Ok(None),
        Some(Value::String(s)) => s.clone(),
        _ => return Err("DescribeInstanceBill 分页游标格式无效（已读取数据不完整）".into()),
    };
    if !seen.insert(cursor.clone()) {
        return Err("DescribeInstanceBill 分页游标重复（已读取数据不完整）".into());
    }
    Ok(Some(cursor))
}

fn inference_instance(id: &str) -> Option<(&str, &str)> {
    let parts = id.split(';').collect::<Vec<_>>();
    if parts.len() < 4 || parts[2].trim().is_empty() || id.contains('!') {
        return None;
    }
    Some((parts[2].trim(), parts[3].trim()))
}

fn token_multiplier(unit: &str) -> Option<f64> {
    // 只接纳明确的 Token 单位，绝不从计费项名称或金额反推用量。
    match unit.trim().to_ascii_lowercase().replace(' ', "").as_str() {
        "token" | "tokens" | "个token" => Some(1.0),
        "千token" | "千tokens" | "1000token" | "1000tokens" | "1ktoken" | "1ktokens" => {
            Some(1_000.0)
        }
        "万token" | "万tokens" | "10000token" | "10000tokens" => Some(10_000.0),
        "百万token" | "百万tokens" | "1000000token" | "1000000tokens" | "1mtoken" | "1mtokens" => {
            Some(1_000_000.0)
        }
        _ => None,
    }
}

fn number(row: &Value, key: &str) -> Option<f64> {
    row.get(key)
        .filter(|v| v.is_string() || v.is_number())
        .and_then(|_| pick_f64(row, &[key]))
        .filter(|n| n.is_finite())
}

fn text<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key].as_str().unwrap_or("")
}

#[derive(Default)]
struct Bills {
    records: usize,
    pages: usize,
    currency: Option<String>,
    cost: f64,
    has_cost: bool,
    tokens: f64,
    has_tokens: bool,
    input: Option<f64>,
    output: Option<f64>,
    models: BTreeMap<String, ModelUsage>,
    raw: BTreeMap<(String, String), f64>,
    warnings: HashSet<String>,
    error: Option<String>,
}

impl Bills {
    fn add(&mut self, row: &Value) {
        let commodity = text(row, "CommodityCode");
        if !matches!(commodity, INFERENCE | DEPLOYMENT | TRAINING) {
            return;
        }
        self.records += 1;
        let parsed = if commodity == INFERENCE {
            inference_instance(text(row, "InstanceID"))
        } else {
            None
        };
        let model = parsed
            .map(|p| p.0)
            .unwrap_or(match commodity {
                TRAINING => "百炼训练（未提供模型维度）",
                DEPLOYMENT => "百炼部署（未提供模型维度）",
                _ => "百炼推理（未提供模型维度）",
            })
            .to_string();
        let currency = text(row, "Currency");
        if !matches!(currency, "CNY" | "USD" | "JPY") {
            self.warnings
                .insert("部分账单缺少有效币种，未计入费用汇总；模型费用仅汇总已解析金额，无可解析金额时的 0 不代表真实零费用".into());
        } else if self.currency.as_deref().is_some_and(|c| c != currency) {
            self.warnings
                .insert("账单存在不同币种，未混合相加；不同币种记录未计入汇总".into());
            return;
        } else {
            self.currency = Some(currency.into());
        }
        let entry = self
            .models
            .entry(model.clone())
            .or_insert_with(|| ModelUsage {
                model: model.clone(),
                requests: 0.0,
                tokens: 0.0,
                cost: 0.0,
            });
        if matches!(currency, "CNY" | "USD" | "JPY") {
            if let Some(cost) = number(row, "PretaxAmount") {
                self.has_cost = true;
                self.cost += cost;
                entry.cost += cost;
            } else {
                self.warnings
                    .insert("部分账单缺少可解析 PretaxAmount，费用汇总不完整；模型费用仅汇总已解析金额，无可解析金额时的 0 不代表真实零费用".into());
            }
        }
        let unit = text(row, "UsageUnit");
        let usage = number(row, "Usage");
        if let (Some(usage), Some(multiplier)) = (usage, token_multiplier(unit)) {
            let tokens = usage * multiplier;
            if tokens.is_finite() {
                self.has_tokens = true;
                self.tokens += tokens;
                entry.tokens += tokens;
                if let Some((_, direction)) = parsed {
                    let target = if direction == "input_token"
                        || direction.ends_with("_input_token")
                    {
                        Some(&mut self.input)
                    } else if direction == "output_token" || direction.ends_with("_output_token") {
                        Some(&mut self.output)
                    } else {
                        None
                    };
                    if let Some(target) = target {
                        *target = Some(target.unwrap_or(0.0) + tokens);
                    } else {
                        self.warnings
                            .insert("部分 Token 计费项未提供明确输入/输出类型".into());
                    }
                }
                return;
            }
        }
        if let Some(usage) = usage {
            // 模型与原单位汇总，不输出实例中的 API Key / 工作空间 / 训练任务 ID。
            *self
                .raw
                .entry((
                    model,
                    if unit.is_empty() {
                        "单位未提供".into()
                    } else {
                        unit.to_string()
                    },
                ))
                .or_default() += usage;
        } else {
            self.warnings
                .insert("部分计费项未提供可解析 Usage，用量未知".into());
        }
    }
}

async fn bills(http: &Http, ak: &str, sk: &str, cycle: &str) -> Bills {
    let mut result = Bills::default();
    let mut cursor = None;
    let mut seen = HashSet::new();
    loop {
        let data = match call(
            http,
            ak,
            sk,
            "DescribeInstanceBill",
            bill_params(cycle, cursor.as_deref()),
        )
        .await
        {
            Ok(data) => data,
            Err(e) => {
                result.error = Some(e);
                break;
            }
        };
        let rows = match items(&data) {
            Ok(rows) => rows,
            Err(e) => {
                result.error = Some(e);
                break;
            }
        };
        result.pages += 1;
        for row in rows {
            result.add(row);
        }
        match next_cursor(&data, &mut seen) {
            Ok(Some(next)) => cursor = Some(next),
            Ok(None) => break,
            Err(e) => {
                result.error = Some(e);
                break;
            }
        }
    }
    result
}

fn metric(snap: &mut UsageSnapshot, label: &str, value: impl Into<String>, hint: Option<&str>) {
    snap.metrics.push(Metric {
        label: label.into(),
        value: value.into(),
        hint: hint.map(str::to_string),
    });
}

fn apply_bills(snap: &mut UsageSnapshot, bills: Bills, cycle: &str) {
    let incomplete = bills.error.is_some() || !bills.warnings.is_empty();
    if let Some(currency) = bills.currency {
        snap.currency = currency;
    }
    if bills.has_cost {
        let hint = if incomplete {
            "已读取记录的部分汇总，账单或字段不完整"
        } else {
            "PretaxAmount 汇总，仅当月已出账记录，不是累计或实时费用"
        };
        metric(
            snap,
            "当月百炼已出账费用",
            format!("{:.6} {}", bills.cost, snap.currency),
            Some(hint),
        );
        snap.month = Some(PeriodUsage {
            requests: 0.0,
            tokens: bills.tokens,
            cost: bills.cost,
            from: format!("{cycle}-01"),
            to: Utc::now()
                .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
                .format("%Y-%m-%d")
                .to_string(),
        });
    } else if bills.records == 0 && bills.error.is_none() {
        metric(
            snap,
            "当月账单状态",
            "当月无百炼已出账记录",
            Some("不代表实时无使用；未出账、免费或非商业化调用可能不在账单中"),
        );
    }
    let token_hint =
        "仅对明确 Token 单位及倍率的可识别账单部分汇总；未识别项不计入，非完整实时用量";
    if bills.has_tokens {
        metric(
            snap,
            "当月计费 Token",
            format!("{:.0}", bills.tokens),
            Some(token_hint),
        );
    }
    metric(
        snap,
        "当月已计费输入 Token",
        bills
            .input
            .map(|n| format!("{n:.0}"))
            .unwrap_or_else(|| "未知（无明确输入 Token 记录）".into()),
        Some(token_hint),
    );
    metric(
        snap,
        "当月已计费输出 Token",
        bills
            .output
            .map(|n| format!("{n:.0}"))
            .unwrap_or_else(|| "未知（无明确输出 Token 记录）".into()),
        Some(token_hint),
    );
    for ((model, unit), usage) in bills.raw {
        metric(
            snap,
            &format!("当月已计费用量 · {model}"),
            format!("{usage} {unit}"),
            Some("保留账单原单位，不换算为 Token"),
        );
    }
    snap.models = bills.models.into_values().collect();
    if let Some(error) = bills.error {
        metric(
            snap,
            "账单接口错误",
            error.clone(),
            Some("余额成功不代表账单成功；已读取页保留，汇总不完整"),
        );
        snap.error = Some(error);
    }
    let mut warnings = bills.warnings.into_iter().collect::<Vec<_>>();
    warnings.sort();
    for warning in warnings {
        metric(snap, "账单字段提示", warning, None);
    }
    metric(
        snap,
        "账单读取范围",
        format!(
            "{cycle} · {} 页 · {} 条百炼计费项{}",
            bills.pages,
            bills.records,
            if incomplete { "（不完整）" } else { "" }
        ),
        None,
    );
}

const PLAN_ENDPOINT: &str = "https://modelstudio.cn-beijing.aliyuncs.com";
#[path = "bailian_cli.rs"]
mod cli;

async fn plan_call(
    http: &Http,
    ak: &str,
    sk: &str,
    action: &str,
    path: &str,
    params: BTreeMap<String, String>,
) -> Result<Value, String> {
    let query = query_string(&params);
    let mut headers = BTreeMap::from([
        ("host".into(), "modelstudio.cn-beijing.aliyuncs.com".into()),
        ("x-acs-action".into(), action.into()),
        ("x-acs-version".into(), "2026-02-10".into()),
        ("x-acs-content-sha256".into(), hex(&Sha256::digest(b""))),
        (
            "x-acs-date".into(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ),
        (
            "x-acs-signature-nonce".into(),
            format!(
                "{}-{}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                NONCE.fetch_add(1, Ordering::Relaxed)
            ),
        ),
    ]);
    let auth = sign_request(ak, sk, "GET", path, &query, &headers);
    headers.insert("authorization".into(), auth);
    let extra = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    let url = format!(
        "{PLAN_ENDPOINT}{path}{}",
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    );
    let (status, value) = http
        .request_json(Method::GET, &url, None, &extra)
        .await
        .map_err(|_| "Token Plan 网络请求失败或响应不是合法 JSON".to_string())?;
    if (200..300).contains(&status) && value["Success"].as_bool() == Some(true) {
        return value
            .get("Data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| "Token Plan 响应缺少 Data".into());
    }
    Err(format!(
        "Token Plan HTTP {status}：请检查 AK/SK、系统时间及 AliyunTokenPlanReadOnlyAccess 权限"
    ))
}

fn plan_time(value: &Value) -> Option<String> {
    value
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| t.to_rfc3339())
}

fn plan_quota(
    label: String,
    total: Option<f64>,
    remaining: Option<f64>,
    reset: Option<String>,
) -> Result<QuotaWindow, String> {
    let (total, remaining) = total
        .zip(remaining)
        .filter(|(t, r)| t.is_finite() && r.is_finite() && *t >= 0.0 && *r >= 0.0 && r <= t)
        .ok_or_else(|| "Token Plan 响应缺少有效 Credits 额度".to_string())?;
    let used = total - remaining;
    Ok(QuotaWindow {
        label,
        used,
        quota: total,
        percent: if total > 0.0 {
            used / total * 100.0
        } else {
            0.0
        },
        reset_at: reset,
        reset_in: None,
    })
}

fn apply_plan_stats(snap: &mut UsageSnapshot, data: &Value) -> Result<(), String> {
    let items = data["Items"]
        .as_array()
        .ok_or_else(|| "Token Plan 响应缺少席位统计列表".to_string())?;
    let mut quotas = Vec::new();
    for item in items {
        let seat = match item["SeatType"].as_str() {
            Some("standard") => "标准",
            Some("pro") => "高级",
            Some("max") => "尊享",
            _ => "其他",
        };
        quotas.push(plan_quota(
            format!("{seat}席位 · Credits"),
            pick_f64(item, &["SeatCredits"]),
            pick_f64(item, &["SeatRemainingCredits"]),
            plan_time(&item["SeatRefreshTime"]),
        )?);
    }
    snap.quotas.extend(quotas);
    for item in items {
        metric(
            snap,
            "席位数量（总数 / 已分配）",
            format!(
                "{} / {}",
                item["TotalSeats"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "未知".into()),
                item["AssignedSeats"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "未知".into())
            ),
            Some("额度采用官方席位类型汇总值，不乘席位数、不重复累加席位明细"),
        );
    }
    if let Some(start) = plan_time(&data["SubscriptionStartTime"]) {
        metric(snap, "订阅开始时间", start, None);
    }
    if let Some(end) = plan_time(&data["SubscriptionEndTime"]) {
        metric(snap, "订阅到期时间", end, None);
    }
    Ok(())
}

async fn shared_plan_quotas(http: &Http, ak: &str, sk: &str) -> Result<Vec<QuotaWindow>, String> {
    let mut quotas = Vec::new();
    let mut count = 0;
    let mut seen = HashSet::new();
    for page in 1..=100 {
        let params = BTreeMap::from([
            ("PageNo".into(), page.to_string()),
            ("PageSize".into(), "100".into()),
            ("StatusList.1".into(), "NORMAL".into()),
        ]);
        let data = plan_call(
            http,
            ak,
            sk,
            "ListSubscriptionSharedPackages",
            "/tokenplan/subscription/shared-packages",
            params,
        )
        .await?;
        let items = data["Items"]
            .as_array()
            .ok_or_else(|| "Token Plan 共享包响应缺少列表".to_string())?;
        let total = data["Total"]
            .as_u64()
            .ok_or_else(|| "Token Plan 共享包响应缺少分页总数".to_string())?;
        if data["PageNo"].as_u64() != Some(page) || count + items.len() as u64 > total {
            return Err("Token Plan 共享包分页信息不一致".into());
        }
        if items.is_empty() && count < total {
            return Err("Token Plan 共享包分页不完整".into());
        }
        for item in items {
            let instance = item["InstanceCode"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Token Plan 共享包缺少实例标识".to_string())?;
            if !seen.insert(instance.to_string()) {
                return Err("Token Plan 共享包分页包含重复实例，未展示不完整额度".into());
            }
            let equities = item["EquityList"]
                .as_array()
                .ok_or_else(|| "Token Plan 共享包响应缺少权益列表".to_string())?;
            for equity in equities {
                if equity["EquityType"].as_str() != Some("CREDITS") {
                    continue;
                }
                quotas.push(plan_quota(
                    format!("共享包 {} · Credits", count + 1),
                    pick_f64(equity, &["CycleTotalValue"]),
                    pick_f64(equity, &["CycleSurplusValue"]),
                    plan_time(&equity["CycleEndTime"]),
                )?);
            }
            count += 1;
        }
        if count >= total {
            return Ok(quotas);
        }
    }
    Err("Token Plan 共享包超过分页上限，未展示不完整额度".into())
}

async fn fetch_plan(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();
    metric(
        &mut snap,
        "数据范围",
        "Token Plan 团队版 · 当前订阅 Credits（非货币、非 Token 数）",
        None,
    );
    let result = async {
        let (ak, sk) = credentials(cfg)?;
        let data = plan_call(
            http,
            ak,
            sk,
            "GetSubscriptionStats",
            "/tokenplan/subscription/stats",
            BTreeMap::new(),
        )
        .await?;
        apply_plan_stats(&mut snap, &data)?;
        snap.ok = true;
        match shared_plan_quotas(http, ak, sk).await {
            Ok(quotas) => snap.quotas.extend(quotas),
            Err(e) => {
                metric(
                    &mut snap,
                    "共享包读取失败",
                    e.clone(),
                    Some("席位统计仍有效；共享包额度未知，未计入"),
                );
                snap.error = Some(e);
            }
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(e) = result {
        snap.error = Some(e);
    }
    snap
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    match cfg.plan_type.unwrap_or_default() {
        BailianPlan::TokenPlanTeam => return fetch_plan(cfg, http).await,
        BailianPlan::TokenPlanPersonal | BailianPlan::CodingPlan => {
            return cli::fetch(cfg).await;
        }
        BailianPlan::Billing => {}
    }
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();
    let cycle = billing_cycle();
    metric(
        &mut snap,
        "数据范围",
        format!("{cycle} 当月已出账账单；百炼推理、部署、训练（中国站）"),
        Some("非完整实时用量，不包含未结算后付费数据；不是历史累计"),
    );
    metric(
        &mut snap,
        "账单更新延迟",
        "约 24 小时（实例信息约 48 小时）",
        Some("当月结果仅供参考，最终账单次月 3 日 12 点后提供"),
    );
    metric(&mut snap, "请求数", "BSS 账单接口未提供", None);
    metric(
        &mut snap,
        "免费额度",
        "BSS 账单接口未提供，请前往百炼控制台查看",
        None,
    );
    let (ak, sk) = match credentials(cfg) {
        Ok(keys) => keys,
        Err(e) => {
            snap.error = Some(e);
            return snap;
        }
    };
    let balance = call(http, ak, sk, "QueryAccountBalance", BTreeMap::new()).await;
    let billed = bills(http, ak, sk, &cycle).await;
    let bills_ok = billed.pages > 0;
    let mut balance_ok = false;
    match balance {
        Ok(data) => {
            snap.balance = number(&data, "AvailableAmount");
            let cash = number(&data, "AvailableCashAmount");
            let currency = text(&data, "Currency");
            if matches!(currency, "CNY" | "USD" | "JPY")
                && (snap.balance.is_some() || cash.is_some())
            {
                snap.currency = currency.into();
                balance_ok = true;
                let hint = "阿里云账户级余额，非百炼专属；可用额度与现金余额口径不同";
                if let Some(n) = snap.balance {
                    metric(
                        &mut snap,
                        "阿里云账户可用额度",
                        format!("{n:.2} {currency}"),
                        Some(hint),
                    );
                }
                if let Some(n) = cash {
                    metric(
                        &mut snap,
                        "阿里云账户现金余额",
                        format!("{n:.2} {currency}"),
                        Some(hint),
                    );
                }
                if billed.currency.as_deref().is_some_and(|c| c != currency) {
                    snap.balance = None;
                }
            } else {
                snap.balance = None;
                let e = "QueryAccountBalance 响应缺少有效余额或币种";
                metric(&mut snap, "余额接口错误", e, None);
                snap.error = Some(e.into());
            }
        }
        Err(e) => {
            metric(&mut snap, "余额接口错误", e.clone(), None);
            snap.error = Some(e);
        }
    }
    apply_bills(&mut snap, billed, &cycle);
    snap.ok = balance_ok || bills_ok;
    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    match cfg.plan_type.unwrap_or_default() {
        BailianPlan::TokenPlanTeam => {
            let snap = fetch_plan(cfg, http).await;
            return if snap.ok {
                Ok(format!(
                    "Token Plan 团队版席位额度只读接口可访问{}",
                    snap.error
                        .map(|e| format!("；共享包未完整读取：{e}"))
                        .unwrap_or_default()
                ))
            } else {
                Err(snap
                    .error
                    .unwrap_or_else(|| "Token Plan 额度读取失败".into()))
            };
        }
        BailianPlan::TokenPlanPersonal | BailianPlan::CodingPlan => {
            return cli::probe(cfg).await
        }
        BailianPlan::Billing => {}
    }
    let (ak, sk) = credentials(cfg)?;
    let balance = call(http, ak, sk, "QueryAccountBalance", BTreeMap::new()).await;
    let bill = call(
        http,
        ak,
        sk,
        "DescribeInstanceBill",
        bill_params(&billing_cycle(), None),
    )
    .await
    .and_then(|data| items(&data).map(|_| ()));
    match (balance, bill) {
        (Ok(_), Ok(_)) => {
            Ok("AK/SK 验证成功：账户余额和账单只读接口均可访问（账单非实时用量）".into())
        }
        (Ok(_), Err(e)) => Ok(format!("部分验证成功：账户余额可访问；账单接口失败：{e}")),
        (Err(e), Ok(_)) => Ok(format!("部分验证成功：账单可访问；余额接口失败：{e}")),
        (Err(a), Err(b)) => Err(format!("账户余额失败：{a}；账单失败：{b}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn official_signature_vector() {
        let query =
            "ImageId=win2019_1809_x64_dtc_zh-cn_40G_alibase_20230811.vhd&RegionId=cn-shanghai";
        let headers = BTreeMap::from([
            ("host".into(), "ecs.cn-shanghai.aliyuncs.com".into()),
            ("x-acs-action".into(), "RunInstances".into()),
            ("x-acs-content-sha256".into(), hex(&Sha256::digest(b""))),
            ("x-acs-date".into(), "2023-10-26T10:22:32Z".into()),
            (
                "x-acs-signature-nonce".into(),
                "3156853299f313e23d1673dc12e1703d".into(),
            ),
            ("x-acs-version".into(), "2014-05-26".into()),
        ]);
        assert!(
            authorization("YourAccessKeyId", "YourAccessKeySecret", query, &headers).ends_with(
                "Signature=06563a9e1b43f5dfe96b81484da74bceab24a1d853912eee15083a6f0f3283c0"
            )
        );
    }

    #[test]
    fn units_and_empty_instance_segments() {
        assert_eq!(
            inference_instance(";workspace-demo;qwen-demo;context_0-128k_input_token;app;0"),
            Some(("qwen-demo", "context_0-128k_input_token"))
        );
        assert_eq!(
            inference_instance("workspace-demo!cn-beijing!training-demo"),
            None
        );
        assert_eq!(token_multiplier("千 Token"), Some(1000.0));
        assert_eq!(token_multiplier("百万tokens"), Some(1_000_000.0));
        for unit in ["", "GB", "秒", "字符", "千", "元/千Token", "tokens/hour"] {
            assert_eq!(token_multiplier(unit), None);
        }
    }

    #[test]
    fn billing_filters_commodity_and_preserves_unknown_units() {
        let mut b = Bills::default();
        b.add(&json!({"CommodityCode":INFERENCE,"InstanceID":";workspace-demo;qwen-demo;input_token;app;0","Usage":"2","UsageUnit":"千Token","PretaxAmount":0.5,"Currency":"CNY"}));
        b.add(&json!({"CommodityCode":INFERENCE,"InstanceID":"key-demo;workspace-demo;qwen-demo;output_token;app;0","Usage":"3","UsageUnit":"Token","PretaxAmount":0.2,"Currency":"CNY"}));
        b.add(&json!({"CommodityCode":TRAINING,"InstanceID":"workspace-demo!cn-beijing!task-demo","Usage":"4","UsageUnit":"小时","PretaxAmount":1,"Currency":"CNY"}));
        b.add(&json!({"CommodityCode":"other-product","ProductCode":INFERENCE,"Usage":"999","UsageUnit":"Token","PretaxAmount":99,"Currency":"CNY"}));
        assert_eq!(b.records, 3);
        assert_eq!(b.tokens, 2003.0);
        assert_eq!(b.input, Some(2000.0));
        assert_eq!(b.output, Some(3.0));
        assert!((b.cost - 1.7).abs() < 1e-10);
        assert_eq!(b.raw.values().copied().collect::<Vec<_>>(), vec![4.0]);
        assert_eq!(b.models.len(), 2);
        assert!(!b.models.keys().any(|k| k.contains("task-demo")));
    }

    #[test]
    fn billed_token_total_is_numeric_and_preserves_known_zero() {
        for (usage, expected) in [("2", "2003"), ("0", "0")] {
            let mut snap = UsageSnapshot::empty(&config());
            let mut b = Bills::default();
            b.add(&json!({"CommodityCode":INFERENCE,"InstanceID":";workspace-demo;qwen-demo;input_token;app;0","Usage":usage,"UsageUnit":"千Token"}));
            b.add(&json!({"CommodityCode":INFERENCE,"InstanceID":";workspace-demo;qwen-demo;output_token;app;0","Usage":if usage == "0" { "0" } else { "3" },"UsageUnit":"Token"}));
            b.add(&json!({"CommodityCode":TRAINING,"Usage":"999","UsageUnit":"小时"}));
            assert!(b.has_tokens);
            apply_bills(&mut snap, b, "2026-10");
            let totals = snap
                .metrics
                .iter()
                .filter(|m| m.label == "当月计费 Token")
                .collect::<Vec<_>>();
            assert_eq!(totals.len(), 1);
            assert_eq!(totals[0].value, expected);
            assert!(totals[0].value.chars().all(|c| c.is_ascii_digit()));
            let hint = totals[0].hint.as_deref().unwrap();
            assert!(hint.contains("可识别账单部分汇总"));
            assert!(hint.contains("非完整实时用量"));
            assert_eq!(
                snap.models.iter().map(|m| m.tokens).sum::<f64>(),
                expected.parse::<f64>().unwrap()
            );
            assert!(snap.tokens_total.is_none());
            assert!(snap.month.is_none());
        }
    }

    #[test]
    fn unknown_token_usage_omits_billed_total() {
        for row in [
            json!({"Usage":0,"UsageUnit":"小时"}),
            json!({"Usage":0}),
            json!({"UsageUnit":"Token"}),
            json!({"Usage":"unknown","UsageUnit":"Token"}),
        ] {
            let mut row = row;
            row["CommodityCode"] = json!(INFERENCE);
            row["Currency"] = json!("CNY");
            row["PretaxAmount"] = json!(1);
            let mut b = Bills::default();
            b.add(&row);
            assert!(!b.has_tokens);
            let mut snap = UsageSnapshot::empty(&config());
            apply_bills(&mut snap, b, "2026-10");
            assert!(!snap.metrics.iter().any(|m| m.label == "当月计费 Token"));
            assert_eq!(snap.month.as_ref().unwrap().cost, 1.0);
        }
    }

    #[test]
    fn unknown_cost_is_not_displayed_as_zero() {
        for row in [
            json!({"Currency":"CNY"}),
            json!({"Currency":"CNY","PretaxAmount":"unknown"}),
            json!({"Currency":"CNY","PretaxAmount":null}),
            json!({"PretaxAmount":0}),
            json!({"Currency":"unknown","PretaxAmount":1}),
        ] {
            let mut row = row;
            row["CommodityCode"] = json!(INFERENCE);
            row["Usage"] = json!(0);
            row["UsageUnit"] = json!("Token");
            let mut b = Bills::default();
            b.add(&row);
            assert!(!b.has_cost);
            let mut snap = UsageSnapshot::empty(&config());
            apply_bills(&mut snap, b, "2026-10");
            assert!(snap.month.is_none());
            assert!(!snap.metrics.iter().any(|m| m.label == "当月百炼已出账费用"));
            assert!(!snap
                .metrics
                .iter()
                .any(|m| m.value == "当月无百炼已出账记录"));
            assert!(snap
                .metrics
                .iter()
                .any(|m| m.label == "账单字段提示" && m.value.contains("0 不代表真实零费用")));
            assert!(snap
                .metrics
                .iter()
                .any(|m| m.label == "当月计费 Token" && m.value == "0"));
        }
    }

    #[test]
    fn known_cost_preserves_zero_and_partial_amounts() {
        for cost in [0.0, 1.5] {
            let mut b = Bills::default();
            b.add(&json!({"CommodityCode":INFERENCE,"Currency":"CNY","PretaxAmount":cost,"Usage":0,"UsageUnit":"Token"}));
            assert!(b.has_cost);
            let mut snap = UsageSnapshot::empty(&config());
            apply_bills(&mut snap, b, "2026-10");
            assert_eq!(snap.month.as_ref().unwrap().cost, cost);
            assert!(snap
                .metrics
                .iter()
                .any(|m| m.label == "当月百炼已出账费用" && m.value == format!("{cost:.6} CNY")));
        }
        let mut b = Bills::default();
        b.add(&json!({"CommodityCode":INFERENCE,"Currency":"CNY","PretaxAmount":1.5,"Usage":2,"UsageUnit":"Token"}));
        b.add(&json!({"CommodityCode":TRAINING,"Currency":"CNY","Usage":1,"UsageUnit":"小时"}));
        let mut snap = UsageSnapshot::empty(&config());
        apply_bills(&mut snap, b, "2026-10");
        assert_eq!(snap.month.as_ref().unwrap().cost, 1.5);
        assert!(snap
            .metrics
            .iter()
            .any(|m| m.label == "当月百炼已出账费用"
                && m.hint.as_deref().unwrap().contains("部分汇总")));
        assert!(snap
            .metrics
            .iter()
            .any(|m| m.label == "账单字段提示" && m.value.contains("PretaxAmount")));
    }

    #[test]
    fn pagination_all_pages_and_repeated_cursor() {
        let mut seen = HashSet::new();
        let mut b = Bills::default();
        for data in [
            json!({"Items":[{"CommodityCode":INFERENCE,"Currency":"CNY","PretaxAmount":1}],"NextToken":"cursor-demo"}),
            json!({"Items":{"Item":[{"CommodityCode":TRAINING,"Currency":"CNY","PretaxAmount":2}]},"NextToken":""}),
        ] {
            for row in items(&data).unwrap() {
                b.add(row);
            }
            let next = next_cursor(&data, &mut seen).unwrap();
            if let Some(next) = next {
                assert_eq!(
                    bill_params("2026-10", Some(&next))["NextToken"],
                    "cursor-demo"
                );
            }
        }
        assert_eq!(b.cost, 3.0);
        assert!(next_cursor(&json!({"NextToken":"cursor-demo"}), &mut seen).is_err());
        let params = bill_params("2026-10", None);
        assert!(!params.contains_key("ProductCode"));
        assert!(!params.contains_key("BillingDate"));
        assert_eq!(params["MaxResults"], "300");
        assert_eq!(encode("a +/中~"), "a%20%2B%2F%E4%B8%AD~");
    }

    fn config() -> ProviderConfig {
        serde_json::from_value(json!({
            "id":"provider-demo", "name":"百炼演示", "kind":"bailian",
            "currency":"CNY", "accessKeyId":"DemoAccessKeyId", "accessKeySecret":"DemoAccessKeySecret"
        })).unwrap()
    }

    #[test]
    fn plan_stats_use_group_totals_without_multiplying_seats() {
        let mut snap = UsageSnapshot::empty(&config());
        apply_plan_stats(
            &mut snap,
            &json!({
                "SubscriptionStartTime": 1790812800000_i64,
                "SubscriptionEndTime": 1793491200000_i64,
                "Items": [{"SeatType":"standard", "TotalSeats":10, "AssignedSeats":3,
                    "SeatCredits":100.5, "SeatRemainingCredits":40.25,
                    "SeatRefreshTime":1793491200000_i64}]
            }),
        )
        .unwrap();
        assert_eq!(snap.quotas.len(), 1);
        assert_eq!(snap.quotas[0].quota, 100.5);
        assert_eq!(snap.quotas[0].used, 60.25);
        assert!(snap.quotas[0].reset_at.is_some());
        assert!(
            snap.balance.is_none() && snap.tokens_total.is_none() && snap.requests_total.is_none()
        );
        assert!(snap.today.is_none() && snap.month.is_none());
    }

    #[test]
    fn plan_stats_reject_unknown_or_invalid_credits_atomically() {
        for invalid in [json!(null), json!(-1), json!(101)] {
            let mut snap = UsageSnapshot::empty(&config());
            assert!(apply_plan_stats(
                &mut snap,
                &json!({"Items":[
                    {"SeatCredits":100,"SeatRemainingCredits":50},
                    {"SeatCredits":100,"SeatRemainingCredits":invalid}
                ]})
            )
            .is_err());
            assert!(snap.quotas.is_empty());
        }
        let quota = plan_quota("demo".into(), Some(0.0), Some(0.0), None).unwrap();
        assert_eq!(quota.percent, 0.0);
        assert!(plan_quota("demo".into(), None, Some(0.0), None).is_err());
        assert!(plan_quota("demo".into(), Some(f64::INFINITY), Some(0.0), None).is_err());
    }

    #[test]
    fn get_signature_includes_method_path_and_sorted_query() {
        let headers = BTreeMap::from([
            ("host".into(), "modelstudio.cn-beijing.aliyuncs.com".into()),
            ("x-acs-content-sha256".into(), hex(&Sha256::digest(b""))),
        ]);
        let params = BTreeMap::from([
            ("PageSize".into(), "100".into()),
            ("PageNo".into(), "1".into()),
            ("StatusList.1".into(), "NORMAL".into()),
        ]);
        let query = query_string(&params);
        assert_eq!(query, "PageNo=1&PageSize=100&StatusList.1=NORMAL");
        let signature = sign_request(
            "DemoAccessKeyId",
            "DemoAccessKeySecret",
            "GET",
            "/tokenplan/subscription/stats",
            &query,
            &headers,
        );
        assert_ne!(
            signature,
            sign_request(
                "DemoAccessKeyId",
                "DemoAccessKeySecret",
                "POST",
                "/tokenplan/subscription/stats",
                &query,
                &headers
            )
        );
        assert_ne!(
            signature,
            sign_request(
                "DemoAccessKeyId",
                "DemoAccessKeySecret",
                "GET",
                "/",
                &query,
                &headers
            )
        );
    }

    #[test]
    fn legacy_config_defaults_to_bss_and_plan_roundtrips() {
        let mut cfg = config();
        assert_eq!(cfg.plan_type.unwrap_or_default(), BailianPlan::Billing);
        for (plan, name) in [
            (BailianPlan::Billing, "bss"),
            (BailianPlan::TokenPlanTeam, "tokenPlanTeam"),
            (BailianPlan::TokenPlanPersonal, "tokenPlanPersonal"),
            (BailianPlan::CodingPlan, "codingPlan"),
        ] {
            cfg.plan_type = Some(plan);
            let value = serde_json::to_value(&cfg).unwrap();
            assert_eq!(value["planType"], name);
            assert_eq!(
                serde_json::from_value::<ProviderConfig>(value)
                    .unwrap()
                    .plan_type,
                Some(plan)
            );
        }
        cfg.plan_type = Some(BailianPlan::TokenPlanTeam);
        cfg.base_url = Some(PLAN_ENDPOINT.into());
        assert!(credentials(&cfg).is_ok());
        cfg.base_url = Some(ENDPOINT.into());
        assert!(credentials(&cfg).is_err());
    }

    #[tokio::test]
    async fn cli_plans_without_explicit_binding_never_claim_success() {
        let http = Http::new().unwrap();
        for plan in [BailianPlan::TokenPlanPersonal, BailianPlan::CodingPlan] {
            let mut cfg = config();
            cfg.plan_type = Some(plan);
            let snap = fetch(&cfg, &http).await;
            assert!(!snap.ok && snap.error.as_deref().unwrap().contains("CLI Profile"));
            assert!(snap.quotas.is_empty() && snap.balance.is_none());
            assert!(snap.tokens_total.is_none() && snap.requests_total.is_none());
            assert!(probe(&cfg, &http).await.is_err());
        }
    }

    #[test]
    fn endpoint_and_credentials_are_isolated() {
        let mut cfg = config();
        assert!(credentials(&cfg).is_ok());
        for url in [
            "https://your-host",
            "https://business.aliyuncs.com.evil.example",
            "http://business.aliyuncs.com",
            "https://business.aliyuncs.com/?redirect=demo",
        ] {
            cfg.base_url = Some(url.into());
            assert!(credentials(&cfg).is_err());
        }
        cfg.base_url = None;
        cfg.access_key_id = None;
        cfg.access_key_secret = None;
        cfg.token = Some("sk-xxxx".into());
        assert!(credentials(&cfg).is_err());
    }

    #[test]
    fn partial_bill_failure_preserves_data_and_unknown_totals() {
        let mut snap = UsageSnapshot::empty(&config());
        snap.balance = Some(10.0);
        let mut b = Bills {
            pages: 1,
            error: Some("BSS HTTP 403 / NoPermission：请检查 RAM 权限".into()),
            ..Bills::default()
        };
        b.add(&json!({"CommodityCode":INFERENCE,"InstanceID":";workspace-demo;image-demo;image;app;0","Usage":"3","UsageUnit":"张","PretaxAmount":1,"Currency":"CNY"}));
        apply_bills(&mut snap, b, "2026-10");
        assert_eq!(snap.balance, Some(10.0));
        assert_eq!(snap.month.as_ref().unwrap().cost, 1.0);
        assert!(snap.error.as_deref().unwrap().contains("NoPermission"));
        assert!(snap.metrics.iter().any(|m| m.label == "账单接口错误"));
        assert!(snap.metrics.iter().any(|m| m.value == "3 张"));
        assert!(snap
            .metrics
            .iter()
            .filter(|m| m.label.contains("Token"))
            .all(|m| m.value.starts_with("未知")));
        assert!(
            snap.used.is_none() && snap.tokens_total.is_none() && snap.requests_total.is_none()
        );
        assert!(snap.days.is_empty() && snap.today.is_none());
        assert_eq!(snap.models[0].tokens, 0.0);
    }

    #[test]
    fn empty_bill_does_not_claim_realtime_zero_usage() {
        let mut snap = UsageSnapshot::empty(&config());
        apply_bills(
            &mut snap,
            Bills {
                pages: 1,
                ..Bills::default()
            },
            "2026-10",
        );
        assert!(snap.month.is_none());
        assert!(snap
            .metrics
            .iter()
            .any(|m| m.value == "当月无百炼已出账记录"));
        assert!(snap.tokens_total.is_none());
    }

    #[test]
    fn errors_never_echo_response_or_account_identifiers() {
        let e = api_result(403, json!({"Success":false,"Code":"NoPermission","Message":"private-response-demo","AccountID":"account-demo"})).unwrap_err();
        assert!(e.contains("NoPermission"));
        assert!(!e.contains("private-response-demo"));
        assert!(!e.contains("account-demo"));
        assert!(api_result(200, json!({"Success":true,"Code":"200","Data":{}})).is_ok());
    }
}
