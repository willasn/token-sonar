# TokenSonar

> 汇总监控各家 AI 厂商 **Token 用量 / 请求次数 / 余额 / 使用费用** 的桌面应用。
> 技术栈：**Tauri v2（Rust 后端） + Vite + React + TypeScript 前端**。

已适配：**DeepSeek 官方**、**New API / One API 中转站**。

完整功能台账见 [`FEATURES.md`](FEATURES.md)。

---

## 目录

- [快速开始](#快速开始)
- [目录结构](#目录结构)
- [架构总览](#架构总览)
- [后端模块](#后端模块)
- [前端模块](#前端模块)
- [Tauri 命令 API](#tauri-命令-api)
- [数据存储](#数据存储)
- [新增一家厂商](#新增一家厂商)
- [预览与调试](#预览与调试)
- [常见问题](#常见问题)

---

## 快速开始

### 前置条件

- Rust ≥ 1.77（`rustup` 安装）
- Node.js ≥ 18（Tauri CLI、Vite 构建与本地预览）
- macOS 需 Xcode Command Line Tools；Windows 需 WebView2；Linux 需 `webkit2gtk-4.1`

### 安装依赖

```bash
npm install          # 安装 Tauri CLI、Vite、React 与 TypeScript
```

### 开发运行

```bash
npm run dev          # = tauri dev
```

### 生产构建

```bash
npm run build        # = tauri build，产物在 src-tauri/target/release/bundle/
```

### 仅编译检查

```bash
npm run check        # = cargo check
```

### 前端类型检查

```bash
npm run web:check    # = tsc --noEmit
```

### 浏览器预览（不启动 Tauri）

```bash
npm run web:dev      # = vite，打开 http://localhost:5173
```

非 Tauri 环境（浏览器直接打开）会自动切换到 **Mock 数据模式**（数据存于 localStorage），
用于在真实账号接入前预览与调试 UI。

---

## 目录结构

```
TokenSonar/
├── README.md                # 本文件：项目 + 架构说明
├── FEATURES.md              # 功能台账（产品 / 开发共用）
├── package.json             # Tauri CLI 脚本 + 前端依赖（Vite / React / TS）
├── vite.config.ts           # Vite 配置：root = ui，产物输出到 ui/dist
├── tsconfig.json            # TypeScript 严格模式 + @/* 路径别名
├── scripts/
│   └── make-dmg-background.py
├── ui/                      # 前端（Vite + React + TypeScript）
│   ├── index.html           # 入口页面，挂载 #root
│   ├── dist/                # 构建产物（gitignore，Tauri 打包时加载）
│   └── src/
│       ├── main.tsx         # 入口：挂载 StoreProvider / UiProvider
│       ├── App.tsx          # 外壳：侧边栏、顶栏、路由、主题、自动刷新
│       ├── types.ts         # 与后端 models.rs 对齐的类型（camelCase）
│       ├── components/
│       │   ├── ui.tsx            # Modal / Confirm / Toast / 全局悬浮提示
│       │   ├── charts.tsx        # 自研 SVG 面积图 + sparkline
│       │   ├── ProviderForm.tsx  # 添加 / 编辑账号弹窗
│       │   ├── UpdateModal.tsx   # 应用更新弹窗（版本说明 / 下载进度 / 重启）
│       │   └── Onboarding.tsx    # 首启语言引导
│       ├── state/
│       │   └── store.tsx    # 单一数据源 + 聚合计算（Context + Hook）
│       ├── lib/
│       │   ├── api.ts       # 唯一调用层：Tauri invoke + Mock 双模式
│       │   ├── mock.ts      # 浏览器演示 Mock（全假数据）
│       │   ├── kinds.ts     # 厂商类型元数据表（KIND_INFO / MODE_INFO）
│       │   ├── format.ts    # 数字 / 金额 / 时间 格式化
│       │   ├── icons.tsx    # 内联 SVG 图标集
│       │   ├── updater.ts   # 应用内更新（Tauri updater 插件封装）
│       │   └── i18n.ts      # 中英文案与语言订阅
│       ├── styles/          # 分令牌 / 基础 / 组件 / 视图四层 CSS
│       └── views/
│           ├── Dashboard.tsx # 总览
│           ├── Providers.tsx # 厂商列表
│           ├── Pricing.tsx   # 价格速查
│           ├── Detail.tsx    # 单家详情 + 本地台账
│           └── Settings.tsx  # 设置
└── src-tauri/               # Rust 后端
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json      # 窗口 / 打包 / 安全配置，frontendDist = ../ui/dist
    ├── capabilities/
    │   └── default.json     # 权限：core:default
    ├── icons/               # 应用图标（含 .icns / .ico）
    └── src/
        ├── main.rs          # 入口
        ├── lib.rs           # Builder + setup + invoke_handler
        ├── models.rs        # 所有数据模型（serde camelCase）
        ├── storage.rs       # JSON 持久化（原子写入）
        ├── http.rs          # reqwest 封装 + URL 拼接
        ├── commands.rs      # 暴露给前端的 Tauri 命令
        ├── login.rs         # 外置登录窗口：站点授权后取回凭证
        ├── sync.rs          # Git 仓库同步：导出 JSON → 提交 → 推送
        └── providers/
            ├── mod.rs        # 分发：fetch_snapshot / probe
            ├── deepseek.rs   # DeepSeek 适配器
            ├── newapi.rs     # New API / One API 适配器
            ├── glm.rs        # 智谱 GLM / Z.ai 适配器
            ├── volcengine.rs # 火山方舟（AK/SK 签名）适配器
            ├── bailian.rs    # 阿里云百炼（BSS 账单、团队 Credits）适配器
            ├── bailian_cli.rs # 百炼个人 Harness / Coding 官方 CLI 只读查询
            ├── mimo.rs       # 小米 MiMo（登录 Cookie → 控制台接口）适配器
            └── util.rs       # 字段无关的容错 JSON 解析工具
```

---

## 架构总览

```
┌──────────────────────────────────────────────────────────────┐
│                   ui/src（Vite + React + TS）                  │
│  App.tsx(路由) ─┬─ views/Dashboard.tsx ─┐                     │
│                 ├─ views/Providers.tsx ─┼─ state/store.tsx    │
│                 ├─ views/Detail.tsx    ─┤   components/charts │
│                 └─ views/Settings.tsx  ─┘   components/ui.tsx │
│                          │                                    │
│                    lib/api.ts（唯一接缝）                       │
│                    Tauri invoke / Mock 双模式                  │
└───────────────────────────┬──────────────────────────────────┘
                            │ IPC（Tauri v2 invoke）
┌───────────────────────────▼──────────────────────────────────┐
│                     src-tauri/（Rust）                         │
│  commands.rs  ← 命令层：参数校验、状态读取、结果回写             │
│       │                                                       │
│  providers/mod.rs  ← 分发层：按 kind 选择适配器                 │
│       ├── deepseek.rs   余额接口 + 本地台账聚合                 │
│       └── newapi.rs     登录/额度/用量接口 + 字段无关归一化      │
│       │                                                       │
│  http.rs（reqwest + rustls）    storage.rs（JSON 原子写入）      │
│  models.rs（共享数据模型）                                      │
└──────────────────────────────────────────────────────────────┘
```

**核心设计取舍**

1. **前端有构建步骤**：Vite 以 `ui/` 为 root，产物输出到 `ui/dist`，`tauri.conf.json` 的
   `frontendDist` 指向它；开发时 `devUrl` 指向 Vite dev server。类型与视图统一走
   `src/lib/api.ts` 这一唯一接缝，不再依赖全局命名空间。
2. **字段无关的解析**：中转站版本差异大，`providers/util.rs` 提供大小写/下划线不敏感的
   键查找与递归记录定位，接口升级不会整体失效。
3. **快照持久化**：抓取结果落盘，冷启动先展示上次数据再静默刷新，避免白屏等待。
4. **Mock 模式**：`lib/api.ts` 在非 Tauri 环境自动降级到 `lib/mock.ts`，UI 可脱离 Rust 独立开发。

---

## 后端模块

### `models.rs`
`ProviderConfig`（账号配置）、`UsageSnapshot`（一次抓取的快照）、`DailyUsage` / `ModelUsage`
/ `RequestLog` / `Metric`（明细维度）、`LedgerEntry`（本地台账）、`AppSettings`。
全部 `#[serde(rename_all = "camelCase")]`，与前端 JS 字段一一对应。

### `storage.rs`
`AppStore` 持有应用数据目录，提供 providers / snapshots / ledger / settings 的读写。
写入采用「先写 `*.tmp` 再 `rename`」保证原子性；删除账号时级联清理快照与台账。

### `http.rs`
`reqwest` 封装（rustls，无需系统 OpenSSL）：`get_json` / `post_json`，统一 25s 超时、
统一 UA，登录接口额外返回 `Set-Cookie`。`join_url` 容忍结尾斜杠与缺失协议。

### `providers/`
- `mod.rs`：`fetch_snapshot()` 按 `kind` 分发并统一补齐 30 天日维度；`probe()` 做最小代价连通性测试。
- `deepseek.rs`：余额 + 模型数来自官方接口，用量来自 `ledger` 参数。
- `newapi.rs`：登录 → 探测换算比 → 读额度 → 依次探测 7 个候选用量接口 → 归一化。
- `glm.rs`：账户汇总接口读余额 / 累计消费，额度接口读套餐等级与 5 小时 / 周 / MCP 使用率。
- `volcengine.rs`：Signature V4（HMAC-SHA256）签名调用火山引擎管控面 OpenAPI，
  读账户余额（billing）、Coding Plan / Agent Plan 套餐与模型信息（ark）。
- `mimo.rs`：登录 Cookie 会话调用小米 MiMo 控制台内部接口，读余额、逐日逐模型用量、
  月度账单与 Token Plan 额度；控制台接口无公开文档，解析全部容错。
- `util.rs`：`to_f64` / `pick` / `pick_f64` / `pick_string` / `record_arrays` /
  `find_usage_records` / `record_date` / `recent_dates` 等容错工具。

### `commands.rs`
见下方 [Tauri 命令 API](#tauri-命令-api)。并发刷新使用 `tauri::async_runtime::spawn`，
单个账号失败不影响其他账号。

---

## 前端模块

| 文件 | 职责 |
| --- | --- |
| `lib/api.ts` | 唯一接缝：Tauri 内走 IPC，浏览器内走 `lib/mock.ts`（localStorage） |
| `state/store.tsx` | Context + Hook 单一数据源、异步动作、聚合计算 |
| `components/charts.tsx` | 面积折线图（悬停十字线 + 提示）、迷你 sparkline |
| `components/ui.tsx` | `useUi()` 提供 Modal / Confirm / Toast 与全局悬浮提示 |
| `App.tsx` | 路由、顶栏/侧边栏、主题、自动刷新定时器 |
| `views/*.tsx` | 每个视图导出组件（Dashboard / Providers / Pricing / Detail / Settings） |

**渲染时序**：`App.tsx` 按当前视图条件渲染对应组件，图表容器宽度由 `ResizeObserver` 实时跟踪。

---

## Tauri 命令 API

所有命令在 `src-tauri/src/lib.rs` 的 `invoke_handler` 中注册，前端通过 `ui/src/lib/api.ts`
的 `api.*` 封装调用（内部即 `invoke("<命令名>", { 参数 })`）。

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `list_providers` | — | `ProviderConfig[]` | 全部账号配置 |
| `save_provider` | `{ provider }` | `ProviderConfig` | 新增或更新（id 为空则生成） |
| `remove_provider` | `{ id }` | `null` | 删除账号 + 级联清理 |
| `test_provider` | `{ provider }` | `string` | 连通性测试，返回可读结果 |
| `refresh_provider` | `{ id }` | `UsageSnapshot` | 抓取单家并落盘 |
| `refresh_all` | — | `UsageSnapshot[]` | 并发抓取全部启用账号 |
| `get_snapshots` | — | `UsageSnapshot[]` | 读取上次快照（按账号顺序） |
| `list_ledger` | `{ providerId? }` | `LedgerEntry[]` | 本地台账 |
| `add_ledger_entry` | `{ entry }` | `LedgerEntry` | 记一笔 |
| `remove_ledger_entry` | `{ id }` | `null` | 删除台账条目 |
| `get_settings` | — | `AppSettings` | 应用设置 |
| `save_settings` | `{ settings }` | `null` | 保存设置 |
| `get_app_info` | — | `{ name, version, dataDir }` | 关于信息 |
| `open_data_dir` | — | `null` | 在系统文件管理器中打开数据目录 |
| `open_external_url` | `{ url }` | `null` | 用系统默认浏览器打开外部链接（厂商控制台） |

---

## 数据存储

目录位置由 Tauri 的 `path().app_data_dir()` 决定（设置页可查看并一键打开）：

| 文件 | 内容 |
| --- | --- |
| `providers.json` | 账号配置（含 API Key / 密码，当前为明文） |
| `snapshots.json` | `providerId -> UsageSnapshot` |
| `ledger.json` | 本地台账条目 |
| `settings.json` | 主题、货币、自动刷新等 |

> 凭证加密存储（Keychain / Credential Manager）在 Roadmap 中，见 `FEATURES.md`。

---

## 新增一家厂商

1. `models.rs`：`ProviderKind` 增加枚举变体，实现 `as_str / default_base_url / default_name`。
2. `src/providers/<kind>.rs`：实现 `pub async fn fetch(cfg, http, ...) -> UsageSnapshot`，
   尽量复用 `util.rs` 的容错解析。
3. `providers/mod.rs`：`fetch_snapshot()` 与 `probe()` 的 `match` 增加分支。
4. 前端 `ui/src/lib/kinds.ts`：`KIND_INFO` 增加图标、名称、描述、主题色与字段配置，
   `MODES_BY_KIND` / `MODE_INFO` 增加该类型的接入方式。
5. 筛选 pills（`FILTERS`）与设置项（如需要）补充类型项。
6. 在 `FEATURES.md` 中登记该厂商的接口与字段。

---

## 预览与调试

```bash
# 1) 只看 UI（Mock 数据）
npm run web:dev

# 2) 完整应用（真实接口）
npm run dev
```

- 前端调试：`tauri dev` 下可直接用系统 WebView 的开发者工具（Rust 侧已开启 `devtools` feature）。
- 后端日志：`cargo` 编译警告 / 错误直接输出到终端；接口错误信息会回传到卡片与详情页。
- 恢复演示数据：设置页「重置演示数据」（仅浏览器预览模式可见）。

---

## 常见问题

**Q：DeepSeek 为什么没有 token 用量？**
A：DeepSeek 官方目前只开放 `/user/balance` 余额接口，没有公开的用量 / 请求数接口。
余额来自官方，用量请在详情页通过「本地台账 → 记一笔」维护，数据会参与所有图表与统计。

**Q：New API 抓不到用量？**
A：按顺序排查：① 优先使用 API Token 认证；② 站点地址只填根域名，不要带 `/api`；
③ 若金额量级不对，调整账号的「积分兑换比」（默认 500000）；④ 部分部署关闭了用量接口，
可在设置中把货币改为「积分」观察原始值。

**Q：金额单位是什么？**
A：New API 的 `quota` 为积分，除以 `quota_per_unit`（默认 500000）得到货币金额。
该值可从站点 `/api/status` 自动探测，也可以在账号表单中手工覆盖。

**Q：数据存在哪？会不会上传？**
A：全部存在本机应用数据目录的 JSON 文件中，应用不依赖也不访问任何自有后端服务。
