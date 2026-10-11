use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::providers::util::now_iso;
use crate::models::{BailianPlan, Metric, ProviderConfig, QuotaWindow, UsageSnapshot};

const LIMIT: usize = 256 * 1024;
const MONTH_UNKNOWN: &str = "Token Plan 个人版月 Credits 未知：官方 CLI 2.1.0 尚未输出月 Credits；旧 5 小时/周比例与 harness 权益不能代替月 Credits";

fn profile(cfg: &ProviderConfig) -> Result<&str, String> {
    let name = cfg.cli_profile.as_deref().unwrap_or("").trim();
    if name.is_empty() || name.len() > 64 || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("请明确绑定 CLI Profile（字母/数字开头，最多 64 位字母、数字、-、_）；手动执行 bl auth login --console --config <profile>。查询该 Profile 的控制台账号，不验证或使用此配置的 API Key".into());
    }
    Ok(name)
}

async fn bounded(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    (&mut stream).take((LIMIT + 1) as u64).read_to_end(&mut bytes).await
        .map_err(|_| "CLI 输出读取失败".to_string())?;
    if bytes.len() > LIMIT { return Err("CLI 输出超过安全尺寸限制".into()); }
    Ok(bytes)
}

async fn execute(command: &mut Command) -> Result<Vec<u8>, String> {
    execute_with_timeout(command, Duration::from_secs(25)).await
}

async fn execute_with_timeout(command: &mut Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "未找到官方 bl CLI；请手动安装 npm install -g bailian-cli@2.1.0，并确保应用 PATH 可找到 bl".to_string()
            } else { "无法启动官方 bl CLI，请检查安装与执行权限".into() }
        })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(timeout, async {
        let (out, _, status) = tokio::try_join!(bounded(stdout), bounded(stderr), async {
            child.wait().await.map_err(|_| "CLI 进程等待失败".to_string())
        })?;
        if !status.success() {
            return Err(match status.code() {
                Some(3) => "CLI Profile 未登录或授权已过期；请手动执行 bl auth login --console --config <profile>",
                Some(2) => "CLI 命令或 Profile 不受支持；请安装官方 bailian-cli 2.1.0 并检查 Profile",
                Some(5) => "官方 CLI 查询超时",
                Some(6) => "官方 CLI 网络请求失败",
                _ => "官方 CLI 查询失败；请在终端使用相同 Profile 检查订阅和控制台权限（未回显原始错误以保护凭证）",
            }.into());
        }
        Ok(out)
    }).await;
    match result {
        Ok(Ok(out)) => Ok(out),
        other => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            match other { Ok(Err(e)) => Err(e), _ => Err("官方 CLI 查询超过 25 秒，已终止".into()) }
        }
    }
}

fn supported_version(bytes: &[u8]) -> bool {
    let text = std::str::from_utf8(bytes).unwrap_or("").trim();
    let version = text.strip_prefix("bl ").unwrap_or(text);
    let parts = version.split('.').collect::<Vec<_>>();
    parts.len() == 3 && parts[0] == "2" && parts[1].parse::<u32>().is_ok_and(|n| n >= 1)
        && parts[2].parse::<u32>().is_ok()
}

async fn query(name: &str, args: &[&str]) -> Result<Value, String> {
    let mut version = Command::new("bl");
    version.arg("--version").env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
    if !supported_version(&execute(&mut version).await?) {
        return Err("CLI 版本不支持：已验证 bailian-cli 2.1.0 JSON 契约，需要 2.x 且至少 2.1.0；未知主版本不自动尝试".into());
    }
    let mut command = Command::new("bl");
    // The console resolver reads only the explicitly selected Profile, not API-key env vars.
    command.args(args).args(["--config", name, "--console-site", "domestic", "--console-region", "cn-beijing", "--output", "json", "--quiet", "--timeout", "15"])
        .env("NO_COLOR", "1").env_remove("DASHSCOPE_API_KEY")
        .env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
    let bytes = execute(&mut command).await?;
    serde_json::from_slice(&bytes).map_err(|_| "CLI 未返回可验证的 JSON；请使用官方 bailian-cli 2.1.0".into())
}

fn number(v: &Value, key: &str) -> Option<f64> {
    v[key].as_f64().filter(|n| n.is_finite() && *n >= 0.0)
}

fn time(v: &Value, key: &str) -> Option<String> {
    v[key].as_i64().and_then(DateTime::<Utc>::from_timestamp_millis).map(|t| t.to_rfc3339())
}

fn metric(snap: &mut UsageSnapshot, label: &str, value: impl Into<String>) {
    snap.metrics.push(Metric { label: label.into(), value: value.into(), hint: None });
}

fn coding(snap: &mut UsageSnapshot, data: &Value) -> Result<(), String> {
    if !data.is_object() || data.as_object().unwrap().is_empty() {
        return Err("CLI 未返回有效 Coding Plan 订阅额度；请确认绑定 Profile 的账号已订阅".into());
    }
    for (field, label) in [("per5Hour", "近 5 小时 · 调用次数"), ("perWeek", "周窗口 · 调用次数"), ("perBillMonth", "账单月 · 调用次数")] {
        let window = &data[field];
        if let Some((used, total)) = number(window, "usedQuota").zip(number(window, "totalQuota")).filter(|(u, t)| *t > 0.0 && u <= t) {
            snap.quotas.push(QuotaWindow { label: label.into(), used, quota: total, percent: used / total * 100.0, reset_at: time(window, "resetTime"), reset_in: None });
        } else {
            metric(snap, label, "额度未知；官方未返回有效已用量及总量，未按零展示");
        }
    }
    if snap.quotas.is_empty() { return Err("Coding Plan 响应缺少有效调用次数额度".into()); }
    if snap.quotas.len() != 3 { snap.error = Some("Coding Plan 部分窗口未知；仅展示有效窗口".into()); }
    Ok(())
}

fn harness(snap: &mut UsageSnapshot, data: &Value) -> Result<(), String> {
    let items = data["items"].as_array().ok_or_else(|| "CLI harness 响应缺少权益列表".to_string())?;
    for (i, row) in items.iter().enumerate() {
        let unit = row["quotaUnit"].as_str().filter(|s| matches!(*s, "次" | "张" | "秒" | "分钟" | "小时" | "Credits" | "credits" | "tokens" | "Token" | "calls" | "images" | "seconds" | "minutes"));
        let pair = number(row, "totalQuota").zip(number(row, "availableQuota"))
            .filter(|(t, a)| *t > 0.0 && a <= t);
        if row["status"].as_str() != Some("issued") || pair.is_none() || unit.is_none() {
            metric(snap, &format!("Harness 权益 {}", i + 1), "额度未知或发放中；未按零展示");
            continue;
        }
        let (total, available) = pair.unwrap();
        let used = total - available;
        // Never copy arbitrary CLI titles, instance IDs or diagnostic text into the snapshot.
        snap.quotas.push(QuotaWindow { label: format!("Harness 权益 {} · {}", i + 1, unit.unwrap()), used, quota: total, percent: used / total * 100.0, reset_at: time(row, "instanceEndTime"), reset_in: None });
    }
    Ok(())
}

pub async fn fetch(cfg: &ProviderConfig) -> UsageSnapshot {
    let mut snap = UsageSnapshot::empty(cfg);
    snap.fetched_at = now_iso();
    let result = async {
        let name = profile(cfg)?;
        metric(&mut snap, "账号绑定", format!("CLI Profile: {name}；仅该 Profile 的控制台账号（非 API Key 所属账号验证）"));
        if cfg.plan_type == Some(BailianPlan::CodingPlan) {
            metric(&mut snap, "数据范围", "Coding Plan 中国站 · 调用次数；5 小时 / 周 / 账单月，非 Credits、非 Token 数");
            let data = query(name, &["usage", "coding-plan"]).await?;
            coding(&mut snap, &data)?;
            snap.ok = true;
        } else {
            metric(&mut snap, "个人版月 Credits", MONTH_UNKNOWN);
            // usage token-plan exposes legacy percentages only, not personal monthly Credits.
            let data = query(name, &["token-plan", "harness-quota"]).await?;
            harness(&mut snap, &data)?;
            snap.ok = !snap.quotas.is_empty();
            snap.error = Some(MONTH_UNKNOWN.into());
            metric(&mut snap, "数据范围", "仅独立 Harness 权益；到期时间非月 Credits 重置，不合计为个人套餐月额度");
        }
        Ok::<(), String>(())
    }.await;
    if let Err(e) = result { snap.error = Some(e); }
    snap
}

pub async fn probe(cfg: &ProviderConfig) -> Result<String, String> {
    let snap = fetch(cfg).await;
    if !snap.ok { return Err(snap.error.unwrap_or_else(|| "未读取到有效套餐额度".into())); }
    Ok(match snap.error {
        Some(e) => format!("指定 CLI Profile 的只读查询可访问；部分数据有效：{e}"),
        None => "指定 CLI Profile 的 Coding Plan 三周期调用次数只读查询成功；未验证 API Key、未调用模型推理".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn cfg() -> ProviderConfig { serde_json::from_value(json!({"id":"demo", "name":"demo", "kind":"bailian"})).unwrap() }

    #[test]
    fn requires_explicit_profile_and_roundtrips() {
        let mut c = cfg();
        assert!(profile(&c).is_err());
        for bad in ["../default", "-x", "a b", "a;bl", ""] { c.cli_profile = Some(bad.into()); assert!(profile(&c).is_err()); }
        c.cli_profile = Some("plan-demo".into());
        assert_eq!(profile(&c).unwrap(), "plan-demo");
        assert_eq!(serde_json::to_value(&c).unwrap()["cliProfile"], "plan-demo");
    }

    #[test]
    fn coding_uses_counts_not_cli_ratio_and_preserves_unknown() {
        let mut s = UsageSnapshot::empty(&cfg());
        coding(&mut s, &json!({"per5Hour":{"usedQuota":25,"totalQuota":100,"percentage":0.9,"resetTime":1793491200000_i64},"perWeek":{},"perBillMonth":{"usedQuota":50,"totalQuota":1000}})).unwrap();
        assert_eq!(s.quotas.len(), 2);
        assert_eq!(s.quotas[0].percent, 25.0);
        assert!(s.quotas[0].reset_at.is_some());
        assert!(s.error.is_some());
        assert!(s.balance.is_none() && s.tokens_total.is_none() && s.month.is_none());
        for data in [json!({}), json!({"per5Hour":{"usedQuota":1,"totalQuota":0}}), json!({"per5Hour":{"usedQuota":101,"totalQuota":100}})] {
            assert!(coding(&mut UsageSnapshot::empty(&cfg()), &data).is_err());
        }
    }

    #[test]
    fn harness_is_not_month_credits_or_legacy_percentage() {
        let mut s = UsageSnapshot::empty(&cfg());
        harness(&mut s, &json!({"items":[{"status":"issued","quotaUnit":"次","totalQuota":100,"availableQuota":75},{"status":"issuing"},{"status":"issued","totalQuota":100,"availableQuota":0}]})).unwrap();
        assert_eq!(s.quotas.len(), 1);
        assert_eq!(s.quotas[0].used, 25.0);
        assert!(s.balance.is_none() && s.requests_total.is_none());
        assert!(harness(&mut s, &json!({"per5HourPercentage":0.5})).is_err());
    }

    #[test]
    fn cli_version_contract_is_guarded() {
        assert!(supported_version(b"bl 2.1.0\n"));
        assert!(supported_version(b"2.2.1"));
        for v in ["bl 1.9.0", "bl 2.0.9", "bl 3.1.0", "bl 2.1.0-beta", "not-json"] { assert!(!supported_version(v.as_bytes())); }
    }

    #[tokio::test]
    async fn subprocess_size_and_timeout_protection() {
        let data = vec![b'x'; LIMIT + 1];
        assert!(bounded(&data[..]).await.is_err());
        assert_eq!(bounded(&b"{}"[..]).await.unwrap(), b"{}");
        let mut command = Command::new("definitely-missing-token-sonar-cli");
        assert!(execute(&mut command).await.unwrap_err().contains("未找到"));
        #[cfg(unix)]
        {
            let mut command = Command::new("sleep");
            command.arg("5");
            assert!(execute_with_timeout(&mut command, Duration::from_millis(10)).await.unwrap_err().contains("已终止"));
            let mut command = Command::new("printf");
            command.arg("{}");
            assert_eq!(execute(&mut command).await.unwrap(), b"{}");
        }
    }
}
