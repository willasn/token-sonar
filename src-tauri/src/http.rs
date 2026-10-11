//! 轻量 HTTP 客户端封装（reqwest + rustls，无需系统 OpenSSL）。

use std::time::Duration;

use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, USER_AGENT,
};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(25);
const UA: &str = "TokenSonar/0.1 (+https://github.com/tokensonar)";

pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(8))
            // 中转站常使用自签证书，这里保持严格校验，失败会在 UI 上明确提示
            .build()
            .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;
        Ok(Http { client })
    }

    fn headers(bearer: Option<&str>, cookie: Option<&str>) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(USER_AGENT, HeaderValue::from_static(UA));
        map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(token) = bearer {
            if !token.trim().is_empty() {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", token.trim())) {
                    map.insert(AUTHORIZATION, v);
                }
            }
        }
        if let Some(cookie) = cookie {
            if !cookie.trim().is_empty() {
                if let Ok(v) = HeaderValue::from_str(cookie.trim()) {
                    map.insert(COOKIE, v);
                }
            }
        }
        map
    }

    /// 在基础头之上叠加自定义头（同名的会覆盖，例如平台接口要求的浏览器 UA）。
    fn headers_with(bearer: Option<&str>, cookie: Option<&str>, extra: &[(&str, &str)]) -> HeaderMap {
        let mut map = Self::headers(bearer, cookie);
        for (k, v) in extra {
            if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(val) = HeaderValue::from_str(v) {
                    map.insert(name, val);
                }
            }
        }
        map
    }

    pub async fn get_json(
        &self,
        url: &str,
        bearer: Option<&str>,
        cookie: Option<&str>,
    ) -> Result<Value, String> {
        self.get_json_h(url, bearer, cookie, &[]).await
    }

    pub async fn get_json_h(
        &self,
        url: &str,
        bearer: Option<&str>,
        cookie: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Value, String> {
        let res = self
            .client
            .get(url)
            .headers(Self::headers_with(bearer, cookie, extra))
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", shorten(&e.to_string())))?;
        Self::parse(res).await
    }

    pub async fn post_json_h(
        &self,
        url: &str,
        body: &Value,
        bearer: Option<&str>,
        cookie: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<(Value, Option<String>), String> {
        let res = self
            .client
            .post(url)
            .headers(Self::headers_with(bearer, cookie, extra))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", shorten(&e.to_string())))?;

        let set_cookie = res
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let json = Self::parse(res).await?;
        Ok((json, set_cookie))
    }

    async fn parse(res: reqwest::Response) -> Result<Value, String> {
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;

        if !status.is_success() {
            let snippet: String = text.chars().take(160).collect();
            return Err(format!("HTTP {} - {}", status.as_u16(), snippet));
        }

        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(_) => {
                let snippet: String = text.chars().take(160).collect();
                Err(format!("响应不是合法 JSON: {snippet}"))
            }
        }
    }

    /// 云厂商 OpenAPI（如火山引擎）会把业务错误也包在 4xx / 5xx 的 JSON 体里，
    /// 因此这里连状态码一起返回，由调用方按错误码分支处理
    /// （例如「未订阅套餐」返回 404，但不是失败）。
    pub async fn request_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
        extra: &[(&str, &str)],
    ) -> Result<(u16, Value), String> {
        let mut req = self
            .client
            .request(method, url)
            .headers(Self::headers_with(None, None, extra));
        // 自行序列化并作为原始 body 发送：签名需要与之完全一致，
        // 且避免 reqwest 的 `.json()` 再插入一个 Content-Type 头
        if let Some(b) = body {
            let payload = serde_json::to_string(b).map_err(|e| format!("序列化请求体失败: {e}"))?;
            req = req.body(payload);
        }

        let res = req
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", shorten(&e.to_string())))?;
        let status = res.status().as_u16();
        let text = res
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;

        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok((status, v)),
            Err(_) => {
                let snippet: String = text.chars().take(160).collect();
                Err(format!("HTTP {status} - 响应不是合法 JSON: {snippet}"))
            }
        }
    }
}

fn shorten(s: &str) -> String {
    let s = s.replace('\n', " ");
    s.chars().take(160).collect()
}

/// 拼接站点地址与 API 路径，容忍结尾斜杠、缺协议等情况。
pub fn join_url(base: &str, path: &str) -> String {
    let mut base = base.trim().trim_end_matches('/').to_string();
    if !base.starts_with("http://") && !base.starts_with("https://") {
        base = format!("https://{base}");
    }
    let path = path.trim();
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{base}{path}")
}
