//! 华为云盘古 Pangu：费用中心账户余额（AK/SK 签名，仅余额）。
//!
//! 盘古大模型的调用统计只在 ModelArts Studio 控制台可见，没有公开的用量接口；
//! 后端的公开只读链路是华为云费用中心：
//!
//! | 数据 | 服务 | 接口 |
//! |---|---|---|
//! | 华为云账户余额（余额 / 信用 / 奖励金 / 保证金 / 欠款） | BSS | `GET /v2/accounts/customer-accounts/balances` |
//!
//! 鉴权使用华为云 API 签名指南的 `SDK-HMAC-SHA256`。请求固定发往官方
//! `bss.myhuaweicloud.com`，不把云密钥签名发送到用户填写的任意站点；
//! 站点地址字段仅作说明，不参与请求。

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::http::Http;
use crate::models::{Metric, ProviderConfig, UsageSnapshot};
use crate::providers::util::{now_iso, pick_f64, pick_string};

const HOST: &str = "bss.myhuaweicloud.com";
const PATH: &str = "/v2/accounts/customer-accounts/balances";
const SIGNED_HEADERS: &str = "content-type;host;x-sdk-date";
const CONTENT_TYPE: &str = "application/json";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], msg: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(msg.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// 规范请求：CanonicalURI 必须以 `/` 结尾，请求本身可以不带上这个斜杠。
fn canonical_request(
    host: &str,
    canonical_uri: &str,
    canonical_query: &str,
    x_sdk_date: &str,
    payload_hash: &str,
) -> String {
    let canonical_headers =
        format!("content-type:{CONTENT_TYPE}\nhost:{host}\nx-sdk-date:{x_sdk_date}\n");
    format!("GET\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{SIGNED_HEADERS}\n{payload_hash}")
}

fn signature(sk: &str, x_sdk_date: &str, canonical: &str) -> String {
    let string_to_sign = format!(
        "SDK-HMAC-SHA256\n{x_sdk_date}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    hex(&hmac_sha256(sk.as_bytes(), &string_to_sign))
}

fn credentials(cfg: &ProviderConfig) -> Result<(String, String), String> {
    let ak = cfg.access_key_id.as_deref().unwrap_or_default().trim();
    let sk = cfg.access_key_secret.as_deref().unwrap_or_default().trim();
    if ak.is_empty() || sk.is_empty() {
        return Err(
            "请填写华为云 Access Key ID（AK）与 Secret Access Key（SK），不是盘古 API Key".into(),
        );
    }
    Ok((ak.to_string(), sk.to_string()))
}

fn ensure_ok(status: u16, v: &Value) -> Result<(), String> {
    if let Some(code) = pick_string(v, &["error_code"]) {
        return Err(format!(
            "{code}（请检查 AK/SK、BSS 只读权限 billing:balance:view 与系统时间）"
        ));
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    Ok(())
}

async fn read_balances(cfg: &ProviderConfig, http: &Http) -> Result<Value, String> {
    let (ak, sk) = credentials(cfg)?;
    let x_sdk_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    // 规范 URI 需要以 `/` 结尾，实际请求地址不带结尾斜杠。
    let canonical = canonical_request(HOST, &format!("{PATH}/"), "", &x_sdk_date, &sha256_hex(b""));
    let auth = format!(
        "SDK-HMAC-SHA256 Access={ak}, SignedHeaders={SIGNED_HEADERS}, Signature={}",
        signature(&sk, &x_sdk_date, &canonical)
    );
    let url = format!("https://{HOST}{PATH}");
    let (status, v) = http
        .request_json(
            reqwest::Method::GET,
            &url,
            None,
            &[
                ("x-sdk-date", x_sdk_date.as_str()),
                ("authorization", auth.as_str()),
            ],
        )
        .await?;
    ensure_ok(status, &v)?;
    Ok(v)
}

fn account_type_label(kind: i64) -> String {
    match kind {
        1 => "账户余额".to_string(),
        2 => "信用额度".to_string(),
        5 => "奖励金".to_string(),
        7 => "保证金".to_string(),
        other => format!("账户类型 {other}"),
    }
}

fn apply_balances(snap: &mut UsageSnapshot, v: &Value) -> Result<(), String> {
    let entries = v
        .get("account_balances")
        .and_then(Value::as_array)
        .ok_or("华为云余额接口未返回 account_balances")?;

    let currency = pick_string(v, &["currency"]).unwrap_or_else(|| "CNY".to_string());
    snap.currency = currency.clone();

    let mut cash_total = 0.0;
    let mut found_cash = false;
    for entry in entries {
        let Some(kind) = pick_f64(entry, &["account_type"]).map(|x| x as i64) else {
            continue;
        };
        let Some(amount) = pick_f64(entry, &["amount"]) else {
            continue;
        };
        if let Some(id) = pick_string(entry, &["account_id"]) {
            snap.account.get_or_insert(id);
        }
        if kind == 1 {
            cash_total += amount;
            found_cash = true;
        }
        let mut hint: Vec<String> = Vec::new();
        if let Some(x) = pick_f64(entry, &["designated_amount"]) {
            if x > 0.0 {
                hint.push(format!("专款专用 {x:.2}"));
            }
        }
        if let Some(x) = pick_f64(entry, &["credit_amount"]) {
            if x > 0.0 {
                hint.push(format!("信用额度 {x:.2}"));
            }
        }
        snap.metrics.push(Metric {
            label: format!("{}（{currency}）", account_type_label(kind)),
            value: format!("{amount:.2}"),
            hint: if hint.is_empty() {
                None
            } else {
                Some(hint.join(" · "))
            },
        });
    }

    // 没有 account_type = 1 的分项时保持未知，不把缺失记作零余额。
    if found_cash {
        snap.balance = Some(cash_total);
    }
    if let Some(debt) = pick_f64(v, &["debt_amount"]) {
        if debt > 0.0 {
            snap.metrics.push(Metric {
                label: format!("欠款总额（{currency}）"),
                value: format!("{debt:.2}"),
                hint: Some("费用中心 debt_amount；欠款不计入账户余额".to_string()),
            });
        }
    }
    snap.metrics.push(Metric {
        label: "数据口径".to_string(),
        value: "华为云账户整体余额".to_string(),
        hint: Some(
            "非盘古专属余额；盘古调用统计仅在 ModelArts Studio 控制台可见，公开接口不提供逐日用量或模型统计"
                .to_string(),
        ),
    });
    Ok(())
}

pub async fn fetch(cfg: &ProviderConfig, http: &Http) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();

    match read_balances(cfg, http).await {
        Ok(v) => match apply_balances(&mut snap, &v) {
            Ok(()) => snap.ok = true,
            Err(e) => snap.error = Some(e),
        },
        Err(e) => snap.error = Some(e),
    }

    snap
}

pub async fn probe(cfg: &ProviderConfig, http: &Http) -> Result<String, String> {
    let v = read_balances(cfg, http).await?;
    let currency = pick_string(&v, &["currency"]).unwrap_or_else(|| "CNY".to_string());
    let mut snap = UsageSnapshot::empty(cfg);
    snap.currency = currency.clone();
    apply_balances(&mut snap, &v)?;
    match snap.balance {
        Some(b) => Ok(format!(
            "连接成功，华为云账户余额 {currency} {b:.2}（华为云整体余额，非盘古专属）"
        )),
        None => Ok("连接成功，华为云 AK/SK 签名校验通过".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signature_matches_official_documented_fixture() {
        // 华为云《API签名认证机制示例》中的公开示例值（非真实账号）
        let canonical = canonical_request(
            "service.region.example.com",
            "/v1/77b6a44cba5143ab91d13ab9a8ff44fd/vpcs/",
            "limit=2&marker=13551d6b-755d-4757-b956-536f674975c0",
            "20191115T033655Z",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        // 规范请求的 SHA-256 与官方文档公布值一致
        assert_eq!(
            sha256_hex(canonical.as_bytes()),
            "b25362e603ee30f4f25e7858e8a7160fd36e803bb2dfe206278659d71a9bcd7a"
        );
        // 文档未公开完整 SK，这里用固定 SK 的 HMAC 结果做回归，防止签名链路被改动
        let sig = signature(
            "MFyfvK41ba2giqM7Uio6PznpdUKGpownRZlmVmHc",
            "20191115T033655Z",
            &canonical,
        );
        assert_eq!(
            sig,
            "7be6668032f70418fcc22abc52071e57aff61b84a1d2381bb430d6870f4f6ebe"
        );
        assert_ne!(
            sig,
            signature(
                "MFyfvK41ba2giqM7Uio6PznpdUKGpownRZlmVmHc",
                "20191115T033656Z",
                &canonical
            )
        );
    }

    #[test]
    fn parses_balances_and_debt_without_inventing_values() {
        let cfg: ProviderConfig =
            serde_json::from_value(json!({"id": "demo", "name": "Demo", "kind": "pangu"})).unwrap();
        let mut snap = UsageSnapshot::empty(&cfg);
        apply_balances(
            &mut snap,
            &json!({
                "account_balances": [
                    {"account_id": "AT-demo", "account_type": 1, "amount": 120.5, "currency": "CNY", "designated_amount": 20.0, "credit_amount": 0, "measure_id": 1},
                    {"account_id": "AT-demo", "account_type": 5, "amount": 30.0, "currency": "CNY", "designated_amount": 0, "credit_amount": 0, "measure_id": 1}
                ],
                "debt_amount": 12.0,
                "measure_id": 1,
                "currency": "CNY"
            }),
        )
        .unwrap();
        assert_eq!(snap.balance, Some(120.5));
        assert_eq!(snap.account.as_deref(), Some("AT-demo"));
        assert!(snap.metrics.iter().any(|m| m.label.contains("奖励金")));
        assert!(snap.metrics.iter().any(|m| m.label.contains("欠款总额")));

        assert!(apply_balances(&mut snap, &json!({})).is_err());
    }

    #[test]
    fn api_error_code_is_reported_without_server_message() {
        let e = ensure_ok(
            403,
            &json!({"error_code": "CBC.0151", "error_msg": "sensitive-placeholder"}),
        )
        .unwrap_err();
        assert!(e.contains("CBC.0151"));
        assert!(!e.contains("sensitive-placeholder"));
        assert!(ensure_ok(500, &json!({})).is_err());
    }
}
