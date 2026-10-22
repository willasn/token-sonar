//! TokenSonar 核心数据模型。
//!
//! 所有字段使用 camelCase 序列化，前端 JS 可直接消费。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 厂商类型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    /// DeepSeek 官方平台（仅需 API Key）
    #[serde(rename = "deepseek")]
    DeepSeek,
    /// New API / One API 中转站（需要站点地址 + 账号）
    #[serde(rename = "newapi")]
    NewApi,
    /// 智谱 GLM / Z.ai 开放平台（API Key 读取套餐额度，无逐日用量）
    #[serde(rename = "glm")]
    Glm,
    /// Kimi 开放平台（API Key 读取账户余额）
    #[serde(rename = "kimi")]
    Kimi,
    /// 火山引擎方舟（AK/SK 云签名，读取账户余额、Coding Plan 与模型信息）
    #[serde(rename = "volcengine")]
    Volcengine,
    #[serde(rename = "bailian")]
    Bailian,
    #[serde(rename = "hunyuan")]
    Hunyuan,
    /// 小米 MiMo 开放平台（网页登录读取余额、用量与 Token Plan 额度）
    #[serde(rename = "mimo")]
    Mimo,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::DeepSeek => "deepseek",
            ProviderKind::NewApi => "newapi",
            ProviderKind::Glm => "glm",
            ProviderKind::Kimi => "kimi",
            ProviderKind::Volcengine => "volcengine",
            ProviderKind::Bailian => "bailian",
            ProviderKind::Hunyuan => "hunyuan",
            ProviderKind::Mimo => "mimo",
        }
    }

    /// 默认站点地址
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::DeepSeek => "https://api.deepseek.com",
            ProviderKind::NewApi => "https://api.example.com",
            ProviderKind::Glm => "https://open.bigmodel.cn",
            ProviderKind::Kimi => "https://api.moonshot.cn",
            ProviderKind::Volcengine => "https://ark.cn-beijing.volcengineapi.com",
            ProviderKind::Bailian => "https://business.aliyuncs.com",
            ProviderKind::Hunyuan => "https://tokenhub.tencentcloudapi.com",
            ProviderKind::Mimo => "https://api.xiaomimimo.com",
        }
    }

    pub fn default_name(&self) -> &'static str {
        match self {
            ProviderKind::DeepSeek => "DeepSeek",
            ProviderKind::NewApi => "New API",
            ProviderKind::Glm => "GLM / Z.ai",
            ProviderKind::Kimi => "Kimi",
            ProviderKind::Volcengine => "火山方舟",
            ProviderKind::Bailian => "阿里云百炼",
            ProviderKind::Hunyuan => "腾讯混元 / TokenHub",
            ProviderKind::Mimo => "小米 MiMo",
        }
    }

}

impl Default for ProviderKind {
    fn default() -> Self {
        ProviderKind::DeepSeek
    }
}

// ---------------------------------------------------------------------------
// 账号配置
// ---------------------------------------------------------------------------

/// 认证方式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthType {
    /// 直接使用 API Key / Token（Bearer）
    #[serde(rename = "token")]
    Token,
    /// 用户名 + 密码登录，换取会话
    #[serde(rename = "password")]
    Password,
}

impl Default for AuthType {
    fn default() -> Self {
        AuthType::Token
    }
}

/// 接入方式。
///
/// 两种方式是**互斥的选择**，取数结果不做合并：
/// - `api`：手动填写 API Key / 令牌，只走厂商公开接口，能读到多少算多少；
/// - `login`：外置窗口登录，抓到网页会话后走控制台内部接口，尽量多地读页面上的信息。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccessMode {
    #[serde(rename = "api")]
    Api,
    #[serde(rename = "login")]
    Login,
}

impl Default for AccessMode {
    fn default() -> Self {
        AccessMode::Api
    }
}



/// 一条 API Key 记录。
///
/// 来源有两种：
/// - `api`：通过平台会话从 platform 接口读到的（通常只有名称与创建时间）；
/// - `manual`：接口读不到时由用户「手动录入」补登（名称 + 密钥，仅存本机）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// api / aksk / system；旧记录缺省为 api
    #[serde(default)]
    pub key_type: Option<String>,
    /// 脱敏后的 key，例如 `sk-****a1b2`
    #[serde(default)]
    pub masked: Option<String>,
    /// 手动录入时保存的完整 key（仅存本机，默认不展示）
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// `api` = 平台接口读取；`manual` = 手动录入
    #[serde(default = "default_key_source")]
    pub source: String,
}

fn default_key_source() -> String {
    "api".to_string()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BailianPlan {
    #[default]
    #[serde(rename = "bss")]
    Billing,
    TokenPlanPersonal,
    TokenPlanTeam,
    CodingPlan,
}

/// 一个被监控的厂商 / 站点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<BailianPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_profile: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 站点地址（New API 必填，DeepSeek 可留空走官方）
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_type: Option<AuthType>,
    /// 接入方式（api / login）。
    /// `None` 表示旧版配置里没有这个字段，由 `mode()` 按已有凭证推断。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_mode: Option<AccessMode>,
    /// API Key / Access Token（仅 `api` 接入方式使用）
    #[serde(default)]
    pub token: Option<String>,
    /// 接入凭证在 Keys 中的名称
    #[serde(default)]
    pub credential_name: Option<String>,
    /// 火山引擎 Access Key ID（`volcengine` 渠道的 AK/SK 签名鉴权）
    #[serde(default)]
    pub access_key_id: Option<String>,
    /// 火山引擎 Access Key Secret（`volcengine` 渠道的 AK/SK 签名鉴权）
    #[serde(default)]
    pub access_key_secret: Option<String>,
    /// `login` 接入方式抓到的网页会话凭证（平台 Token / 站点 JWT）
    #[serde(default)]
    pub session_token: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// 已废弃：历史上登录凭证存这里，加载时迁移到 `session_token`，读取时仅作兜底。
    #[serde(default)]
    pub platform_token: Option<String>,
    /// 手动录入的 API Keys（平台接口读不到完整 Key 时补登）
    #[serde(default)]
    pub api_keys: Option<Vec<ApiKeyInfo>>,
    /// UI 上的小图标（emoji 或字母）
    #[serde(default)]
    pub icon: Option<String>,
    /// UI 主题色
    #[serde(default)]
    pub accent: Option<String>,
    /// 展示货币：CNY / USD
    #[serde(default = "default_currency")]
    pub currency: String,
    /// New API 积分换算比：quota / quota_per_unit = 货币金额，默认 500000
    #[serde(default = "default_quota_per_unit")]
    pub quota_per_unit: f64,
    /// 是否把积分换算为货币显示
    #[serde(default = "default_true")]
    pub display_in_currency: bool,
    #[serde(default)]
    pub notes: Option<String>,
    /// 列表卡片中间区域展示哪一行：`metrics`（用量指标）/ `quota`（套餐额度）。
    /// 空值表示按默认（有套餐额度数据时展示额度行，否则展示用量指标行）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_view: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_true() -> bool {
    true
}
fn default_currency() -> String {
    "CNY".to_string()
}
fn default_quota_per_unit() -> f64 {
    500_000.0
}

impl ProviderConfig {
    /// 接入方式。旧配置没有 `accessMode` 时按凭证推断：有会话凭证即视为登录模式。
    pub fn mode(&self) -> AccessMode {
        match self.access_mode {
            Some(m) => m,
            None if self.session().is_some() => AccessMode::Login,
            None => AccessMode::Api,
        }
    }

    /// `login` 接入方式的会话凭证。
    pub fn session(&self) -> Option<String> {
        self.session_token
            .clone()
            .or_else(|| self.platform_token.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 旧配置迁移：历史上登录凭证存在 `platformToken` 里，归位到 `sessionToken`；
    /// 并把推断出的接入方式落定成显式值，保证前后端看到的是同一个。
    ///
    /// New API 已移除「登录授权」链路（只保留系统访问令牌），历史登录型配置
    /// 一律收敛为 `api` 并清掉会话凭证，用户重新填一次访问令牌即可。
    pub fn normalize(&mut self) {
        if self.session_token.is_none() {
            if let Some(t) = self.platform_token.clone() {
                let t = t.trim().to_string();
                if !t.is_empty() {
                    self.session_token = Some(t);
                }
            }
        }
        if matches!(self.kind, ProviderKind::NewApi) {
            self.access_mode = Some(AccessMode::Api);
            self.session_token = None;
            self.platform_token = None;
            return;
        }
        self.access_mode = Some(self.mode());
    }
}

// ---------------------------------------------------------------------------
// 用量数据
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeriodUsage {
    #[serde(default)]
    pub requests: f64,
    #[serde(default)]
    pub tokens: f64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub date: String,
    #[serde(default)]
    pub requests: f64,
    #[serde(default)]
    pub tokens: f64,
    #[serde(default)]
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    #[serde(default)]
    pub requests: f64,
    #[serde(default)]
    pub tokens: f64,
    #[serde(default)]
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLog {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tokens: f64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub duration_ms: f64,
    #[serde(default)]
    pub status: String,
}

/// 详情页里灵活的「补充指标」，适配各家私有字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub hint: Option<String>,
}

/// 套餐用量限额的一个滚动窗口（如火山方舟的近 5 小时 / 近一周 / 近一月用量）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// 窗口名称，例如「近 5 小时用量」
    pub label: String,
    /// 该窗口已用量
    #[serde(default)]
    pub used: f64,
    /// 该窗口总配额
    #[serde(default)]
    pub quota: f64,
    /// 使用率（0 ~ 100）
    #[serde(default)]
    pub percent: f64,
    /// 下次重置时间（yyyy-mm-dd HH:MM）
    #[serde(default)]
    pub reset_at: Option<String>,
    /// 距离重置的相对时间，例如「4 小时 26 分后重置」
    #[serde(default)]
    pub reset_in: Option<String>,
}

/// 一次抓取的完整快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub provider_id: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub fetched_at: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// 账户标识（用户名 / 邮箱 / 备注）
    #[serde(default)]
    pub account: Option<String>,
    /// 剩余额度（已换算为货币单位）
    #[serde(default)]
    pub balance: Option<f64>,
    /// 剩余额度原始积分
    #[serde(default)]
    pub balance_raw: Option<f64>,
    /// 累计充值 / 授予额度
    #[serde(default)]
    pub granted: Option<f64>,
    /// 累计已消耗金额
    #[serde(default)]
    pub used: Option<f64>,
    /// 累计请求数
    #[serde(default)]
    pub requests_total: Option<f64>,
    /// 累计消耗 token
    #[serde(default)]
    pub tokens_total: Option<f64>,
    #[serde(default)]
    pub today: Option<PeriodUsage>,
    /// 当前自然月汇总（requests / tokens / cost）
    #[serde(default)]
    pub month: Option<PeriodUsage>,
    #[serde(default)]
    pub days: Vec<DailyUsage>,
    #[serde(default)]
    pub models: Vec<ModelUsage>,
    /// 套餐用量限额（滚动窗口），如火山方舟的近 5 小时 / 近一周 / 近一月用量
    #[serde(default)]
    pub quotas: Vec<QuotaWindow>,
    #[serde(default)]
    pub logs: Vec<RequestLog>,
    #[serde(default)]
    pub metrics: Vec<Metric>,
    /// API Keys 列表（平台接口读取 + 本地手动录入）
    #[serde(default)]
    pub api_keys: Vec<ApiKeyInfo>,
    /// 该厂商是否支持服务端用量接口（DeepSeek 为 false，走本地台账）
    #[serde(default)]
    pub supports_usage_api: bool,
}

impl UsageSnapshot {
    pub fn empty(provider: &ProviderConfig) -> Self {
        UsageSnapshot {
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            kind: provider.kind.as_str().to_string(),
            currency: provider.currency.clone(),
            fetched_at: String::new(),
            ok: false,
            error: None,
            account: None,
            balance: None,
            balance_raw: None,
            granted: None,
            used: None,
            requests_total: None,
            tokens_total: None,
            today: None,
            month: None,
            days: vec![],
            models: vec![],
            quotas: vec![],
            logs: vec![],
            metrics: vec![],
            api_keys: vec![],
            supports_usage_api: matches!(provider.kind, ProviderKind::NewApi),
        }
    }
}

// ---------------------------------------------------------------------------
// 历史本地台账数据（保留读取与管理，刷新时不参与统计）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub id: String,
    pub provider_id: String,
    /// yyyy-mm-dd
    pub date: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub prompt_tokens: f64,
    #[serde(default)]
    pub completion_tokens: f64,
    #[serde(default)]
    pub requests: f64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// 应用设置
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// dark / light / system
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 界面语言：zh / en。
    /// 空字符串表示用户还没选择过，前端据此弹出首次启动的语言引导页。
    #[serde(default)]
    pub language: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_true")]
    pub auto_refresh: bool,
    /// 自动刷新间隔（分钟）
    #[serde(default = "default_interval")]
    pub refresh_interval_min: u32,
    #[serde(default = "default_true")]
    pub launch_at_login_hint: bool,
    #[serde(default)]
    pub last_view: String,
    /// 厂商主页面展示形态：`card`（卡片）/ `list`（列表）
    #[serde(default = "default_providers_view")]
    pub providers_view: String,
}

fn default_theme() -> String {
    "dark".to_string()
}
fn default_interval() -> u32 {
    15
}
fn default_providers_view() -> String {
    "card".to_string()
}

fn default_sync_branch() -> String {
    "main".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            theme: default_theme(),
            language: String::new(),
            currency: "CNY".to_string(),
            auto_refresh: true,
            refresh_interval_min: default_interval(),
            launch_at_login_hint: true,
            last_view: "dashboard".to_string(),
            providers_view: default_providers_view(),
        }
    }
}

// ---------------------------------------------------------------------------
// Git 仓库同步
// ---------------------------------------------------------------------------

/// Git 远端连接配置（存放在 `sync.json`）。
///
/// 凭据仅落在本机配置文件里，不会随 UI 之外的任何通道外发；
/// 用户名 + 密码（或访问令牌）在调用 git 时拼进 URL，用完即刻丢弃。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    /// 仓库地址，支持 https:// 形式，也支持 git@ / ssh://（走本机 SSH 凭据）
    #[serde(default)]
    pub repo_url: String,
    /// 用户名；ssh 地址留空
    #[serde(default)]
    pub username: String,
    /// 密码或访问令牌（GitHub / GitLab 等用 Personal Access Token 代替密码）
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_sync_branch")]
    pub branch: String,
    /// 最近一次同步成功的时间
    #[serde(default)]
    pub last_sync_at: String,
    /// 最近一次同步的结果说明（成功或失败原因）
    #[serde(default)]
    pub last_message: String,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            repo_url: String::new(),
            username: String::new(),
            password: String::new(),
            branch: default_sync_branch(),
            last_sync_at: String::new(),
            last_message: String::new(),
        }
    }
}

/// 一次同步的执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub ok: bool,
    /// 面向用户的结论文案
    pub message: String,
    /// 提交哈希（短）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// 本次是否有数据变化
    pub changed: bool,
    /// 导出并写入仓库的文件数
    pub file_count: usize,
    pub synced_at: String,
}

/// 从远端仓库读回的备份内容（合并同步的前置预览，不写本机）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBackup {
    /// 备份里的账号列表
    pub providers: Vec<ProviderConfig>,
    /// 备份里的应用设置；远端还没有 `settings.json`（首次同步）时为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<AppSettings>,
    /// 读取的分支
    pub branch: String,
    /// 读取的提交哈希（短）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}
