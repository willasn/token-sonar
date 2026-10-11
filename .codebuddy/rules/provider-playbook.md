---
description: 新增一家厂商（ProviderKind）时的完整改动清单与登录获取接入步骤
globs: src-tauri/src/providers/**,ui/src/lib/kinds.ts,ui/src/components/ProviderForm.tsx,ui/src/views/Providers.tsx
alwaysApply: false
---

# 新增厂商适配器 Playbook

按清单从上到下改，每一处都是独立可验证的单元。

## A. 新厂商类型（必改）

| # | 文件 | 改动点 |
| --- | --- | --- |
| 1 | `src-tauri/src/models.rs` | `ProviderKind` 加变体 + `#[serde(rename)]`；`as_str()` / `default_base_url()` / `default_name()` 三个 `match` 各加分支 |
| 2 | `src-tauri/src/providers/<kind>.rs` | 新建：实现 `fetch(cfg, http, days) -> UsageSnapshot` 与 `probe(cfg, http) -> Result<String,String>`；字段解析复用 `providers/util.rs`（`pick` / `pick_f64` / `find_usage_records`） |
| 3 | `src-tauri/src/providers/mod.rs` | `pub mod <kind>;`；`fetch_snapshot()` 与 `probe()` 的 `match` 加分支 |
| 4 | `ui/src/lib/kinds.ts` | `KIND_INFO` 加一项（类型卡片 / 名称 / 图标 / 主题色 / `url` `api` 字段配置自动生效）；`MODES_BY_KIND` / `MODE_INFO` 加接入方式 |
| 5 | `ui/src/components/ProviderForm.tsx` | 表单字段按 `KIND_INFO` 表驱动一般无需改；若新类型有专属字段（如 AK/SK、货币换算），在表里加 `akSk` / `currencyFields` 即可，勿写死类型分支 |
| 6 | `ui/src/lib/kinds.ts` | `FILTERS` 加筛选 pill（`KIND_FILTERS` 随 `KIND_ORDER` 自动生效） |
| 7 | 全部视图 (`ui/src/views/*.tsx`) | 确认无 `p.kind === "x" ? A : B` 二元判断，一律查 `KIND_INFO` / `kindInfo(kind)` |
| 8 | `ui/src/lib/mock.ts` | Mock：`DEMO_PROVIDERS` 加一条 + 快照生成函数加分支 |
| 9 | `FEATURES.md` | 「支持的厂商类型」表格 + 「表单字段」登记凭证与接口 |

## B. 追加「登录获取」能力

> 目前**只有 DeepSeek 官方**保留外置登录窗口（`login.rs` 是 DeepSeek 专用实现）。
> New API 曾用同一套机制抓网页会话，因站点域名由用户填写、无法预先写进 capability，
> 且用户级接口还要求 `New-Api-User` 头配合，链路脆弱，已移除、只保留系统访问令牌。
> 新增登录窗口前先确认：站点是否有稳定的取数接口，能否只靠用户填的令牌完成。

**B1 账号密码直连（无窗口）**：在 `providers/<kind>.rs` 写一个读取凭证的辅助函数（签名按各厂商自定，
如 `login(cfg) -> Result<Session, String>`）；需要新认证方式则在 `models.rs` 的 `AuthType` 加变体；
前端 `buildBody()` 加用户名/密码字段。

**B2 外置 WebView 登录窗口**（当前形态以 DeepSeek 为例，`login.rs` 仅服务它一个站点）：
1. `src-tauri/src/login.rs`：把现在写死的 `DEEPSEEK_WINDOW` / `DEEPSEEK_LOGIN_URL` / `start_login()` /
   `wait_for_token()` 重新参数化成「登录目标」（窗口标签、起始地址、窗口标题、回传命令、验证函数），
   再为新站点加一份配置
   - 域名固定 → `init_script()` 传回传命令名，并在 `capabilities/login.json` 加 `remote.urls` + `windows` + `permissions`
   - 域名由用户填写（自建站点）→ 只能走标题回传（`TLTOKEN|`），且该窗口不能授予任何 IPC 权限
2. `providers/<kind>.rs` → 实现验证函数（如 `verify_platform_token(http, token)`），
   请求一次只读接口，HTTP 200 才算有效
3. `lib.rs` → `generate_handler![]` 注册 `start_<kind>_login`
4. `build.rs` → `COMMANDS` 加命令名
5. `capabilities/default.json` → `allow-start-<kind>-login`
6. `ui/src/components/ProviderForm.tsx` → 表单字段加登录按钮（`.field-row`）；
   `runLogin()` 目前只调 `api.startDeepseekLogin()`，多站点时需泛化为按类型调用（含参数校验）
7. `ui/src/lib/mock.ts` → `mockInvoke()` 加 `case "start_<kind>_login"`

## C. 注入脚本捕获凭证的通用做法

`login.rs` 的 `INIT_SCRIPT` 已统一：Hook `window.fetch` / `XHR` 的 `Authorization`，
并周期性扫描 `localStorage` / `sessionStorage` 里 `sk-…`、JWT 或 token/jwt/auth/key/session 类字段的值。
候选凭证全部入队（脚本对每个值只回传一次），优先走 IPC 命令回传，失败退回 `document.title`（`TLTOKEN|`）；
脚本每 5 秒回写 `TLPING|` 心跳，便于区分「页面里没有凭证」与「注入 / 回传通道没生效」。
新站点只需确认它把令牌放在哪里，必要时在 `push()` 的 `isCandidate()` 里放宽过滤规则。

> 站点若把会话放在 **httpOnly Cookie** 里（脚本读不到），只能由后端向登录窗口的 Cookie 仓库索取
> 并拼成 `Cookie` 头验证 —— 这套「Cookie 通道」原为 New API 而写，已随其登录方式一并移除；
> 再次需要时参照 git 历史中的 `site_cookie_header()` 重新实现。

## D. 验证

```bash
cd src-tauri && cargo check --offline
npm run web:check         # 前端类型检查（tsc --noEmit）
npm run web:dev           # 演示模式点「添加账号」走一遍表单
```
