# TokenSonar · 功能清单（FEATURES）

> 面向产品与开发的完整功能台账。实现状态：**已实现** / **部分实现** / **规划中**。
> 版本 v0.1.0。

---

## 1. 产品定位

一个本地桌面应用（Tauri v2 + Rust + 原生 HTML/CSS/JS），把分散在多家 AI 厂商 / 中转站的
**余额、Token 用量、请求次数、使用费用** 汇总到同一块看板，并支持逐家下钻查看明细。

设计风格参考 CC Switch：左侧固定侧边栏导航 + 右侧内容区，卡片化信息密度，`暗色 / 亮色` 双主题。

---

## 2. 界面总览

| 视图 | 入口 | 说明 | 状态 |
| --- | --- | --- | --- |
| **Dashboard 总览** | 侧边栏「总览」 | 所有账号的聚合统计 + 趋势 + 分布 + 排行 | 已实现 |
| **厂商列表** | 侧边栏「厂商」 | CC Switch 风格卡片列表，点击进入详情 | 已实现 |
| **厂商详情** | 列表卡片 / 侧边栏账号名 | 单家完整用量明细 | 已实现 |
| **设置** | 侧边栏「设置」 | 主题、货币、自动刷新、数据目录 | 已实现 |
| **添加 / 编辑账号弹窗** | 顶栏「添加账号」 / 卡片编辑按钮 | 类型选择 + 凭证表单 + 连通性测试 | 已实现 |

---

## 3. Dashboard 总览（全部账号一起统计）

### 3.1 顶部指标卡（4 张）
- **总余额**：所有账号剩余额度求和；多币种时角标提示「多币种合并显示」
- **今日消耗**：所有账号当日费用求和；副标题展示近 7 天费用与环比涨跌幅
- **今日 Token**：当日 prompt + completion token 总量；副标题展示累计 token
- **今日请求**：当日请求次数；副标题展示累计请求数

### 3.2 用量趋势图
- 近 **30 天** 折线 + 渐变面积图（自研 SVG 图表，零第三方依赖）
- 指标可切换：**费用 / Token / 请求数**（分段控件）
- 鼠标悬停显示十字准线 + 日期悬浮提示
- 数据源：各账号 `days` 按日期对齐累加

### 3.3 余额分布
- 按账号列出剩余额度，横向条形图，长度按最大值归一化
- 每行使用账号自己的主题色渐变

### 3.4 今日消耗排行
- Top 8 账号：今日费用 / Token / 请求数，异常账号带红色「异常」标签
- 点击任意行跳转到该账号详情

### 3.5 模型消耗 Top
- 跨账号合并的同名模型聚合，Top 8
- 展示费用、Token 与占比条

### 3.6 空状态
- 无账号时展示引导卡片与「添加账号」按钮

---

## 4. 厂商列表（CC Switch 风格）

- 响应式卡片网格（每张卡片最小宽度 440px）
- 卡片元素：
  - 顶部一条 2px 主题色高亮条（账号自定义色）
  - 图标徽标（自定义 1-2 字符）+ 名称 + 类型标签（DeepSeek / New API / GLM / 火山方舟）
  - 状态徽标：**正常 / 异常 / 已停用 / 未刷新**
  - 站点地址或官方域名、账号名
  - 4 项核心指标：余额、今日费用、今日 Token、今日请求
  - 近 14 天费用迷你趋势图（sparkline）
  - 更新时间（相对时间，如「3 分钟前」）
  - 行内操作：刷新、启用/停用、编辑、删除
- **筛选**：全部 / DeepSeek / New API / GLM / 火山方舟 / 异常 / 已停用
- **百炼账单模式卡片**：展示阿里云账户级可用额度、当月已出账费用与可识别的账单 Token；不把月账单当作今日实时用量，缺失指标不估算。
- **搜索**：顶栏搜索框，支持按账号名、站点地址、备注、模型名过滤
- 侧边栏同步展示账号小列表（带状态点 + 余额），点击直达详情

---

## 5. 厂商详情

- **返回 / 刷新 / 编辑 / 删除** 操作区
- 错误横幅：抓取失败时展示后端返回的原始错误信息
- 4 张指标卡：剩余额度、今日消费、本月 Token、累计消费（副标题含本月 API 请求数）
- **用量趋势图**（近 30 天，可切换费用 / Token / 请求数）
- **账户明细**：来自服务端接口的补充指标（累计消费、本月/上月消费、请求数、Token、缓存命中率等）
- **模型用量表**：模型 / 请求数 / Token / 费用 / 占比条
- **Keys 表**：名称 / 类型 / Key（脱敏）/ 创建时间，支持复制与**手动录入**
- **最近请求表**（New API）：时间 / 模型 / Token / 费用 / 耗时 / 状态，最多 30 条
- **百炼账单模式详情**：展示账户余额、当月已出账费用、可解析模型及输入 / 输出 / 总 Token；不明用量单位保留原单位，并提示账单延迟、当月非最终及部分读取失败，不展示实时请求数或逐日趋势。

---

## 6. 账号管理

### 6.1 支持的厂商类型

两种**接入方式互斥**（表单顶部切换，二选一），只走所选的那一条取数链路，结果不做合并：

| 类型 | 接入方式 | 凭证 | 站点地址 | 能读到什么 |
| --- | --- | --- | --- | --- |
| **DeepSeek 官方** | `api` | API Key `sk-…` | 默认 `https://api.deepseek.com`，可改 | 官方公开接口：余额、可用模型（无用量） |
| | `login` | 平台会话 Token（登录窗口抓取） | 固定 | 平台内部接口：日/模型维度用量、消费金额、API Keys、平台余额 |
| **New API 中转** | `api` | 系统访问令牌（站点「个人设置 - 系统访问令牌」生成） | 必填（中转站根地址） | `/api/user/self` 额度与累计量 + 令牌有权访问的统计接口；该接口读不到时退回令牌级 `/api/usage/token` |
| **GLM / Z.ai** | `api` | 开放平台 API Key | 默认 `https://open.bigmodel.cn`（海外 `https://api.z.ai`），可改 | 账户汇总接口：余额、累计充值 / 累计消费；套餐额度接口：套餐等级、5 小时 / 周 / MCP 额度使用率与重置时间（不提供逐日用量） |
| **Kimi / 月之暗面** | `api` | 对应站点的官方 API Key（两站 Key 独立） | 国内 `https://api.moonshot.cn`（默认，CNY）；国际 `https://api.moonshot.ai`（USD），仅支持这两个官方余额端点 | 官方余额接口：可用余额（`available_balance`）、代金券余额（`voucher_balance`）、现金余额（`cash_balance`）；不提供逐日费用、Token、请求数或模型统计，不提供本地台账 |
| **腾讯混元 / TokenHub** | `api` | 腾讯云 SecretId + SecretKey（AK/SK，非模型 API Key） | 默认 `https://tokenhub.tencentcloudapi.com`，固定广州官方管控面 | 腾讯云整体余额（非混元专属）、今日 / 本月 TokenHub 账户与模型 Token（含其他模型，非按量账单费用）；仅可见企业版 `enterprise` credits / `enterprise-auto` tokens。个人 Coding Plan、通用 / Hy Token Plan 额度当前公开接口不可读取 |
| **阿里云百炼** | `api`，按套餐配置 | BSS / Token Plan 团队：阿里云 AK/SK；个人 / Coding：明确绑定官方 CLI 控制台 Profile | 固定中国站 BSS 或北京 ModelStudio 管控面 | BSS 账户余额及已出账费用 / Token；团队当前订阅席位与共享包 Credits（非货币 / Token）；Coding 三周期调用次数；个人独立 Harness 权益，个人月 Credits 未知 |
| **火山方舟** | `api` | Access Key ID + Secret Access Key（AK/SK） | 默认 `https://ark.cn-beijing.volcengineapi.com`，可改（只用于推导签名地域） | 管控面 OpenAPI（AK/SK 签名）：费用中心余额（可用 / 现金 / 冻结 / 欠费 / 信控额度）、Coding Plan 与 Agent Plan 套餐档位与有效期、套餐用量限额（Agent Plan 的 AFP 窗口 / Coding Plan 的 5 小时、周、月使用率）、已开通模型、Coding Plan 可用模型（不提供逐日用量） |
| **小米 MiMo** | `login` | 平台登录 Cookie（登录窗口由 Rust 读取 `platform.xiaomimimo.com` 的 Cookie） | 固定控制台 `https://platform.xiaomimimo.com`；API 基址 `https://api.xiaomimimo.com` 仅作展示 | 控制台内部接口：账户余额（充值 / 赠送 / 冻结 / 透支）、逐日逐模型费用 / Token / 请求数、月度账单、联网插件用量、个人 Token Plan 套餐与额度、API Keys。官方 API Key 只有 `/v1/models` 与推理接口，不提供余额 / 用量，故不设 `api` 接入方式 |

旧配置迁移：加载时若没有 `accessMode`，按「有会话凭证即视为登录方式」推断，并把历史的
`platformToken` 归位到 `sessionToken`（`models.rs` 的 `normalize()`，`storage.rs` 载入时执行）。
New API 已移除「登录授权」链路，历史登录型配置会被 `normalize()` 自动收敛为 `api` 并清空会话凭证。

### 6.2 表单字段
- 通用：显示名称、图标字符、类型、主题色（取色器）、备注
- 接入方式：厂商类型卡片下方的一组切换卡（API Key / 登录授权），按厂商展示各自说明；
  切换只重建凭证字段，另一种方式的凭证用隐藏字段保留，来回切换不丢
- 凭证（按所选接入方式二选一，分别存 `token` / `sessionToken`，互不覆盖）：
  - DeepSeek `api`：**API Key**，读余额与可用模型
  - DeepSeek `login`：**平台用量 Token**，「登录获取」打开外置登录窗口
  - New API `api`：**系统访问令牌**，手动粘贴（站点「个人设置 - 系统访问令牌」生成）；
    新版站点要求 `New-Api-User` 头时，可写成 `令牌#uid=<用户 ID>`
  - GLM `api`：**API Key**，智谱开放平台创建，用于读取账户余额与套餐额度（无登录授权方式）
  - Kimi `api`：**API Key**，按官方站点分别创建（国内站 CNY、国际站 USD，Key 不通用），仅支持两站官方余额端点（无登录授权及服务端用量统计）
  - 腾讯混元 / TokenHub `api`：**SecretId（AK）+ SecretKey（SK）**，分别保存为 `accessKeyId` / `accessKeySecret`；复用 `KIND_INFO.akSk` 双字段，字段标签由元数据指定。只存本机，需费用中心与 TokenHub 对应 CAM 只读权限；固定广州，无登录授权或三套餐接入模式。
  - 阿里云百炼 `api`：套餐下拉区分默认 BSS、Token Plan 团队 / 个人、Coding Plan；BSS 与团队使用 **阿里云 AccessKey ID + AccessKey Secret**，非百炼 API Key。BSS 固定中国站，只读权限最小集为 `bssapi:DescribeInstanceBill` 与 `bss:DescribeAcccount`，`Resource: "*"`（`Acccount` 为官方拼写）；团队固定北京 ModelStudio 管控面，权限为 `AliyunTokenPlanReadOnlyAccess`。个人 / Coding 可保存可选 API Key，但测试和刷新明确返回未接入，不发送推理请求。
  - 火山方舟 `api`：**Access Key ID + Secret Access Key** 两个字段，火山引擎控制台「API 访问密钥」创建；
    只保存本机，密钥需具备费用中心（billing）与方舟（ark）的只读权限（方舟管控面只认 AK/SK 签名，不支持方舟 API Key）
  - 小米 MiMo `login`：**平台登录 Cookie**，「登录获取」打开小米账号登录窗口，后端自动读取并校验；
    字段可手动粘贴 Cookie 兜底，Cookie 仅保存本机
- New API 专属：展示货币、积分兑换比、是否换算为货币
- 已移除「用户名 + 密码」表单入口：登录常有人机验证，服务端直连接口过不去，DeepSeek 统一走弹窗登录
  （账号密码直连仅作为后端能力保留，UI 未暴露）；New API 只保留系统访问令牌，不再有网页登录授权

### 6.3 外置登录窗口（DeepSeek / 小米 MiMo）

平台登录可能出现短信验证码、滑块等人机验证，服务端接口无法完成，因此改为开一个真实窗口让用户自己登录：

1. 点「登录获取」→ 打开独立窗口 `deeplogin`，加载 `https://platform.deepseek.com/usage`
2. 建窗口时注入脚本（在页面自身脚本之前执行）：
   - Hook `window.fetch` 与 `XMLHttpRequest.setRequestHeader`，抓取 `Authorization: Bearer …`
   - 每秒扫一次 `localStorage` / `sessionStorage`（已登录时无需任何操作即可取到）
   - 过滤掉 `sk-` 开头的 API Key，只保留平台用量 Token
3. 候选 Token 通过两条通道回传，谁先到用谁：
   - **IPC（主）**：`deepseek_login_token` 命令，由 `capabilities/login.json` 的
     `remote.urls = ["https://platform.deepseek.com"]` 授权，且只作用于 `deeplogin` 窗口
   - **标题（降级）**：写进 `document.title`，由 `on_document_title_changed` 回调接收，不需要任何 IPC 权限
4. 拿到候选后**必须验证**：用它请求一次 `/api/v0/usage/amount`，HTTP 200 才采纳
5. 验证通过 → 关闭并销毁窗口 → Token 回填表单 → 保存后自动刷新
6. **脚本心跳**：脚本每 5 秒回写一次 `TLPING|`，App 收到即提示「取凭证脚本已注入」——
   没有这条提示就说明注入或标题回传通道没生效（排查用）

Token 过期时错误提示会引导回到「登录获取」重新授权。

等待期间后端会持续往主窗口推 `login-progress` 事件（捕获数量 / 正在验证第几个 /
每 15 秒一次的心跳），表单下方实时显示进度，不会再停在“等待登录”上没有任何反馈；
万一全部候选都没通过校验，错误里会带上最后一条校验失败原因。

> New API 中转站不再提供登录窗口：站点域名由用户填写、无法预先写进 capability，
> 且用户级接口还要求 `New-Api-User` 头配合，链路脆弱。现在只保留系统访问令牌一种接入方式。

**小米 MiMo（Cookie 通道）**：点「登录获取」打开窗口 `mimologin`，加载
`https://platform.xiaomimimo.com/console/balance`（未登录会跳转 `account.xiaomi.com` SSO）。
不注入脚本：后端每 400ms 调 `WebviewWindow::cookies_for_url("https://platform.xiaomimimo.com/")`
读取 Cookie（含 HttpOnly），拼成 `Cookie` 头请求一次 `GET /api/v1/balance`，
业务 `code == 0` 才采纳并回填 `sessionToken`。之后所有取数都走 `/api/v1/*`：
`balance`、`usage`、`usage/detail/list`、`usage/plugin/list`、`usage/bill/monthly`、
`tokenPlan/detail`、`tokenPlan/usage`、`apiKeys`、`userProfile`。
登录失效（HTTP 401 / code 401）时统一提示重新「登录获取」。

### 6.4 Keys
- 详情页按 API Keys（大模型调用）、AKSK、系统访问令牌分类；每条凭证有名称，留空默认为「默认」（旧记录亦按默认展示）
- 添加账号时填写的 API Key / AKSK / 系统访问令牌自动在详情展示，直接引用接入凭证，编辑账号后同步更新，不重复存储
- 通过平台会话读取账号下所有 API Key；接口未公开，读取失败不阻断其它数据
- 读取不到（或平台只返回名称）时，可在详情页「手动录入」补登名称、类型与密钥
- 列表中密钥脱敏展示，可一键复制；手动录入的密钥只存在本机 `providers.json`，不会写入快照文件

### 6.5 其他能力
- **测试连接**：弹窗内一键探测，展示「连接成功，余额 xx.xx CNY」等结果
- **编辑**：回填已有配置（含密钥）
- **删除**：二次确认；同时清理该账号的快照与历史台账记录
- **启用 / 停用**：停用后不再抓取，卡片置灰
- 保存后自动触发一次该账号的刷新

---

## 7. 用量来源

用量仅来自厂商服务端接口；未提供逐日用量或接口不可用时，不使用本地台账补算。历史台账数据保留在本机，但不再参与刷新统计或在详情页展示。

---

## 8. 数据抓取

| 能力 | 说明 | 状态 |
| --- | --- | --- |
| 单账号刷新 | 列表卡片 / 详情页刷新按钮 | 已实现 |
| 全部刷新 | 顶栏「刷新全部」，并发抓取、失败互不影响 | 已实现 |
| 启动自动拉取 | 应用启动后静默刷新一次 | 已实现 |
| 定时自动刷新 | 设置项，间隔 5/10/15/30/60 分钟，默认 15 | 已实现 |
| 连通性测试 | 添加/编辑弹窗内 | 已实现 |
| 抓取失败提示 | 卡片错误文案 + 详情页错误横幅 + 侧边栏异常计数 | 已实现 |
| 请求超时 | 连接 8s / 总超时 25s | 已实现 |
| 快照持久化 | 每次抓取写入本地 JSON，冷启动先展示上次数据 | 已实现 |

### 8.1 DeepSeek 适配器

**A. 余额（官方公开接口，需 API Key）**
- `GET {base}/user/balance` → `balance_infos[]`（currency / total_balance / granted_balance / topped_up_balance）
  - 优先取与账号设置一致的币种，否则取第一条
  - 生成指标：余额、赠送余额、充值余额、账户可用状态
- `GET {base}/models` → 可用模型数量（可选，失败不影响主流程）

**B. 用量 / 消费 / API Keys（platform 内部接口，需网页会话）**

会话 Token 的两种来源：
- **登录窗口（推荐，见 6.3）**：外置 WebView 里完成登录，注入脚本捕获 Token
- 账号密码直连：`POST https://platform.deepseek.com/api/v0/users/login`（失败回退
  `chat.deepseek.com` 同名接口），body `{email|mobile, password, device_id, os}`，
  响应取 `data.biz_data.user.token`（UI 未暴露，仅作能力保留）

会话在内存中缓存 30 分钟；401 会自动重新登录一次，仍失败则提示重新授权。

数据接口（均为平台前端自用接口，**无公开文档与 SLA**，全部解析容错）：
- `GET /api/v0/usage/amount?month=&year=` → `data.biz_data.total[]` / `days[]`
  - 指标项 `usage[]`：type ∈ `REQUEST` / `PROMPT_CACHE_HIT_TOKEN` / `PROMPT_CACHE_MISS_TOKEN` / `RESPONSE_TOKEN` / `PROMPT_TOKEN`
- `GET /api/v0/usage/cost?month=&year=` → `data.biz_data[0].total[]` / `days[]`，同结构取金额
- `GET /api/v0/api_keys`（及若干候选路径）→ API Key 列表
- 请求头：`x-app-version: 1.0.0` + 浏览器 UA + `Authorization: Bearer <平台 Token>`
- 取「当月 + 上月」两个自然月，合并成日维度与模型维度；趋势图裁剪为近 30 天
- 生成指标：累计消费金额、本月/上月消费、本月 API 请求、本月 Token、今日消费、输入缓存命中率

**C. 接口不可用**：不生成逐日用量，也不使用本地台账兜底

### 8.2 New API 适配器
只走 `api` 一条链路（`newapi.rs` 的 `fetch_by_key`）：手动粘贴的系统访问令牌（Bearer）。

**额度**
- `GET /api/user/self` → `quota`（当前余额）、`request_count`（请求次数）、`used_quota`（历史消耗）、`group`、`username`
- 新版 New API 的用户级接口还要求 `New-Api-User: <用户 ID>` 头，因此 API Token 字段支持
  写 `令牌#uid=<用户 ID>`；站点只要求该头时才自动逐 ID 探测（1–100）识别，识别失败回退手动 `#uid=`
- **账户级接口读不到时自动退回令牌级额度**：`GET /api/usage/token`
  （`TokenAuthReadOnly` 鉴权，只认令牌本身、**不需要 `New-Api-User`**），
  展示该令牌的 `total_available` / `total_used` / `total_granted` / `unlimited_quota`（指标标注「令牌」）；
  两条都失败才报错。因此「不想提供用户 ID」的账号也能接入
- **鉴权报错**会把站点返回的 `user id not provided` / `invalid access token` 翻译成可操作的中文提示

**统计内容**
- 探测控制台看板数据源与使用日志接口（`/api/data/self`、`/api/log/self/stat`、`/api/log/self`），
  取到什么就展示什么；令牌没有权限时只降级不报错
- 数据看板 `/api/data/self?start_timestamp=…&end_timestamp=…`（按模型 + 日期聚合，字段 `count` / `quota` / `token_used`）
  - 返回数组 → 归一化成日 / 模型维度，并生成「统计次数 / 统计额度 / 统计Tokens」（今日）指标
  - 返回对象（部分版本直接给今日汇总）→ 直接取 `today_*` 与 `avg_rpm` / `avg_tpm`
- **用量明细**：按候选接口依次探测，取第一个返回可用记录的接口
  1. `/api/usage/token?type=1&start_timestamp=…&end_timestamp=…`
  2. `/api/usage/token?type=2&…`
  3. `/api/usage/token?type=1`
  4. `/api/usage/token`
  5. `/api/user/self/usage`
  6. `/api/log/self/stat?…`、`/api/log/self/stat`

**换算**
- `quota / quota_per_unit` → 货币金额
  - 默认 `quota_per_unit = 500000`（New API 默认 1 单位 = 500000 积分）
  - 自动探测 `GET /api/status` 中的 `quota_per_unit` / `display_in_currency`
  - 可在表单中手工覆盖；货币选「积分」时不做换算
- **字段无关归一化**：只要 JSON 中出现语义相近的键即可识别（大小写与下划线不敏感）
  - 日期：`date / day / created_at / timestamp / time / stat_time / datetime`（支持秒/毫秒时间戳、RFC3339）
  - Token：`total_tokens`，否则 `prompt_tokens + completion_tokens`
  - 请求数：`request_count / requests / count`；日志模式下每条记录记为 1 次
  - 费用：`quota / used_quota / cost / total_cost / amount / money`
  - 模型：`model_name / model / name`
- 聚合输出：日维度（补齐最近 30 天空缺为 0）、模型维度、最近请求日志

### 8.3 GLM / Z.ai 适配器

只有 `api` 一种接入方式（`glm.rs`）：填入智谱开放平台的 API Key，读取账户余额与账号套餐的额度使用情况。

- **账户汇总（余额）**：`GET {origin}/api/biz/account/query-customer-account-report`
  - 未出现在官方文档里的接口，社区发现并在多个工具中使用；国内 / 海外站点均可用
  - 鉴权同样是 `Authorization: <api_key>`（裸 Key）；返回 `{ code: 200, data: { balance, availableBalance, rechargeAmount, giveAmount, totalSpendAmount, frozenBalance, … } }`
  - 映射：`availableBalance`（缺失退回 `balance`）→ 剩余额度；`totalSpendAmount` → 累计消费（以平台返回值为准）；`rechargeAmount` → 累计充值
  - 生成指标「账户余额（币种）」+「累计充值 / 赠送 / 累计消费 / 冻结」的分解提示；失败只降级为「读取提示」，不影响连通性
- **套餐额度**：`GET {origin}/api/monitor/usage/quota/limit`（`origin` 取站点地址的 scheme + host，
  忽略用户可能填的业务路径；国内默认 `https://open.bigmodel.cn`，海外 `https://api.z.ai`）
- **鉴权**：`Authorization: <api_key>`（裸 Key，**不带** `Bearer ` 前缀）
- **返回**：`{ code: 200, data: { level, limits: [{ type, unit, usage, currentValue, percentage, nextResetTime }] } }`
  - `unit`：`3` = 5 小时额度、`6` = 周额度、`5` = 月度 MCP / 搜索额度；缺失时按顺序回退
  - 使用率优先按 `currentValue / usage` 计算，缺失时用 `percentage`；`nextResetTime` 兼容秒 / 毫秒时间戳与 ISO 字符串
  - 生成指标：套餐等级、5 小时额度、周额度、MCP 次数额度（含已用/上限与重置时间）、额度数据来源
- **容错**：业务失败也包在 HTTP 200 里，因此按 `code` 判定成功（200 / 0）；
  鉴权类错误翻译成「请确认填的是智谱开放平台的 API Key」
- **按量付费降级**：账号未订阅 Coding Plan 时，额度接口返回「当前用户不存在 coding plan」，
  不视为失败，降级为「无编程套餐（按量付费）」提示（额度接口是 Coding Plan 专属，
  按量付费账号读不到额度）；余额与累计消费仍由账户汇总接口提供，测试连接也会带上余额
- **逐日用量**：平台未开放该接口，`snap.supportsUsageApi` 保持 false；不使用本地台账补算

### 8.4 火山方舟适配器

只有 `api` 一种接入方式（`volcengine.rs`）：填入火山引擎的 **Access Key ID + Secret Access Key**，
用 Signature V4（HMAC-SHA256）签名调用**管控面** OpenAPI。
管控面接口（管理 API Key、推理接入点、套餐等）**只支持 AK/SK 签名鉴权**，方舟的 API Key 走不通。

**签名（Signature V4）**
- 请求头：`Host`、`X-Date`（ISO 8601 UTC，秒）、`X-Content-Sha256`、`Content-Type: application/json`
- `Authorization: HMAC-SHA256 Credential={AK}/{YYYYMMDD}/{region}/{service}/request,
  SignedHeaders=content-type;host;x-content-sha256;x-date, Signature={sig}`
- 派生密钥链：`kDate = HMAC(SK, date)` → `kRegion` → `kService` → `kSigning = HMAC(kService, "request")`
  （逐级使用**原始二进制摘要**作为下一级密钥，与七牛 / AWS SigV4 一致；已用官方文档示例值比对验证）
- 地域由站点地址推导：`https://ark.<region>.volcengineapi.com` → `<region>`，识别不出时用 `cn-beijing`
- 站点地址只用于推导地域与签名用途，实际请求打到固定的 `ark.cn-beijing.volcengineapi.com` 与
  `billing.volcengineapi.com`

**取数接口**
- **账户余额**：`GET https://billing.volcengineapi.com/?Action=QueryBalanceAcct&Version=2022-01-01`（service = `billing`）
  - 返回 `Result.{AvailableBalance, CashBalance, ArrearsBalance, FreezeAmount, CreditLimit, AccountID}`（金额为字符串）
  - 映射：`AvailableBalance` → 剩余额度；`AccountID` → 账号标识；其余生成分解提示
- **套餐**：`POST https://ark.cn-beijing.volcengineapi.com/?Action=GetPersonalPlan&Version=2024-01-01`（service = `ark`）
  - body `{ "Plan": "CodingPlan" }` / `{ "Plan": "AgentPlan" }`，两者各查一次
  - 返回 `Result.{PlanType, Status, StartTime, EndTime, AutoRenew}`；`Status`：`Running` = 生效中、`Expired` = 已过期
  - 未订阅时返回 404 `ResourceNotFound.Plan`，凭证仍然有效，按「未订阅」降级而不是报错
- **套餐用量限额**（详情页「套餐用量限额」卡片，近 5 小时 / 近一周 / 近一月使用率）
  - Agent Plan：`POST ...?Action=GetAFPUsage&Version=2024-01-01`，返回 `Result.{AFPFiveHour, AFPWeekly, AFPMonthly}.{Quota, Used, ResetTime}`
  - Coding Plan：Coding Plan 没有 AFP 额度（`GetAFPUsage` 会返回全 0 窗口），改调
    `POST ...?Action=GetCodingPlanUsage&Version=2024-01-01`（body `{}`），
    返回 `Result.QuotaUsage[]`，每项 `{ Level: session|weekly|monthly, Percent, ResetTimestamp, Cap }`
    （`ResetTimestamp` 为 Unix 秒）
  - 先试 AFP，拿不到数据再试 Coding Plan 接口；两者都拿不到时该卡片不显示
  - Coding Plan 接口报错或字段没认出来时，会在「账户明细」里留一行「套餐用量读取提示」（悬浮可看原始返回/错误，便于排查）
- **已开通模型**：`POST ...?Action=ListModelActivations&Version=2024-01-01`，body `{ PageNumber, PageSize }`
  - 返回 `Result.{TotalCount, Items[].{DisplayName, FoundationModelName}}`
- **Coding Plan 可用模型**：`POST ...?Action=ListArkCodingPlanModel&Version=2024-01-01`，body `{}`
  - 返回 `Result.Datas[].ModelID`

**容错与降级**
- 业务错误放在 `ResponseMetadata.Error.{Code, Message}` 里，配合 4xx / 5xx 状态码返回；
  因此新增 `Http::request_json()` 把「状态码 + JSON 体」一起返回，按错误码分支处理
- 鉴权 / 权限类错误（`AccessDenied` / `InvalidAccessKey` / `SignatureDoesNotMatch` / 401 / 403）
  翻译成「请确认 AK/SK 且密钥具备费用中心与方舟只读权限」的可操作提示
- 余额与套餐 / 模型的权限范围可能不同：任一项读到数据即视为该账号可用，失败项降级成「读取提示」
- **按量推理用量**：`POST GetInferenceUsage`（ark / 2024-01-01），北京时间覆盖本月与近 30 天；`ProjectName=default`，只统计 `BillingStatus=normal`，按动态 `Fields` 对齐 `Data`，汇总 Token、请求数、当月模型与输入/输出 Token；仅输入输出均有效时补算缺失的 TotalTokens。
- **按量消费**：`POST https://open.volcengineapi.com/?Action=ListBillDetail&Version=2022-01-01`，官方费用接口签名地域 `cn-north-1` / service `billing`；当月按天计费项、后付费消费使用账单，逐页读取（每页 300，上限 100 页），精确匹配 `ProductZh=火山方舟`，仅以 CodingPlan / AgentPlan / 套餐明确名称排除套餐，不泛泛排除 agent 模型；同一 `GroupPeriod=1` / `GroupTerm=0` 查询按 `BillDetailId` 去重，不按结算类型丢弃金额。费用使用定价币种的 `PayableAmount`，不猜产品代码或模型费用。
- **双源独立降级**：推理成功才开启 `supportsUsageApi`；账单失败保留 Token，推理失败保留费用，显式展示未知维度与独立读取提示。分页异常/上限保留已读取部分但不冒充完整账期。负数仅作现有非可空用量字段的未知标记，前端按能力表显示「—」；含未知维度的汇总保持未知，不计成真实零。
- **口径限制**：推理仅默认项目，费用为账户当月已出账按量消费而非实时/全历史，可能延迟；模型不关联账单成本。跨月近 30 天的上月费用未查询，显示未知。当前余额与账单共用币种字段，只支持 CNY 定价账单，其他币种明确降级而不混算。官方仅定义结算类型，未承诺不同类型是重复层；保留各类型应付金额，计费口径仍需真实账单核对。只填本月/今日与逐日用量，不将本月值冒充全历史累计字段；演示模式同口径，模型费用保持未知。不使用本地台账或从 Token 推算费用。

---

### 8.5 阿里云百炼适配器

- **套餐类型**：同一百炼厂商配置 `planType`：`bss`（缺省，兼容既有账单配置）、`tokenPlanTeam`、`tokenPlanPersonal`、`codingPlan`；表单、编辑、连接测试、控制台入口与 Keys 分类按配置感知元数据驱动。
- **Token Plan 团队版**：AK/SK + `AliyunTokenPlanReadOnlyAccess`，固定 `https://modelstudio.cn-beijing.aliyuncs.com`，GET `/tokenplan/subscription/stats`（`GetSubscriptionStats`）与分页 `/tokenplan/subscription/shared-packages`（`ListSubscriptionSharedPackages`），版本 `2026-02-10`、ACS3-HMAC-SHA256。展示席位类型汇总 Credits、共享包 CREDITS、官方刷新 / 周期结束时间与订阅到期时间；不乘席位数、不再累加席位明细，不将 Credits 填入货币余额或 Token 数。共享包失败保留席位统计并提示额度不完整；未知字段不补零。
- **Coding Plan 实际查询**：官方 `bailian-cli 2.1.0` 公开源码与 npm 包确认 `bl usage coding-plan --output json`：`per5Hour` / `perWeek` / `perBillMonth` 各有 `usedQuota`、`totalQuota`、`resetTime`（毫秒）；已用/总量为模型调用次数，非 Credits、非 Token、非自然月费用。Rust 本地非 shell 子进程执行，未知/无订阅/零总量不补零，部分窗口保留有效额度并提示缺失；fetch / probe 均走实际只读查询。
- **明确账号绑定**：个人 / Coding 必填 `cliProfile`，用户自行安装 `npm install -g bailian-cli@2.1.0`，再手动执行 `bl auth login --console --config <profile>`；查询明确指定 Profile 的控制台账号，既有 Provider API Key 不参与查询或账号验证。不自动选择全局激活账号、不自动登录/安装、不读取或打印凭据、不调用推理。CLI 需在应用 PATH 中；检查 2.x 且至少 2.1.0，未来主版本拒绝；请求超时 15 秒、单进程 25 秒、stdout/stderr 各 256 KiB 上限，退出/取消终止子进程，错误仅静态脱敏提示，凭据不进 argv。
- **Token Plan 个人版部分接入与确切限制**：实际执行 `bl token-plan harness-quota --output json`，解析 `items` 的 `status`、`quotaUnit`、`totalQuota`、`availableQuota`、`instanceEndTime`，只展示已发放且单位已知的独立 Harness 权益，不累加为月套餐 Credits。官方 `bl usage token-plan` 仅输出 `per5HourPercentage` / `per1WeekPercentage` 及 reset 时间，公开 CLI 2.1.0 未提供 2026-09-22 后个人版月 Credits；月 Credits 明确未知并提示控制台，不能把旧比例或 Harness 当作个人月额度。Harness 空或无有效额度不宣称查询成功；返回有效 Harness 时 probe 也明确部分成功。Mock 仅虚构演示，不执行 CLI、不验证真实账号。
- **CLI 契约证据**：[官方用量文档](https://help.aliyun.com/zh/model-studio/cli/usage-quota)、[安装鉴权](https://help.aliyun.com/zh/model-studio/cli/installation)、[Coding JSON 源码](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/coding-plan.ts)、[个人旧比例源码](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts)、[Harness 源码](https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/token-plan/harness-quota.ts)。

- **公开只读接口**：AK/SK 签名调用固定中国站 BSS `https://business.aliyuncs.com`；`QueryAccountBalance` 读取阿里云账户级可用额度与现金余额，**不是百炼专属余额**。
- **当月账单**：`DescribeInstanceBill` 分页读取当月已出账实例账单，按 `CommodityCode` 筛选百炼推理 / 部署 / 训练；费用采用 `PretaxAmount`，仅从可解析的 `InstanceID` 提取模型，不猜测未知模型。
- **用量归一化**：仅对明确为 Token 的 `UsageUnit` 归一化输入 / 输出 / 总 Token；不明单位保留原单位，不将调用次数、时长等折算为 Token，不估算未知用量。
- **时效与范围**：费用约延迟 24 小时，实例账单约延迟 48 小时，当月数据非最终账单；不提供实时请求数、逐日用量或免费额度剩余，不使用本地台账补算。
- **部分失败提示**：余额或账单读取部分失败时保留已成功读取的数据，并在详情提示失败项；权限错误提示检查 AK/SK 及最小只读权限，不把未读到的数据记作零。

---

### 8.6 腾讯混元 / TokenHub 适配器

- **公开只读接口**：腾讯云 SecretId / SecretKey 通过 TC3-HMAC-SHA256 签名访问固定广州管控面；默认 `https://tokenhub.tencentcloudapi.com`，不是模型推理 Base URL。费用中心 `DescribeAccountBalance`（2018-07-09）读取腾讯云整体余额，原始分换算 CNY，**非混元专属余额**。
- **Token 统计**：TokenHub `DescribeUsageRankList`（2026-03-22），按北京时间今日 / 本月区间读取账户 `TotalStats.TotalToken` 与模型 `TopList`。在账户明细指标中展示，包含其他模型；总 Token 已含缓存输入，不重复累加。不等于混元按量付费消耗，不查询账单费用、全账户请求数、逐日序列或历史累计量，不合并新加坡及传统混元历史数据。
- **可见企业版额度**：`DescribeTokenPlanList` 分页与 `DescribeTokenPlan` 仅读取接口可见企业版 `enterprise`（credits）/ `enterprise-auto`（tokens）的总额度与已用量；未知额度不生成零额度卡片，到期时间不当作滚动窗口重置时间。
- **明确能力限制**：个人 Coding Plan、通用 / Hy Token Plan 的订阅与额度当前公开接口不可读取，不能用企业版列表为空推断个人未购买；不提供虚假的三套餐接入方式或个人 5 小时 / 周 / 月额度。所需只读权限对应 `billing:DescribeAccountBalance` 与 `tokenhub:DescribeUsageRankList`、`DescribeTokenPlanList`、`DescribeTokenPlan`。
- **前端与演示**：类型、顺序、筛选、接入模式、中英文提示均注册到元数据表；演示模式使用独立快照分支，所有余额、Token 和企业版 quota 明确为虚构演示数据，无假逐日用量，未知费用 / 请求 / 累计量保持为空，个人额度明确不可读取。

---

## 9. 设置

| 项目 | 选项 | 状态 |
| --- | --- | --- |
| 外观主题 | 暗色 / 亮色（顶栏图标可快捷切换） | 已实现 |
| 默认展示货币 | CNY / USD / 积分 | 已实现 |
| 自动刷新开关 | 开 / 关 | 已实现 |
| 刷新间隔 | 5 / 10 / 15 / 30 / 60 分钟 | 已实现 |
| 数据目录 | 展示路径 + 一键在系统文件管理器中打开 | 已实现 |
| 关于信息 | 版本、运行环境、账号数量 | 已实现 |
| 应用更新 | 启动静默检查 + 手动「检查更新」，从 GitHub Releases 下载安装并重启 | 已实现 |
| Git 仓库同步 | 仓库地址 / 用户名 / 密码或令牌 / 分支（首次同步自动保存）+ 一键同步（先拉取合并，冲突可选） | 已实现 |

---

## 9.1 Git 仓库同步

在设置页配置一个自有 Git 仓库，只需点一个「同步」按钮即可完成合并；Git 细节（引擎、凭据、暂存副本）对用户透明。

| 项目 | 说明 |
| --- | --- |
| 配置项 | 仓库地址（https / git@ / ssh://）、用户名、密码或访问令牌、分支（默认 `main`）；点同步时自动保存草稿，无需单独「保存」 |
| 一键同步 | 点「同步」→ 选同步范围 → 拉取远端备份（只读）→ 与本机账号合并 → 推送合并结果 |
| 同步范围 | 弹窗二选一：**同步全部**（含账号密码、API Key、云 AK/SK）/ **不含账号密码与 API Key**（只同步账号条目与站点信息） |
| 凭证保护 | 「不含凭证」模式下导出前抹掉 `password` / `token` / `accessKeySecret` / `sessionToken` / `platformToken`（含手动录入 Key 的 `secret`）；本机已有凭证保留，不会被远端空白覆盖，也不因凭证差异触发冲突 |
| 合并规则 | 远端有、本机没有的账号直接补进来（新电脑可从零恢复）；同 id 内容一致则跳过；同 id 内容不同视为冲突 |
| 冲突处理 | 冲突时弹窗让用户选择：远端覆盖 / 本地覆盖 / 两者都保留（远端版以新 id +「（远端）」后缀新增）；取消则中止且不改动本机 |
| 设置项 | 应用设置始终**以远端为准**（远端还没有备份时保持本机）；本机设置不参与冲突合并 |
| 导出内容 | `tokensonar/` 目录下 3 个文件：`providers.json`（账号配置与凭证）、`settings.json`（应用设置）、`manifest.json`（版本号、导出时间、账号条数） |
| 不含内容 | 本地台账 `ledger.json` 与用量快照 `snapshots.json`：前者为不再参与统计的历史数据，后者每次刷新都会重抓，均属本机运行数据 |
| 本地暂存副本 | 应用数据目录下的 `sync-repo/`，重复同步只提交增量，无变化时不产生提交 |
| 凭据处理 | 用户名密码只拼进单次 git 命令的 URL；克隆后立刻抹掉 `.git/config` 里的 userinfo；所有错误文案脱敏后再展示 |
| 前置条件 | https 仓库需填写用户名与令牌（GitHub / GitLab 用 PAT 代替密码），走内置引擎无需安装 Git；git@ / ssh:// 需本机已安装 `git` 命令行 |
| 风险提示 | 导出文件包含账号 Token、云 AK/SK，**必须使用私有仓库**；如无必要建议改用「不含账号密码 / API Key」 |

---

## 9.2 应用内更新

基于 Tauri 官方 updater 插件，从 GitHub Releases 检查并安装新版本，无需用户手动下载安装包。

| 项目 | 说明 |
| --- | --- |
| 检查时机 | 应用启动就绪后延迟 4 秒静默检查一次（失败不打扰），设置页提供手动「检查更新」按钮 |
| 更新源 | `https://github.com/willasn/token-sonar/releases/latest/download/latest.json`（随 `tauri.conf.json` 的 `plugins.updater.endpoints` 配置） |
| 安全校验 | 更新包使用 minisign 签名，安装前用内置公钥（`plugins.updater.pubkey`）核对完整性；私钥仅存于 GitHub Secrets，不入库 |
| 交互 | 发现新版本 → 弹窗展示版本号与更新说明 → 「下载并安装」（带进度条）→ 「重启并应用」 |
| 平台差异 | Windows 安装后由安装器自动重启；macOS 需应用内重启 |
| 演示模式 | 非 Tauri 环境（浏览器预览）不执行检查，按钮置灰并提示不支持 |
| 发布流程 | 打 `v*` 标签触发 `.github/workflows/release.yml`，由 `tauri-action` 产出正式 Release 与 `latest.json`；发版前需同步提升 `Cargo.toml`、`tauri.conf.json` 与 `package.json` 的版本号 |

---

## 10. 数据与存储

- 全部数据以 JSON 存放于系统应用数据目录（原子写入，先写 `.tmp` 再 rename）
  - `providers.json`：账号配置（含 API Key，明文）
  - `snapshots.json`：最近一次抓取的用量快照（key = 账号 id）
  - `ledger.json`：本地台账
  - `settings.json`：应用设置
  - `sync.json`：Git 仓库同步配置（含凭证，明文）
  - `sync-repo/`：同步目标仓库的本机暂存副本
- 不依赖任何后端服务，除用户主动触发的 Git 同步外不上传任何数据

---

## 11. 交互细节

- 视图切换动画、卡片 hover 抬升、按钮按下反馈
- 顶栏刷新按钮加载态（旋转指示器）、侧边栏「同步中…」
- Toast 提示（成功 / 失败 / 警告），自动消失
- 弹窗支持 Esc 关闭、点击遮罩关闭
- 窗口尺寸变化时图表按容器宽度重绘（180ms 防抖）
- 搜索框 Esc 清空

---

## 12. 已知限制

1. **DeepSeek 用量接口非公开**：`/api/v0/usage/*` 是平台前端自用接口，可能随时变更或增加风控；
   失效时不再生成逐日用量，不使用本地台账补算。
2. **凭证明文存储**：尚未接入系统钥匙串（Keychain / Credential Manager）。
3. **多币种直接求和**：不同币种账号混用时总览金额为数值相加，仅作参考。
4. **New API 接口差异**：不同部署版本字段差异较大，已用容错解析兼容；若仍取不到用量，
   可在设置中调整「积分兑换比」或改用 Token 认证。
5. **Git 同步以远端分支为基线**：同步会先拉取远端备份再合并推送；https 走内置引擎（无需安装 Git），git@ / ssh:// 需本机 `git` 命令行。合并是并集语义——本机已删除但远端仍存在的账号会再被拉回来；应用设置始终以远端为准，「不含账号密码 / API Key」模式下本机凭证不会被远端覆盖。
6. 未实现：用量预警阈值、CSV 导出、按时间区间筛选、多账号分组。
7. **百炼 BSS 账单模式（另支持 Token Plan 团队额度、CLI Coding 三周期调用次数及个人 Harness 权益；个人月 Credits 未知）**：账户余额非百炼余额；费用约延迟 24 小时、实例约延迟 48 小时，当月非最终；无实时请求数、逐日用量、免费额度剩余，未知模型 / 用量不估算，不明单位保留原单位。

---

## 13. Roadmap

| 优先级 | 功能 |
| --- | --- |
| P0 | 凭证接入系统钥匙串加密存储 |
| P0 | 本地代理埋点（自动统计 DeepSeek / 官方直调的 token 与耗时） |
| P1 | 余额 / 用量阈值告警（系统通知） |
| P1 | 用量数据 CSV / JSON 导出 |
| P1 | 自定义时间区间（7 / 30 / 90 天，自定义起止） |
| P2 | 更多厂商预设：OpenAI 官方、Anthropic 官方、OpenRouter（Kimi / 月之暗面余额查询已实现） |
| P2 | 账号分组与团队视角 |
| P2 | 成本预测（按近 7 天速率预估月度支出、余额耗尽日期） |
