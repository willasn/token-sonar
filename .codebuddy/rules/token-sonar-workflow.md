---
description: TokenSonar 改动流程：先定位后读取、按模块边界最小改动、上下文最小化
globs: **/*
alwaysApply: true
---

# TokenSonar 上下文最小化与模块边界

项目为 **Rust（Tauri 2.x）+ Vite + React + TypeScript 前端**（`tauri.conf.json` 的 `frontendDist = ../ui/dist`）。
任何任务开始前必须遵守本流程：先定位，再读取，只改边界内的文件。

## 1. 铁律：按需读文件

1. **先查结构**：不确定改动落点时，只读 `README.md` 的「目录结构」段 + `FEATURES.md` 对应章节，不要翻代码。
2. **先 grep 符号，再读文件**：定位一律用 Grep / Glob 按符号名搜索（`ProviderKind`、`KIND_INFO`、`MODES_BY_KIND`、`fetch_snapshot`、`useStore`、`api.*` …）。禁止一次性 Read 整个目录或整包文件。
3. **只读直接相关文件**：单个任务读文件数控制在 5 个以内；大文件用 `offset` / `limit` 读片段，不整篇读。
4. **功能点 ↔ 实现对应关系查 `FEATURES.md`**；本项目无 feature_flags 注册表，开关类配置在 `src-tauri/src/models.rs` 的 Settings 与 `commands.rs` 的 `get_settings` / `save_settings`。
5. **只在新增功能时更新 `FEATURES.md` 登记；目录结构变动时更新 `README.md` 目录结构段。**

## 2. 模块边界表（改哪儿，只读哪儿）

| 改动类型 | 必读文件 | 落点 |
| --- | --- | --- |
| 厂商取数 / 接口字段 | `src-tauri/src/providers/<kind>.rs`、`providers/util.rs` | 只改适配器，不动 `commands.rs` |
| 新增/调整厂商类型 | `models.rs`（`ProviderKind`）、`providers/mod.rs`、`ui/src/lib/kinds.ts`（`KIND_INFO` / `MODES_BY_KIND` / `MODE_INFO`）、`ui/src/components/ProviderForm.tsx` | 见 `provider-playbook.md` |
| 登录 / 取凭证通道 | `src-tauri/src/login.rs`、`capabilities/*.json`、`build.rs`、`lib.rs` | 新增目标走 `Target` 枚举 |
| 新增 Tauri 命令 | `commands.rs`、`lib.rs`、`build.rs`、`capabilities/default.json`、`ui/src/lib/mock.ts`（Mock） | 见下方「三件套」 |
| 数据模型 / 字段 | `models.rs`、`storage.rs`、前端 `ui/src/types.ts` + `ui/src/state/store.tsx` + 表单 `ui/src/components/ProviderForm.tsx` | serde 一律 camelCase |
| 前端视图 | `ui/src/views/<View>.tsx`、`ui/src/state/store.tsx` | 跨视图共享逻辑放 `store.tsx` / `components/ui.tsx` |
| UI 样式 | `ui/src/styles/tokens.css`（变量）→ `components.css`（通用件）→ `views.css`（视图级） | 优先复用已有 class |

## 3. 必改清单（漏一处就跑不起来）

**新增 Tauri 命令（四件套）**
1. `src-tauri/src/build.rs` → `COMMANDS` 数组加命令名（**漏了会直接构建失败：Permission not found**）
2. `src-tauri/src/lib.rs` → `generate_handler![]` 注册
3. `src-tauri/capabilities/default.json` → `allow-<命令名>`
4. `ui/src/lib/mock.ts` → `mockInvoke()` 加 `case`（否则演示模式抛「未实现命令」）

**新增厂商类型**：见 `.codebuddy/rules/provider-playbook.md`。

**已知陷阱**
- 前端**禁止** `p.kind === "deepseek" ? A : B` 的二元判断，必须查 `KIND_INFO` / `kindInfo(kind)` 表，否则新增第三种类型会**静默走 B 分支**。
- `login.rs` 的登录窗口：域名固定的站点可走 IPC 通道并写进 `capabilities/login.json`；域名由用户填写的自建站点只能用**标题通道**（`document.title` + `on_document_title_changed`），不授予 IPC 权限。
- 自签证书站点：WebView 能开，但后端 reqwest 严格校验，验签会失败（既有行为）。

## 4. 解耦约定（写新代码时遵守）

- 厂商专属逻辑**只放** `providers/<kind>.rs`；禁止在 `commands.rs` / `models.rs` / 前端写厂商分支。
- 后端分发只走 `providers/mod.rs` 的 `match`；前端类型分支只走 `KIND_INFO` 查表。
- 前端只通过 `ui/src/lib/api.ts` 的 `invoke` / `api.*` 访问后端，禁止直接调用 `window.__TAURI_INTERNALS__`。
- 新增能力优先「加文件 / 加表项」，而不是往已有分支里加 `else`（开闭原则，保证改动独立）。
- 新增 Toast / 弹层 / 图标复用 `components/ui.tsx`（`useUi()`）/ `lib/icons.tsx`，不重复实现。

## 5. 个人账号与本地配置隔离（红线）

**任何进入 Git 的文件里，绝对不能出现个人账号信息与个人配置信息。** 这类信息一律放进被 `.gitignore` 忽略的本地配置文件，不随代码推送到远程仓库。

### 5.1 禁止入库的内容（写入代码 / 文档 / 注释 / 测试 / Mock / 脚本均算违规）

- 真实令牌：API Key、`sk-…`、JWT、Session、Cookie、Bearer 值、密码
- 个人账号标识：用户名、手机号、邮箱、用户 ID、租户 / 组织 ID
- 个人站点信息：自建站点域名、内网 IP、私有 Base URL、自定义端口
- 本机环境信息：`/Users/xxx/…` 绝对路径、本机端口、本机证书 / 密钥文件路径
- 任何从真实抓包、真实调试、真实账号复制来的数据样本

### 5.2 正确做法

1. 个人配置只放本地文件：`config.local.json` / `.env.local` / `*.local.*`（已在 `.gitignore`），
   仓库里只提交不含真实值的模板（如 `config.local.example.json`）。
2. 代码里**只出现键名，不出现值**：读 `env!` / `option_env!` / `std::env::var` / 运行时导入，
   缺失时用占位值或演示值兜底，不把真实值写进任何源码分支。
3. 需要真实数据调试时：临时写入本地配置文件，用完不落库；提交前必须扫一遍 diff。
4. 文档 / `FEATURES.md` / 注释里的示例一律用占位形式：`sk-xxxx`、`https://your-host`、`user@example.com`。
5. 演示数据（`ui/src/lib/mock.ts` 的 `DEMO_PROVIDERS` / 快照生成函数）只能用假数据，禁止粘贴真实账号与用量。

### 5.3 提交前必查

```bash
git status --porcelain                       # 确认本地配置文件未被跟踪
git --no-pager diff --stat                   # 确认改动文件范围
git --no-pager diff | grep -nEi 'sk-[A-Za-z0-9]{8,}|Bearer |(token|apiKey|password|cookie)[[:space:]]*[:=]'  # 抓漏网凭证（命中需人工确认是否占位值）
```

命中真实值：把值迁到本地配置文件，改代码为读取键名，再重新提交。
**发现疑似已泄漏的既有内容：立即报告用户，不要自行改写历史或强行推送。**

## 6. 快速验证（改动后必跑）

```bash
cd src-tauri && cargo check --offline      # 后端：秒级，无 warning 才通过
npm run web:check                          # 前端类型检查（tsc --noEmit，严格模式）
npm run web:build                          # 前端构建（校验 import / JSX / 产物）
npm run web:dev                            # 演示模式（浏览器预览 UI / Mock 数据）
```

## 7. 收尾自查

- [ ] 只改了边界内的文件，未顺手重构无关代码
- [ ] 新增命令/类型已补齐第 3 节的全部必改点
- [ ] 新增功能已登记 `FEATURES.md`（结构变动则补 `README.md` 目录结构）
- [ ] 无个人账号 / 令牌 / 私有域名 / 本机绝对路径入库（第 5 节）
