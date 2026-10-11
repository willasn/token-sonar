//! 各家响应结构差异较大，这里提供一组「字段无关」的容错解析工具：
//! 只要 JSON 里出现了语义相近的键，就能被识别出来，避免版本升级导致整体不可用。

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Utc};
use serde_json::Value;

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
