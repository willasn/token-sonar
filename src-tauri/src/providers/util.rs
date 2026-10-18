//! 各家响应结构差异较大，这里提供一组「字段无关」的容错解析工具：
//! 只要 JSON 里出现了语义相近的键，就能被识别出来，避免版本升级导致整体不可用。

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Utc};
use serde_json::Value;

use crate::http::{join_url, Http};
use crate::models::ProviderConfig;

/// 把任意 JSON 标量转成 f64（支持数字与数字字符串）。
pub fn to_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Null => None,
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            // 兼容 "1,234.5" 这类带千分位的返回值
            let cleaned: String = s.chars().filter(|c| *c != ',').collect();
            cleaned.parse::<f64>().ok()
        }
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// 在对象中按候选键名取值（大小写不敏感，忽略下划线与连字符差异）。
pub fn pick<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let map = obj.as_object()?;
    let normalized: Vec<(String, &Value)> = map
        .iter()
        .map(|(k, v)| (normalize_key(k), v))
        .collect();
    for key in keys {
        let target = normalize_key(key);
        if let Some((_, v)) = normalized.iter().find(|(k, _)| k == &target) {
            if !v.is_null() {
                return Some(v);
            }
        }
    }
    None
}

pub fn pick_f64(obj: &Value, keys: &[&str]) -> Option<f64> {
    pick(obj, keys).and_then(to_f64)
}

pub fn pick_string(obj: &Value, keys: &[&str]) -> Option<String> {
    match pick(obj, keys)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn normalize_key(k: &str) -> String {
    k.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 递归收集 JSON 中所有的「对象数组」，用于定位用量列表。
pub fn record_arrays<'a>(v: &'a Value, out: &mut Vec<&'a Vec<Value>>, depth: usize) {
    if depth > 6 {
        return;
    }
    match v {
        Value::Array(arr) => {
            if arr.iter().any(|x| x.is_object()) && arr.iter().all(|x| x.is_object()) {
                out.push(arr);
                return;
            }
            for child in arr {
                record_arrays(child, out, depth + 1);
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                record_arrays(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// 从任意层级里找出最像「用量记录」的对象数组：同时含有日期/时间标识与用量字段。
pub fn find_usage_records(v: &Value) -> Vec<Value> {
    let mut arrays: Vec<&Vec<Value>> = Vec::new();
    record_arrays(v, &mut arrays, 0);

    let mut best: Option<&Vec<Value>> = None;
    let mut best_score = 0usize;
    for arr in arrays {
        let score = arr
            .iter()
            .take(20)
            .filter(|r| is_usage_like(r))
            .count();
        if score > 0 && score > best_score {
            best_score = score;
            best = Some(arr);
        }
    }

    best.cloned().unwrap_or_default()
}

fn is_usage_like(r: &Value) -> bool {
    let has_time = pick(
        r,
        &[
            "date",
            "day",
            "created_at",
            "createdAt",
            "timestamp",
            "time",
            "stat_time",
            "datetime",
        ],
    )
    .is_some();
    let has_usage = pick(
        r,
        &[
            "total_tokens",
            "totalTokens",
            "tokens",
            "token_used",
            "prompt_tokens",
            "completion_tokens",
            "request_count",
            "requests",
            "count",
            "quota",
            "used_quota",
            "cost",
            "amount",
        ],
    )
    .is_some();
    has_time && has_usage
}

/// 从记录里提取日期（yyyy-mm-dd）。
pub fn record_date(r: &Value) -> Option<String> {
    let raw = pick(
        r,
        &[
            "date",
            "day",
            "date_str",
            "dateStr",
            "stat_date",
            "created_at",
            "createdAt",
            "timestamp",
            "time",
            "datetime",
        ],
    )?;

    match raw {
        Value::Number(n) => {
            let ts = n.as_i64()?;
            ts_to_date(ts)
        }
        Value::String(s) => {
            let s = s.trim();
            if let Ok(d) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                return Some(d.format("%Y-%m-%d").to_string());
            }
            if let Ok(d) = DateTime::parse_from_rfc3339(s) {
                return Some(d.with_timezone(&Local).format("%Y-%m-%d").to_string());
            }
            if s.len() >= 10 && s.chars().nth(4) == Some('-') {
                return Some(s[..10].to_string());
            }
            s.parse::<i64>().ok().and_then(ts_to_date)
        }
        _ => None,
    }
}

pub fn ts_to_date(ts: i64) -> Option<String> {
    let dt = if ts > 1_000_000_000_000 {
        DateTime::<Utc>::from_timestamp(ts / 1000, 0)?
    } else {
        DateTime::<Utc>::from_timestamp(ts, 0)?
    };
    Some(dt.with_timezone(&Local).format("%Y-%m-%d").to_string())
}

pub fn today_str() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// 返回最近 `days` 天的日期序列（含今天，升序）。
pub fn recent_dates(days: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(days);
    let now = Local::now().date_naive();
    for i in (0..days).rev() {
        let d = now - chrono::Duration::days(i as i64);
        out.push(format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()));
    }
    out
}

pub fn now_iso() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn unixtime_days_ago(days: i64) -> i64 {
    (Local::now() - chrono::Duration::days(days)).timestamp()
}

// ---------------------------------------------------------------------------
// 调用大模型（联网检索价格等场景共用）
// ---------------------------------------------------------------------------

/// 取该渠道的 API 基址（用户填写优先，缺省用厂商官方地址）。
pub fn api_base(cfg: &ProviderConfig) -> String {
    cfg.base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.kind.default_base_url())
        .trim_end_matches('/')
        .to_string()
}

/// 取该渠道的 API Key（`api` 接入方式的 `token`）。
pub fn api_token(cfg: &ProviderConfig) -> Result<String, String> {
    let key = cfg.token.as_deref().unwrap_or("").trim().to_string();
    if key.is_empty() {
        return Err("该渠道没有可用的 API Key，无法调用模型".to_string());
    }
    Ok(key)
}

/// 调一次 OpenAI 兼容的 `chat/completions`，返回首条回复的文本内容。
pub async fn openai_chat(
    http: &Http,
    url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
        "temperature": 0,
        "stream": false,
    });
    let (mut v, _) = http
        .post_json_h(url, &body, Some(api_key), None, &[])
        .await?;
    // 部分中转站会把结果再包一层 `data`
    if v.get("choices").is_none() {
        if let Some(inner) = v.get("data").cloned() {
            v = inner;
        }
    }
    let content = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| pick_string(m, &["content"]))
        .unwrap_or_default();
    if content.trim().is_empty() {
        // 中转站常见的错误体
        let msg = pick_string(&v, &["message", "msg", "error"])
            .unwrap_or_else(|| "模型未返回内容".to_string());
        return Err(format!("调用模型失败: {msg}"));
    }
    Ok(content)
}

/// 读取 OpenAI 兼容的 `GET /models`，返回模型 id 列表（失败返回空）。
pub async fn fetch_model_ids(http: &Http, base: &str, api_key: &str, path: &str) -> Vec<String> {
    let url = join_url(base, path);
    let Ok(v) = http.get_json(&url, Some(api_key), None).await else {
        return Vec::new();
    };
    let root = if v.get("data").is_none() {
        v.get("result").cloned().unwrap_or(v.clone())
    } else {
        v.clone()
    };
    let list = root
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| root.as_array());
    let mut out: Vec<String> = Vec::new();
    for item in list.into_iter().flatten() {
        let id = pick_string(item, &["id", "model", "model_name"]);
        if let Some(id) = id {
            let id = id.trim().to_string();
            if !id.is_empty() && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// 从模型输出里抽出 JSON（容忍 ```json 代码块与前后解释文字）。
pub fn extract_json(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }
    let start = trimmed.find(|c| c == '{' || c == '[');
    let end = trimmed.rfind(|c| c == '}' || c == ']');
    if let (Some(s), Some(e)) = (start, end) {
        if e > s {
            return serde_json::from_str::<Value>(&trimmed[s..=e])
                .map_err(|err| format!("模型输出的 JSON 无法解析: {err}"));
        }
    }
    Err("模型输出里没有找到 JSON".to_string())
}

