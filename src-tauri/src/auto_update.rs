//! 应用内自动更新：在 Rust 侧并行探测多个更新源。
//!
//! `tauri.conf.json` 的 `plugins.updater.endpoints` 同时配置 Cloudflare R2 镜像与
//! GitHub Release。这里不要求两个源都可用：
//!
//! 1. 并行请求每个源的 `latest.json`，整体超时 [`CHECK_TIMEOUT`]；
//! 2. 只要有任意一个源在超时内返回，就用它（多个源都返回时按版本号降序对比）；
//! 3. 全部源超时 / 不可用时才报「网络超时」；
//! 4. 安装失败（签名不匹配 / 文件损坏）时自动回退到下一个源。
//!
//! 更新检查与安装都放在 Rust 侧：前端不直接访问外部 origin，WebView 的 CSP 无需放宽。

use std::time::Duration;

use semver::Version;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Url};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::task::JoinSet;
use tokio::time::{timeout_at, Instant};

/// 下载进度事件名，前端经 `listen` 订阅。
pub const UPDATE_PROGRESS_EVENT: &str = "update:progress";

/// 检查更新源的整体超时时间（不是单个源的串行累加）。
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// 更新检查结果。`hasUpdate` 为 false 时其余字段均为 `None`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub has_update: bool,
    /// 当前运行版本
    pub current: String,
    /// 远端最新版本
    pub latest: Option<String>,
    /// 更新说明（Release notes）
    pub notes: Option<String>,
    /// 发布时间（Unix 秒），由前端按本地时区格式化
    pub published_at: Option<i64>,
}

/// 下载进度负载。`chunk` 是本次回调新增的字节数，由前端累加；`total` 未知时为 `None`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub chunk: u64,
    pub total: Option<u64>,
}

fn update_error(e: tauri_plugin_updater::Error) -> String {
    format!("更新失败：{e}")
}

/// 读取 `tauri.conf.json` 中 `plugins.updater.endpoints` 配置的全部更新源。
fn configured_endpoints(app: &AppHandle) -> Vec<Url> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("endpoints"))
        .and_then(Value::as_array)
        .map(|endpoints| {
            endpoints
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|url| Url::parse(url).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// 请求单个更新源的 `latest.json`，返回其中声明的版本号。
///
/// 超时、非 2xx、JSON 解析失败或版本号非法都视为该源不可用，返回 `None`。
async fn probe_endpoint(client: &reqwest::Client, url: &Url) -> Option<Version> {
    let response = client.get(url.clone()).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let manifest: Value = response.json().await.ok()?;
    Version::parse(manifest.get("version")?.as_str()?).ok()
}

/// 并行探测所有更新源的版本号，返回 `[(version, url)]` 列表（按版本降序）。
///
/// 整体超时 [`CHECK_TIMEOUT`]：任意一个源先返回就先收下，不等待其它源；
/// 因此只有镜像可用、或只有 GitHub 可用时都能正常查到更新。
async fn probe_endpoints_ordered(app: &AppHandle) -> Result<Vec<(Version, Url)>, String> {
    let client = reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(|e| format!("更新检查失败：{e}"))?;

    let mut tasks = JoinSet::new();
    for url in configured_endpoints(app) {
        let client = client.clone();
        tasks.spawn(async move {
            let version = probe_endpoint(&client, &url).await;
            (url, version)
        });
    }

    let deadline = Instant::now() + CHECK_TIMEOUT;
    let mut candidates: Vec<(Version, Url)> = Vec::new();
    loop {
        match timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(Ok((url, Some(version))))) => candidates.push((version, url)),
            // 该源不可用（超时 / 非 2xx / 解析失败），继续等其它源
            Ok(Some(_)) => {}
            // 所有源都已结束
            Ok(None) => break,
            // 整体超时：已有结果照常使用
            Err(_) => break,
        }
    }
    tasks.abort_all();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(candidates)
}

/// 所有更新源在超时内都连不上时的统一提示。
fn no_source_error() -> String {
    format!(
        "更新检查失败：网络超时，{} 秒内 R2 镜像与 GitHub 均无法连接",
        CHECK_TIMEOUT.as_secs()
    )
}

/// 用 `updater.check()` 获取指定 endpoint 的更新对象。
///
/// `Ok(Some)` 表示有更新，`Ok(None)` 表示该源版本不新，`Err` 表示该源不可用。
async fn check_update_from(
    app: &AppHandle,
    url: Url,
) -> Result<Option<Update>, tauri_plugin_updater::Error> {
    let updater = app
        .updater_builder()
        .endpoints(vec![url])?
        .timeout(CHECK_TIMEOUT)
        .build()?;

    match updater.check().await {
        Ok(Some(mut update)) => {
            update.timeout = None;
            Ok(Some(update))
        }
        other => other,
    }
}

/// 检查是否有新版本。只查询、不下载。
#[tauri::command]
pub async fn auto_update_check(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let candidates = probe_endpoints_ordered(&app).await?;

    if candidates.is_empty() {
        return Err(no_source_error());
    }

    let mut last_check_err: Option<tauri_plugin_updater::Error> = None;
    // 是否至少有一个源成功完成了 check（无论结果是有更新还是没更新）
    let mut any_checked_ok = false;

    for (_, url) in candidates {
        match check_update_from(&app, url).await {
            Ok(Some(update)) => {
                return Ok(UpdateInfo {
                    has_update: true,
                    current: update.current_version.clone(),
                    latest: Some(update.version.clone()),
                    notes: update.body.clone(),
                    published_at: update.date.map(|date| date.unix_timestamp()),
                });
            }
            Ok(None) => any_checked_ok = true,
            Err(err) => last_check_err = Some(err),
        }
    }

    if any_checked_ok || last_check_err.is_none() {
        return Ok(UpdateInfo {
            has_update: false,
            current,
            latest: None,
            notes: None,
            published_at: None,
        });
    }

    Err(last_check_err
        .map(update_error)
        .unwrap_or_else(no_source_error))
}

/// 下载并安装更新，成功后重启应用。下载进度经 [`UPDATE_PROGRESS_EVENT`] 回传。
///
/// 某个源的安装包签名验证失败或下载损坏时，自动回退到下一个源。
#[tauri::command]
pub async fn auto_update_install(app: AppHandle) -> Result<(), String> {
    let candidates = probe_endpoints_ordered(&app).await?;

    if candidates.is_empty() {
        return Err(no_source_error());
    }

    let mut last_install_err: Option<tauri_plugin_updater::Error> = None;
    let mut found_update = false;

    for (_, url) in candidates {
        let update = match check_update_from(&app, url.clone()).await {
            Ok(Some(update)) => update,
            Ok(None) => continue,
            Err(err) => {
                last_install_err = Some(err);
                continue;
            }
        };

        found_update = true;
        let emitter = app.clone();
        let install_result = update
            .download_and_install(
                move |chunk_length, content_length| {
                    let _ = emitter.emit(
                        UPDATE_PROGRESS_EVENT,
                        ProgressPayload {
                            chunk: chunk_length as u64,
                            total: content_length,
                        },
                    );
                },
                || {},
            )
            .await;

        match install_result {
            Ok(()) => {
                app.restart();
            }
            Err(err) => {
                last_install_err = Some(err);
                // 继续尝试下一个 endpoint
            }
        }
    }

    if !found_update {
        return Err("当前已是最新版本，无需更新".into());
    }

    Err(last_install_err
        .map(update_error)
        .unwrap_or_else(|| "更新安装失败：所有更新源均不可用".into()))
}
