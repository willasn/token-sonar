/* TokenSonar · 浏览器演示模式（Mock）
 *
 * 直接用浏览器打开（非 Tauri 容器）时，api.ts 会把命令转发到这里。
 * 数据存于 localStorage，全部为假数据，便于在真实账号接入前预览 UI。
 */
import type {
  ApiKeyInfo,
  AppSettings,
  LedgerEntry,
  ProviderConfig,
  UsageSnapshot,
} from "../types";
import { kindInfo, providerInfo } from "./kinds";

const KEY = "tokensonar.demo.v4";

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function dateStr(d: Date): string {
  return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate());
}

function daysBack(n: number): string[] {
  const list: string[] = [];
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    list.push(dateStr(d));
  }
  return list;
}

function nowStamp(): string {
  const d = new Date();
  return (
    d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + " " +
    pad(d.getHours()) + ":" + pad(d.getMinutes()) + ":" + pad(d.getSeconds())
  );
}

function seedRandom(seed: number): () => number {
  let s = seed % 2147483647;
  if (s <= 0) s += 2147483646;
  return function () {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
}

function hash(str: string): number {
  let h = 0;
  for (let i = 0; i < str.length; i++) h = (h * 31 + str.charCodeAt(i)) % 100000;
  return h;
}

const DEMO_PROVIDERS: ProviderConfig[] = [
  {
    id: "pv_demo_bailian_team",
    name: "百炼 Token Plan 团队（演示）",
    kind: "bailian",
    planType: "tokenPlanTeam",
    enabled: true,
    baseUrl: "https://modelstudio.cn-beijing.aliyuncs.com",
    authType: "token",
    accessMode: "api",
    accessKeyId: "DemoAccessKeyId",
    accessKeySecret: "DemoAccessKeySecret",
    icon: "ALI",
    accent: "#ff6a00",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: false,
    cardView: "quota",
    notes: "纯虚构 Credits 演示数据，非货币或 Token 数",
    createdAt: "2026-09-20 10:00:00",
    updatedAt: "2026-09-20 10:00:00",
  },
  {
    id: "pv_demo_hunyuan",
    name: "腾讯混元 / TokenHub（演示）",
    kind: "hunyuan",
    enabled: true,
    baseUrl: "https://tokenhub.tencentcloudapi.com",
    authType: "token",
    accessMode: "api",
    accessKeyId: "demo-secret-id",
    accessKeySecret: "demo-secret-key",
    icon: "HY",
    accent: "#0052d9",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "纯演示数据：腾讯云整体余额与广州 TokenHub 统计，含其他模型；个人套餐额度不可读取",
    createdAt: "2026-09-20 10:00:00",
    updatedAt: "2026-09-20 10:00:00",
  },
  {
    id: "pv_demo_bailian",
    name: "阿里云百炼（演示）",
    kind: "bailian",
    enabled: true,
    baseUrl: "https://business.aliyuncs.com",
    authType: "token",
    accessMode: "api",
    accessKeyId: "demo-access-key-id",
    accessKeySecret: "demo-access-key-secret",
    icon: "ALI",
    accent: "#ff6a00",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "演示账单：账户可用额度非百炼专属，费用仅含当月已出账数据",
    createdAt: "2026-09-20 10:00:00",
    updatedAt: "2026-09-20 10:00:00",
  },
  {
    id: "pv_demo_deepseek",
    name: "DeepSeek 官方",
    kind: "deepseek",
    enabled: true,
    baseUrl: "https://api.deepseek.com",
    authType: "token",
    accessMode: "login",
    token: "",
    username: "",
    password: "",
    sessionToken: "mock-platform-token-demo",
    icon: "DS",
    accent: "#4d6bfe",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "登录授权：读用量、消费金额与 API Keys",
    apiKeys: [
      {
        id: "key_demo_1",
        name: "生产环境",
        masked: null,
        secret: "sk-demo-prod-0001",
        createdAt: "2026-08-12 10:00:00",
        lastUsedAt: "2026-09-27 21:10:00",
        status: "正常",
        source: "manual",
      },
    ],
    createdAt: "2026-09-01 10:00:00",
    updatedAt: "2026-09-01 10:00:00",
  },
  {
    id: "pv_demo_mimo",
    name: "小米 MiMo（演示）",
    kind: "mimo",
    enabled: true,
    baseUrl: "https://api.xiaomimimo.com",
    authType: "token",
    accessMode: "login",
    token: "",
    username: "",
    password: "",
    sessionToken: "mock_cookie=demo",
    icon: "MI",
    accent: "#ff6900",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "登录授权：读余额、逐日逐模型用量、月度账单与 Token Plan 额度（演示数据）",
    createdAt: "2026-09-20 10:00:00",
    updatedAt: "2026-09-20 10:00:00",
  },
  {
    id: "pv_demo_newapi",
    name: "中转站 · New API",
    kind: "newapi",
    enabled: true,
    baseUrl: "https://api.demo-relay.com",
    authType: "token",
    accessMode: "api",
    token: "sk-demo-newapi-token",
    username: "demo",
    password: "",
    sessionToken: "",
    icon: "NA",
    accent: "#00d3a7",
    currency: "USD",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "访问令牌：只读取该令牌有权限的接口",
    createdAt: "2026-09-05 10:00:00",
    updatedAt: "2026-09-05 10:00:00",
  },
  {
    id: "pv_demo_glm",
    name: "GLM / Z.ai",
    kind: "glm",
    enabled: true,
    baseUrl: "https://open.bigmodel.cn",
    authType: "token",
    accessMode: "api",
    token: "sk-demo-glm-key",
    username: "",
    password: "",
    sessionToken: "",
    icon: "GLM",
    accent: "#3859ff",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "API Key：读取套餐等级与 5 小时 / 周 / MCP 额度",
    createdAt: "2026-09-12 10:00:00",
    updatedAt: "2026-09-12 10:00:00",
  },
  {
    id: "pv_demo_kimi",
    name: "Kimi / 月之暗面",
    kind: "kimi",
    enabled: true,
    baseUrl: "https://api.moonshot.cn",
    authType: "token",
    accessMode: "api",
    token: "sk-demo-kimi-key",
    icon: "K",
    accent: "#151515",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "API Key：仅查询可用余额、代金券余额与现金余额",
    createdAt: "2026-09-20 10:00:00",
    updatedAt: "2026-09-20 10:00:00",
  },
  {
    id: "pv_demo_volcengine",
    name: "火山方舟",
    kind: "volcengine",
    enabled: true,
    baseUrl: "https://ark.cn-beijing.volcengineapi.com",
    authType: "token",
    accessMode: "api",
    token: "",
    accessKeyId: "AKLTZGVtby1hY2Nlc3Mta2V5",
    accessKeySecret: "sk-demo-secret",
    username: "",
    password: "",
    sessionToken: "",
    icon: "VE",
    accent: "#1664ff",
    currency: "CNY",
    quotaPerUnit: 500000,
    displayInCurrency: true,
    notes: "AK/SK 签名：读取账户余额、Coding Plan 与模型信息",
    createdAt: "2026-09-18 10:00:00",
    updatedAt: "2026-09-18 10:00:00",
  },
];

const DEMO_MODELS_DS = ["deepseek-chat", "deepseek-reasoner"];
const DEMO_MODELS_MIMO = ["mimo-v2.6-pro", "mimo-v2.6-flash"];
const DEMO_MODELS_NA = [
  "gpt-4o-mini",
  "claude-3-5-sonnet",
  "deepseek-chat",
  "gemini-2.0-flash",
  "qwen-plus",
];

function genSnapshot(p: ProviderConfig): UsageSnapshot {
  if (p.kind === "hunyuan") {
    return {
      providerId: p.id,
      providerName: p.name,
      kind: p.kind,
      currency: "CNY",
      fetchedAt: nowStamp(),
      ok: true,
      error: null,
      account: null,
      balance: 246.8,
      balanceRaw: 24680,
      granted: null,
      used: null,
      requestsTotal: null,
      tokensTotal: null,
      today: null,
      month: null,
      days: [],
      models: [],
      quotas: [
        { label: "企业版演示 · 总额度（credits）", used: 240, quota: 1200, percent: 20 },
        { label: "企业自动版演示 · 总额度（tokens）", used: 300000, quota: 2000000, percent: 15 },
      ],
      logs: [],
      metrics: [
        { label: "数据性质", value: "纯演示数据", hint: "所有数值均为虚构展示，不来自真实账号或接口响应" },
        { label: "统计范围", value: "TokenHub / 广州", hint: "演示；固定官方管控面，不合并新加坡及传统混元历史数据" },
        { label: "腾讯云整体余额", value: "¥246.80", hint: "演示；费用中心账户整体余额，非混元或 TokenHub 专属；原始单位为分" },
        { label: "TokenHub · 今日 Token", value: "18000", hint: "演示；账户统计含其他模型，不等于按量账单费用" },
        { label: "今日 · hunyuan-demo", value: "12000 tokens", hint: "演示模型统计；缓存输入包含在总 Token 中" },
        { label: "今日 · other-model-demo", value: "6000 tokens", hint: "演示；TokenHub 也包含其他模型" },
        { label: "TokenHub · 本月 Token", value: "450000", hint: "演示；账户统计含其他模型，非历史累计量" },
        { label: "本月 · hunyuan-demo", value: "300000 tokens", hint: "演示模型统计；不是计费 Token 或模型费用" },
        { label: "本月 · other-model-demo", value: "150000 tokens", hint: "演示；TokenHub 也包含其他模型" },
        { label: "套餐 · 企业版演示", value: "enterprise / 演示", hint: "仅接口可见企业版 credits；不代表个人订阅状态" },
        { label: "套餐 · 企业自动版演示", value: "enterprise-auto / 演示", hint: "仅接口可见企业版 tokens；无个人滚动额度或重置时间" },
        { label: "个人 Coding / 通用 / Hy Token Plan", value: "公开接口不可读取", hint: "不能以企业套餐为空判定未购买，也不以零额度冒充未知" },
        { label: "费用与请求数", value: "不可读取", hint: "无按量账单费用、全账户请求数、逐日用量或历史累计量；未知字段保持为空" },
      ],
      apiKeys: [],
      supportsUsageApi: true,
    };
  }
  const rnd = seedRandom(hash(p.id) + new Date().getDate());
  const dates = kindInfo(p.kind).independentUsageSources
    ? Array.from({ length: Math.max(30, new Date(Date.now() + 8 * 3600000).getUTCDate()) }, (_, i) => {
      const date = new Date(Date.now() + 8 * 3600000);
      date.setUTCDate(date.getUTCDate() - (Math.max(30, date.getUTCDate()) - 1 - i));
      return date.toISOString().slice(0, 10);
    })
    : daysBack(30);
  const days: UsageSnapshot["days"] = [];
  const models: UsageSnapshot["models"] = [];
  const modelMap: Record<string, { model: string; requests: number; tokens: number; cost: number }> = {};
  const isDs = p.kind === "deepseek";
  const isGlm = p.kind === "glm";
  const isVe = p.kind === "volcengine";
  const isKimi = p.kind === "kimi";
  const isMimo = p.kind === "mimo";
  const hasUsage =
    (isDs && p.accessMode === "login" && !!p.sessionToken) ||
    (isMimo && p.accessMode === "login" && !!p.sessionToken) ||
    p.kind === "newapi" ||
    !!kindInfo(p.kind).independentUsageSources;
  const kimiCurrency = isKimi && new URL(p.baseUrl || "https://api.moonshot.cn").hostname === "api.moonshot.ai" ? "USD" : "CNY";
  const pool = isDs ? DEMO_MODELS_DS : isMimo ? DEMO_MODELS_MIMO : DEMO_MODELS_NA;

  let tokensTotal = 0;
  let costTotal = 0;
  let reqTotal = 0;

  dates.forEach((d, i) => {
    let tokens = 0;
    let cost = 0;
    let reqs = 0;

    if (hasUsage) {
      const wave = 0.55 + 0.45 * Math.sin(i / 3.1) + rnd() * 0.35;
      reqs = Math.round(60 + wave * 420);
      tokens = Math.round(reqs * (900 + rnd() * 2600));
      cost = Number(Number((tokens / 1000000) * (0.6 + rnd() * 2.4)).toFixed(2));
    }

    if (hasUsage) {
      days.push({ date: d, requests: reqs, tokens, cost });
      tokensTotal += tokens;
      costTotal += cost;
      reqTotal += reqs;
      const m = pool[i % pool.length];
      if (!modelMap[m]) modelMap[m] = { model: m, requests: 0, tokens: 0, cost: 0 };
      modelMap[m].requests += reqs;
      modelMap[m].tokens += tokens;
      modelMap[m].cost += cost;
    }
  });

  for (const k in modelMap) models.push(modelMap[k]);
  models.sort((a, b) => b.cost - a.cost);

  const today = dates[dates.length - 1];
  const t = hasUsage ? days[days.length - 1] : null;

  const snap: UsageSnapshot = {
    providerId: p.id,
    providerName: p.name,
    kind: p.kind,
    currency: isKimi ? kimiCurrency : p.currency,
    fetchedAt: nowStamp(),
    ok: true,
    error: null,
    account: isDs || isKimi ? null : isMimo ? "demo@mimo.example.com" : isVe ? "2100000001" : isGlm ? "demo@bigmodel.cn" : "demo",
    balance: isKimi ? 88.6 : isDs ? 128.42 : isMimo ? 286.5 : isVe ? 986.42 : isGlm ? 42.5 : 36.75,
    balanceRaw: p.kind === "newapi" ? 18375000 : null,
    granted: isDs ? 50 : null,
    used: hasUsage ? Number((costTotal * (isDs ? 1 : 1.6)).toFixed(2)) : isGlm ? 57.5 : null,
    requestsTotal: hasUsage ? (isDs ? reqTotal : reqTotal + 128400) : null,
    tokensTotal: hasUsage ? tokensTotal : null,
    today: t ? { requests: t.requests, tokens: t.tokens, cost: t.cost, from: today, to: today } : null,
    month: hasUsage ? {
      requests: reqTotal,
      tokens: tokensTotal,
      cost: Number(costTotal.toFixed(2)),
      from: today.slice(0, 8) + "01",
      to: today,
    } : null,
    days: days,
    models: models,
    quotas: isMimo
      ? [{ label: "Token Plan · 套餐额度（演示）", used: 1_200_000_000, quota: 4_100_000_000, percent: 29, resetAt: "2026-11-20T00:00:00+08:00" }]
      : [],
    logs: [],
    metrics: [],
    apiKeys: [],
    supportsUsageApi: hasUsage,
  };

  if (providerInfo(p).quotaOnly) {
    snap.account = null;
    snap.balance = null;
    snap.used = null;
    snap.requestsTotal = null;
    snap.tokensTotal = null;
    snap.today = null;
    snap.month = null;
    snap.days = [];
    snap.models = [];
    snap.supportsUsageApi = false;
    if (providerInfo(p).cliUsage) {
      if (p.planType === "codingPlan") {
        snap.quotas = [
          { label: "近 5 小时 · 调用次数（演示）", used: 25, quota: 100, percent: 25, resetAt: "2026-11-01T00:00:00+08:00" },
          { label: "周窗口 · 调用次数（演示）", used: 120, quota: 1000, percent: 12 },
          { label: "账单月 · 调用次数（演示）", used: 300, quota: 4000, percent: 7.5 },
        ];
      } else {
        snap.quotas = [{ label: "Harness 权益 1 · 次（演示）", used: 10, quota: 100, percent: 10 }];
        snap.error = "个人版月 Credits 未知；Harness 权益不能代替月 Credits（演示）";
      }
      snap.metrics = [{ label: "数据口径", value: "纯虚构 CLI 演示数据，不执行本机 CLI、不验证账号绑定" }];
    } else {
      snap.quotas = [
        { label: "标准席位 · Credits（演示）", used: 60, quota: 100, percent: 60, resetAt: "2026-11-01T00:00:00+08:00" },
        { label: "共享包 1 · Credits（演示）", used: 20, quota: 200, percent: 10, resetAt: "2026-11-01T00:00:00+08:00" },
      ];
      snap.metrics = [{ label: "数据口径", value: "虚构 Token Plan 团队版演示数据", hint: "Credits 非货币、非 Token；不乘席位数、不重复累加席位明细" }];
    }
    return snap;
  }
  if (providerInfo(p).billingUsage) {
    snap.account = "demo";
    snap.balance = 321.5;
    snap.month = { requests: 0, tokens: 1250000, cost: 12.8, from: today.slice(0, 8) + "01", to: today };
    snap.models = [
      { model: "qwen-plus", requests: 0, tokens: 1250000, cost: 10.8 },
      { model: "qwen-demo", requests: 0, tokens: 0, cost: 2 },
    ];
    snap.metrics = [
      { label: "阿里云账户可用额度", value: "321.50", hint: "演示数据；账户级可用额度，非百炼专属" },
      { label: "当月百炼已出账费用", value: "12.80", hint: "演示数据；当月已出账费用，非实时用量" },
      { label: "当月计费 Token", value: "1250000", hint: "仅含可识别的 Token 计费项" },
      { label: "原单位账单用量（演示）", value: "1250 千 Token；2 次", hint: "不同原始单位不可直接相加" },
      { label: "数据口径", value: "当月已出账账单", hint: "存在出账延迟；无账单不代表没有实际使用" },
    ];
  } else if (isKimi) {
    snap.metrics = [
      { label: `可用余额（${kimiCurrency}）`, value: "88.60", hint: "Moonshot 官方余额接口" },
      { label: `代金券余额（${kimiCurrency}）`, value: "20.00", hint: null },
      { label: `现金余额（${kimiCurrency}）`, value: "68.60", hint: null },
    ];
  } else if (isMimo) {
    snap.metrics = [
      { label: "余额（CNY）", value: "286.50", hint: "演示数据；按量付费账户余额" },
      { label: "充值余额", value: "200.00", hint: null },
      { label: "赠送余额", value: "86.50", hint: null },
      { label: "Token Plan 套餐", value: "Lite（演示）", hint: "订阅套餐与按量付费余额不互通" },
      { label: "套餐到期时间", value: "2026-11-20 00:00:00", hint: null },
      { label: "平台累计消费（CNY）", value: "128.40", hint: "演示数据；控制台账单明细累计值" },
      { label: "平台累计 Token", value: "18600000", hint: "含缓存输入、批量推理与 ASR（演示）" },
      { label: "本月消费（CNY）", value: costTotal.toFixed(2), hint: "演示数据；当月已出账账单" },
      { label: "本月 Token", value: String(tokensTotal), hint: String(reqTotal) + " 次请求（演示）" },
    ];
  } else if (isDs) {
    snap.account = p.username || "demo@deepseek.com";
    const apiKeyList: ApiKeyInfo[] = [
      {
        id: "api_1",
        name: "默认 Key",
        masked: "sk-****0f3a",
        secret: null,
        createdAt: "2026-07-03 11:20:00",
        lastUsedAt: "2026-09-28 09:04:00",
        status: "正常",
        source: "api",
      },
      {
        id: "api_2",
        name: "测试环境",
        masked: "sk-****9c21",
        secret: null,
        createdAt: "2026-08-19 16:45:00",
        lastUsedAt: "2026-09-26 14:31:00",
        status: "正常",
        source: "api",
      },
    ];
    (p.apiKeys || []).forEach((k) => {
      apiKeyList.push(Object.assign({}, { masked: "sk-****0001" }, k, { source: "manual" as const }));
    });
    snap.apiKeys = apiKeyList;
    snap.metrics = [
      { label: "余额（CNY）", value: "128.42", hint: "DeepSeek 余额接口" },
      { label: "赠送余额", value: "50.00", hint: null },
      { label: "充值余额", value: "78.42", hint: null },
      { label: "可用模型", value: "2 个", hint: null },
      { label: "API Keys", value: snap.apiKeys.length + " 个", hint: "其中 1 个来自手动录入" },
    ];
    if (hasUsage && t) {
      const monthName = today.slice(0, 7);
      snap.metrics.push(
        {
          label: "累计消费金额（近 2 个自然月）",
          value: (costTotal * 2.1).toFixed(2) + " CNY",
          hint: "平台按自然月返回，这里合计最近两个自然月",
        },
        {
          label: "本月消费（" + monthName + "）",
          value: costTotal.toFixed(2) + " CNY",
          hint: monthName + "-01 ~ " + today,
        },
        { label: "上月消费", value: (costTotal * 1.1).toFixed(2) + " CNY", hint: null },
        { label: "本月 API 请求", value: String(reqTotal), hint: "上月 " + Math.round(reqTotal * 1.1) },
        { label: "本月 Token 用量", value: String(tokensTotal), hint: null },
        {
          label: "今日消费",
          value: t.cost.toFixed(2) + " CNY",
          hint: t.requests + " 次请求 · " + t.tokens + " tokens",
        },
        { label: "输入缓存命中率", value: "62.4%", hint: "命中 128k / 未命中 77k / 输出 46k" },
        { label: "用量数据来源", value: "DeepSeek 平台内部接口", hint: "平台登录授权后获取" },
      );
    }
  } else if (isGlm) {
    snap.metrics = [
      {
        label: "账户余额（CNY）",
        value: "42.50",
        hint: "累计充值 100.00 · 累计消费 57.50",
      },
      { label: "套餐等级", value: "GLM Coding 套餐", hint: null },
      {
        label: "5 小时额度",
        value: "37.4% 已用",
        hint: "已用 1120 / 上限 3000 · 重置于 " + today,
      },
      { label: "周额度", value: "12.6% 已用", hint: "已用 3800 / 上限 30000" },
      { label: "MCP 次数额度", value: "9.0% 已用", hint: "已用 45 / 上限 500" },
      {
        label: "额度数据来源",
        value: "GLM 开放平台额度接口",
        hint: "GET /api/monitor/usage/quota/limit",
      },
    ];
  } else if (isVe) {
    const monthDays = days.filter((d) => d.date >= today.slice(0, 8) + "01");
    // Independent fake bill amounts, not calculated from inference tokens.
    days.forEach((d, i) => { d.cost = d.date >= today.slice(0, 8) + "01" ? Number((0.8 + (i % 7) * 0.35).toFixed(2)) : -1; });
    snap.month = {
      requests: monthDays.reduce((n, d) => n + d.requests, 0),
      tokens: monthDays.reduce((n, d) => n + d.tokens, 0),
      cost: monthDays.reduce((n, d) => n + d.cost, 0),
      from: today.slice(0, 8) + "01", to: today,
    };
    snap.today = { ...days[days.length - 1], from: today, to: today };
    snap.used = null;
    snap.tokensTotal = null;
    snap.requestsTotal = null;
    snap.models = [{ model: "doubao-demo", requests: snap.month.requests, tokens: snap.month.tokens, cost: -1 }];
    snap.quotas = [
      { label: "近5小时用量", used: 43.05, quota: 50, percent: 86.11, resetIn: "4小时12分后重置" },
      { label: "近一周用量", used: 246.6, quota: 500, percent: 49.32, resetIn: "5天14小时后重置" },
      { label: "近一月用量", used: 900.4, quota: 2000, percent: 45.02, resetIn: "28天14小时后重置" },
    ];
    snap.metrics = [
      { label: "当月输入 Token", value: String(Math.round(snap.month.tokens * 0.7)), hint: "演示数据；北京时间 default 项目 normal 正常后付费推理" },
      { label: "当月输出 Token", value: String(snap.month.tokens - Math.round(snap.month.tokens * 0.7)), hint: "演示数据；北京时间 default 项目 normal 正常后付费推理" },
      { label: "当月按量已出账应付金额", value: snap.month.cost.toFixed(2), hint: "演示数据；账户级火山方舟按量消费账单，独立于推理 Token 生成；非实时且可能延迟" },
      { label: "按量数据口径", value: "推理用量与账单双源", hint: "演示数据；Token/请求为北京时间 default 项目 normal 正常后付费推理；费用为账户当月按量已出账应付金额，两者范围不同。不包含 Coding/Agent 套餐，不关联模型费用；上月账单费用及模型费用未知，-1 不代表零；累计值未提供。" },
      {
        label: "账户余额（CNY）",
        value: "986.42",
        hint: "现金 986.42 · 信控额度 10000.00",
      },
      { label: "Coding Plan", value: "Lite · 生效中", hint: "已开启自动续费" },
      { label: "Coding Plan 开始时间", value: today.slice(0, 8) + "01", hint: null },
      { label: "Coding Plan 结束时间", value: today, hint: null },
      { label: "Agent Plan", value: "Small · 生效中", hint: "已开启自动续费" },
      { label: "Agent Plan 开始时间", value: today.slice(0, 8) + "01", hint: null },
      { label: "Agent Plan 结束时间", value: today, hint: null },
      {
        label: "已开通模型",
        value: "12 个",
        hint:
          "含 Doubao Seed 2.1 Pro、Doubao Seed 2.1 Lite、Kimi K2.8 Preview、" +
          "DeepSeek V3.2、GLM-4.7、Qwen3-Max 等",
      },
      {
        label: "Coding Plan 可用模型",
        value: "4 个",
        hint: "doubao-seed-2.1-pro、doubao-seed-2.1-lite、kimi-k2.8-preview、deepseek-v3.2",
      },
      {
        label: "套餐数据来源",
        value: "方舟管控面接口",
        hint: "POST /?Action=GetPersonalPlan&Version=2024-01-01",
      },
      {
        label: "套餐用量数据来源",
        value: "方舟管控面接口",
        hint: "POST /?Action=GetAFPUsage&Version=2024-01-01",
      },
    ];
  } else {
    snap.logs = days.slice(-12).map((d, i) => ({
      id: "log_" + i,
      time: d.date + " 14:2" + (i % 10) + ":11",
      model: pool[(i * 3) % pool.length],
      tokens: d.tokens,
      cost: d.cost,
      durationMs: 800 + i * 130,
      status: "成功",
    }));
    snap.metrics = [
      { label: "剩余额度（USD）", value: "36.75", hint: "原始积分 18375000" },
      { label: "累计消耗", value: snap.used!.toFixed(2), hint: null },
      { label: "累计请求数", value: String(128400 + reqTotal), hint: null },
      { label: "分组", value: "default", hint: null },
    ];
  }
  return snap;
}

interface DemoDb {
  providers: ProviderConfig[];
  ledger: LedgerEntry[];
  snapshots: Record<string, UsageSnapshot>;
  settings: AppSettings | null;
  sync?: Record<string, unknown>;
}

function loadDemo(): DemoDb {
  let data: DemoDb | null = null;
  try {
    data = JSON.parse(localStorage.getItem(KEY) || "null");
  } catch {
    data = null;
  }
  if (!data || !data.providers) {
    data = { providers: DEMO_PROVIDERS.slice(), ledger: [], snapshots: {}, settings: null };
    saveDemo(data);
  }
  return data;
}

function saveDemo(d: DemoDb): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(d));
  } catch {
    /* 忽略 */
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function snapshotsOf(db: DemoDb): UsageSnapshot[] {
  return db.providers
    .map((p) => db.snapshots[p.id])
    .filter(Boolean) as UsageSnapshot[];
}

const DEFAULT_SETTINGS: AppSettings = {
  theme: "dark",
  language: "",
  currency: "CNY",
  autoRefresh: true,
  refreshIntervalMin: 15,
  launchAtLoginHint: true,
  lastView: "dashboard",
  providersView: "card",
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function mockInvoke(cmd: string, args: Record<string, any>): Promise<any> {
  const db = loadDemo();
  switch (cmd) {
    case "list_providers":
      return db.providers;
    case "save_provider": {
      const p: ProviderConfig = Object.assign({}, args.provider);
      if (!p.id) p.id = "pv_" + Date.now().toString(16);
      p.updatedAt = nowStamp();
      if (!p.createdAt) p.createdAt = p.updatedAt;
      const idx = db.providers.findIndex((x) => x.id === p.id);
      if (idx >= 0) db.providers[idx] = p;
      else db.providers.push(p);
      saveDemo(db);
      return p;
    }
    case "remove_provider": {
      db.providers = db.providers.filter((p) => p.id !== args.id);
      delete db.snapshots[args.id];
      db.ledger = db.ledger.filter((e) => e.providerId !== args.id);
      saveDemo(db);
      return null;
    }
    case "reorder_providers": {
      const ids: string[] = args.ids || [];
      const byId: Record<string, ProviderConfig> = {};
      db.providers.forEach((p) => {
        byId[p.id] = p;
      });
      const ordered: ProviderConfig[] = [];
      ids.forEach((id) => {
        if (byId[id]) {
          ordered.push(byId[id]);
          delete byId[id];
        }
      });
      db.providers.forEach((p) => {
        if (byId[p.id]) ordered.push(p);
      });
      db.providers = ordered;
      saveDemo(db);
      return db.providers;
    }
    case "test_provider":
      await delay(600);
      if (providerInfo(args.provider).cliUsage) return "CLI 只读额度查询（纯虚构演示，未验证真实 Profile；个人版月 Credits 仍未知）";
      if (providerInfo(args.provider).quotaOnly) return "Token Plan 团队版只读额度接口（演示模式，非实际凭证验证）";
      return "连接成功（演示模式），余额 128.42 CNY";
    case "start_deepseek_login":
      await delay(900);
      return "mock-platform-token-" + Date.now().toString(16);
    case "start_mimo_login":
      await delay(900);
      return "mock_cookie=token-" + Date.now().toString(16);
    case "deepseek_login_token":
      return null;
    case "refresh_provider": {
      await delay(500);
      const p = db.providers.find((x) => x.id === args.id);
      if (!p) throw new Error("未找到该账号");
      const snap = genSnapshot(p);
      db.snapshots[p.id] = snap;
      saveDemo(db);
      return snap;
    }
    case "refresh_all": {
      await delay(900);
      db.providers.forEach((p) => {
        if (!p.enabled) return;
        db.snapshots[p.id] = genSnapshot(p);
      });
      saveDemo(db);
      return snapshotsOf(db);
    }
    case "get_snapshots": {
      db.providers.forEach((p) => {
        if (!db.snapshots[p.id] || p.kind !== "newapi") {
          db.snapshots[p.id] = genSnapshot(p);
        }
      });
      saveDemo(db);
      return snapshotsOf(db);
    }
    case "list_ledger":
      return db.ledger.filter((e) => !args.providerId || e.providerId === args.providerId);
    case "add_ledger_entry": {
      const e: LedgerEntry = Object.assign({}, args.entry);
      if (!e.id) e.id = "lg_" + Date.now().toString(16);
      if (!e.date) e.date = dateStr(new Date());
      e.createdAt = nowStamp();
      db.ledger.push(e);
      saveDemo(db);
      return e;
    }
    case "remove_ledger_entry": {
      db.ledger = db.ledger.filter((e) => e.id !== args.id);
      saveDemo(db);
      return null;
    }
    case "get_settings":
      return db.settings || DEFAULT_SETTINGS;
    case "save_settings":
      db.settings = args.settings;
      saveDemo(db);
      return null;
    case "get_app_info":
      return { name: "TokenSonar", version: "0.1.0 (demo)", dataDir: "浏览器演示模式" };
    case "open_data_dir":
      return null;
    case "open_external_url":
      return null;

    /* ---------------------------- Git 同步 ---------------------------- */
    case "get_sync_config":
      return (
        db.sync || {
          repoUrl: "",
          username: "",
          password: "",
          branch: "main",
          lastSyncAt: "",
          lastMessage: "",
        }
      );
    case "save_sync_config": {
      const cfg = Object.assign({}, args.config);
      if (!cfg.branch) cfg.branch = "main";
      db.sync = cfg;
      saveDemo(db);
      return cfg;
    }
    case "test_sync_connection": {
      await delay(700);
      const cfg = args.config || {};
      if (!String(cfg.repoUrl || "").trim()) throw new Error("请先填写仓库地址");
      return "演示模式：模拟连接成功，远端可写";
    }
    case "pull_remote_backup": {
      await delay(700);
      const cfg = db.sync || {};
      if (!String(cfg.repoUrl || "").trim()) throw new Error("请先在「Git 同步」里配置并保存仓库");
      // 演示模式：远端备份等同于本机数据，因此不会产生冲突
      return {
        providers: db.providers.slice(),
        settings: db.settings || null,
        branch: cfg.branch || "main",
        commit: "demo123",
      };
    }
    case "sync_to_repo": {
      await delay(1200);
      const cfg = db.sync || {};
      if (!String(cfg.repoUrl || "").trim()) throw new Error("请先在「Git 同步」里配置并保存仓库");
      let list: ProviderConfig[] = Array.isArray(args.providers) ? args.providers.slice() : db.providers.slice();
      // 不含凭证模式：与后端一致，落库前抹掉密码与各类密钥
      if (args.includeCredentials === false) {
        list = list.map((p) => {
          const c: ProviderConfig = Object.assign({}, p, {
            token: null,
            accessKeySecret: null,
            sessionToken: null,
            password: null,
            platformToken: null,
          });
          if (Array.isArray(c.apiKeys)) {
            c.apiKeys = c.apiKeys.map((k: ApiKeyInfo) => Object.assign({}, k, { secret: null }));
          }
          return c;
        });
      }
      db.providers = list;
      const at = nowStamp();
      const msg = "演示模式：已模拟推送到 " + (cfg.branch || "main") + "（" + db.providers.length + " 个账号）";
      db.sync = Object.assign({}, cfg, { lastSyncAt: at, lastMessage: msg });
      saveDemo(db);
      return { ok: true, message: msg, commit: "demo123", changed: true, fileCount: 3, syncedAt: at };
    }
    default:
      throw new Error("演示模式未实现命令: " + cmd);
  }
}

export function mockReset(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
}
