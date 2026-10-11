/* TokenSonar · 厂商类型元数据表
 *
 * 前端所有厂商类型分支都必须查这张表（KIND_INFO / MODES_BY_KIND / MODE_INFO），
 * 禁止写 `kind === "deepseek" ? A : B` 的二元判断。
 * 新增渠道类型时：后端 models.rs 加变体 + 本文件加一项 + providers 适配器。
 */
import type { ProviderConfig, ProviderKind, UsageSnapshot } from "../types";

export interface KindField {
  label: string;
  hint?: string;
  placeholder?: string;
}

export interface KindInfo {
  /** i18n key：类型名 */
  name: string;
  /** i18n key：类型说明 */
  desc: string;
  icon: string;
  accent: string;
  chip: string;
  /** 列表卡片用的类型标签 i18n key */
  kindLabel: string;
  /** 详情页用的类型标签 i18n key */
  detailLabel: string;
  /** 官方厂商：站点地址即官方接口地址，可留空走默认值 */
  official: boolean;
  /** 卡片左上角「控制台」入口地址；空串表示用用户填写的站点地址 */
  console: string;
  url: KindField;
  api: KindField;
  /** 登录模式的凭证字段（仅 DeepSeek 有） */
  login?: KindField;
  /** 中转站额度是站点积分，需要货币 / 换算比设置 */
  currencyFields?: boolean;
  /** 该渠道用 Access Key ID + Secret Key 双字段签名鉴权 */
  akSk?: boolean;
  accessKeyIdLabel?: string;
  accessKeySecretLabel?: string;
  /** 仅提供账户余额分项，不提供服务端逐日或模型用量 */
  balanceOnly?: boolean;
  /** 提供当月已出账账单，不是日用量或累计用量接口 */
  billingUsage?: boolean;
  /** 推理与账单双源，负数表示未读取到该维度 */
  independentUsageSources?: boolean;
  usageScope?: string;
  /** 接入凭证在 Keys 中的分类，缺省为 API Key */
  credentialType?: "api" | "system";
  plans?: { key: NonNullable<ProviderConfig["planType"]>; label: string }[];
  quotaOnly?: boolean;
  cliUsage?: boolean;
  planHint?: string;
}

export const KIND_INFO: Record<ProviderKind, KindInfo> = {
  hunyuan: {
    name: "kind.hunyuan.name",
    desc: "kind.hunyuan.desc",
    icon: "HY",
    accent: "#0052d9",
    chip: "chip-accent",
    kindLabel: "providers.kindHunyuan",
    detailLabel: "detail.kindHunyuan",
    official: true,
    console: "https://console.cloud.tencent.com/tokenhub",
    url: { label: "form.apiUrl", hint: "form.hunyuanUrlHint", placeholder: "https://tokenhub.tencentcloudapi.com" },
    api: { label: "form.accessKey", hint: "form.hunyuanAkHint", placeholder: "AKIDxxxxxxxxxxxxxxxx" },
    akSk: true,
    accessKeyIdLabel: "form.secretId",
    accessKeySecretLabel: "form.secretKey",
    usageScope: "usage.hunyuanScope",
  },
  bailian: {
    name: "kind.bailian.name",
    desc: "kind.bailian.desc",
    icon: "ALI",
    accent: "#ff6a00",
    chip: "chip-accent",
    kindLabel: "providers.kindBailian",
    detailLabel: "detail.kindBailian",
    official: true,
    console: "https://bailian.console.aliyun.com/",
    url: { label: "form.apiUrl", hint: "form.bailianUrlHint", placeholder: "https://business.aliyuncs.com" },
    api: { label: "form.accessKey", hint: "form.bailianAkHint", placeholder: "LTAIxxxxxxxxxxxxxxxx" },
    akSk: true,
    billingUsage: true,
    plans: [
      { key: "bss", label: "plan.bss" },
      { key: "tokenPlanTeam", label: "plan.tokenPlanTeam" },
      { key: "tokenPlanPersonal", label: "plan.tokenPlanPersonal" },
      { key: "codingPlan", label: "plan.codingPlan" },
    ],
  },
  deepseek: {
    name: "kind.deepseek.name",
    desc: "kind.deepseek.desc",
    icon: "DS",
    accent: "#4d6bfe",
    chip: "chip-accent",
    kindLabel: "providers.kindDeepseek",
    detailLabel: "detail.kindDeepseek",
    official: true,
    console: "https://platform.deepseek.com/usage",
    url: { label: "form.apiUrl", hint: "form.apiUrlHint", placeholder: "https://api.deepseek.com" },
    api: { label: "form.apiKey", placeholder: "form.apiKeyPlaceholder", hint: "form.apiKeyHint" },
    login: {
      label: "form.platformToken",
      placeholder: "form.loginPlaceholder",
      hint: "form.platformTokenHint",
    },
  },
  newapi: {
    name: "kind.newapi.name",
    desc: "kind.newapi.desc",
    icon: "NA",
    accent: "#00d3a7",
    chip: "chip-teal",
    kindLabel: "providers.kindNewapi",
    detailLabel: "detail.kindNewapi",
    official: false,
    console: "",
    url: { label: "form.siteUrl", hint: "form.siteUrlHint", placeholder: "https://api.example.com" },
    api: {
      label: "form.accessToken",
      placeholder: "form.accessTokenPlaceholder",
      hint: "form.accessTokenHint",
    },
    currencyFields: true,
    credentialType: "system",
  },
  glm: {
    name: "kind.glm.name",
    desc: "kind.glm.desc",
    icon: "GLM",
    accent: "#3859ff",
    chip: "chip-accent",
    kindLabel: "providers.kindGlm",
    detailLabel: "detail.kindGlm",
    official: true,
    console: "https://open.bigmodel.cn/console/overview",
    url: { label: "form.apiUrl", hint: "form.glmUrlHint", placeholder: "https://open.bigmodel.cn" },
    api: { label: "form.apiKey", placeholder: "form.apiKeyPlaceholder", hint: "form.glmApiKeyHint" },
  },
  kimi: {
    name: "kind.kimi.name",
    desc: "kind.kimi.desc",
    icon: "K",
    accent: "#151515",
    chip: "chip-accent",
    kindLabel: "providers.kindKimi",
    detailLabel: "detail.kindKimi",
    official: true,
    console: "https://platform.kimi.com/console",
    balanceOnly: true,
    url: { label: "form.apiUrl", hint: "form.kimiUrlHint", placeholder: "https://api.moonshot.cn" },
    api: { label: "form.apiKey", placeholder: "form.apiKeyPlaceholder", hint: "form.kimiApiKeyHint" },
  },
  volcengine: {
    name: "kind.volcengine.name",
    desc: "kind.volcengine.desc",
    icon: "VE",
    accent: "#1664ff",
    chip: "chip-accent",
    kindLabel: "providers.kindVolcengine",
    detailLabel: "detail.kindVolcengine",
    official: true,
    console: "https://console.volcengine.com/ark",
    url: {
      label: "form.apiUrl",
      hint: "form.volcengineUrlHint",
      placeholder: "https://ark.cn-beijing.volcengineapi.com",
    },
    api: { label: "form.accessKey", hint: "form.volcengineAkHint" },
    akSk: true,
    independentUsageSources: true,
    usageScope: "usage.paygScope",
  },
};

const PLAN_INFO: Record<string, Partial<KindInfo>> = {
  tokenPlanTeam: {
    billingUsage: false, quotaOnly: true,
    planHint: "plan.teamHint",
    url: { label: "form.apiUrl", placeholder: "https://modelstudio.cn-beijing.aliyuncs.com", hint: "plan.teamHint" },
    api: { label: "form.accessKey", placeholder: "LTAIxxxxxxxxxxxxxxxx", hint: "plan.teamHint" },
    console: "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/overview",
  },
  tokenPlanPersonal: {
    billingUsage: false, quotaOnly: true, cliUsage: true, akSk: false,
    planHint: "plan.personalHint",
    url: { label: "form.apiUrl", placeholder: "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/overview", hint: "plan.configOnlyHint" },
    api: { label: "form.apiKey", placeholder: "form.apiKeyPlaceholder", hint: "plan.configOnlyHint" },
    console: "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/overview",
  },
  codingPlan: {
    billingUsage: false, quotaOnly: true, cliUsage: true, akSk: false,
    planHint: "plan.codingHint",
    url: { label: "form.apiUrl", placeholder: "https://bailian.console.aliyun.com/", hint: "plan.configOnlyHint" },
    api: { label: "form.apiKey", placeholder: "form.apiKeyPlaceholder", hint: "plan.configOnlyHint" },
    console: "https://bailian.console.aliyun.com/",
  },
};

export function kindInfo(kind: string): KindInfo {
  return KIND_INFO[kind as ProviderKind] || KIND_INFO.deepseek;
}

export function providerInfo(provider: { kind: string; planType?: ProviderConfig["planType"] }): KindInfo {
  const info = kindInfo(provider.kind);
  return info.plans && provider.planType ? { ...info, ...PLAN_INFO[provider.planType] } : info;
}

export const KIND_ORDER: ProviderKind[] = ["deepseek", "newapi", "glm", "volcengine", "kimi", "bailian", "hunyuan"];

export function usageValue(snap: UsageSnapshot | null, value: number | null | undefined): number | null {
  return snap && kindInfo(snap.kind).independentUsageSources && (value == null || value < 0) ? null : value ?? null;
}

export function billingTokens(snap: UsageSnapshot | null): number | null {
  const value = snap?.metrics.find((m) => /^(当月计费\s*Token|Monthly billed tokens)$/i.test(m.label.trim()))?.value.trim();
  if (!value) return null;
  const parsed = Number(value.replace(/,/g, ""));
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

/** 一级分类「接入方式」，互斥 */
export const MODES = [
  { key: "api", icon: "key" },
  { key: "login", icon: "logout" },
] as const;

export type AccessModeKey = "api" | "login";

/** 每个渠道实际支持的接入方式 */
export const MODES_BY_KIND: Record<ProviderKind, AccessModeKey[]> = {
  deepseek: ["api", "login"],
  newapi: ["api"],
  glm: ["api"],
  volcengine: ["api"],
  kimi: ["api"],
  bailian: ["api"],
  hunyuan: ["api"],
};

interface ModeInfo {
  label: string;
  desc: string;
  gets: string;
}

/** 文案统一走 i18n key，取值时再按当前语言翻译 */
export const MODE_INFO: Record<string, Partial<Record<AccessModeKey, ModeInfo>>> = {
  hunyuan: {
    api: { label: "mode.hunyuan.api.label", desc: "mode.hunyuan.api.desc", gets: "mode.hunyuan.api.gets" },
  },
  bailian: {
    api: { label: "mode.bailian.api.label", desc: "mode.bailian.api.desc", gets: "mode.bailian.api.gets" },
  },
  deepseek: {
    api: { label: "mode.api.label", desc: "mode.api.desc", gets: "mode.api.gets" },
    login: { label: "mode.login.label", desc: "mode.login.desc", gets: "mode.login.gets" },
  },
  newapi: {
    api: { label: "mode.newapi.api.label", desc: "mode.newapi.api.desc", gets: "mode.newapi.api.gets" },
  },
  glm: {
    api: { label: "mode.api.label", desc: "mode.glm.api.desc", gets: "mode.glm.api.gets" },
  },
  kimi: {
    api: { label: "mode.api.label", desc: "mode.kimi.api.desc", gets: "mode.kimi.api.gets" },
  },
  volcengine: {
    api: {
      label: "mode.volcengine.api.label",
      desc: "mode.volcengine.api.desc",
      gets: "mode.volcengine.api.gets",
    },
  },
};

export const FILTERS = [
  { key: "all", label: "filter.all" },
  { key: "deepseek", label: "filter.deepseek" },
  { key: "newapi", label: "filter.newapi" },
  { key: "glm", label: "filter.glm" },
  { key: "volcengine", label: "filter.volcengine" },
  { key: "kimi", label: "filter.kimi" },
  { key: "bailian", label: "filter.bailian" },
  { key: "hunyuan", label: "filter.hunyuan" },
  { key: "error", label: "filter.error" },
  { key: "off", label: "filter.off" },
];

/** 可按键筛选的渠道类型（其余 key 是状态筛选） */
export const KIND_FILTERS = KIND_ORDER;
