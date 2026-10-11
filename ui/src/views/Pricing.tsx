/* TokenSonar · 价格视图（React 版）
 *
 * 由旧 ui/assets/js/views/pricing.js 1:1 平移：汇总国内外主流大模型「官方 API 直连」定价，
 * 含缓存命中 / 未命中、闲时 / 忙时，以及按上下文长度分档的阶梯价。
 *
 * 价格单位统一为「每百万 Token」，currency 决定货币符号（CNY=¥ / USD=$）。
 * 一行数据 = 一个模型的一个档位（时段档 或 上下文阶梯档）：
 *   tier  档位标签，如「标准」「高峰」「空闲」「≤32K」
 *   input 输入·未命中（缓存未命中）
 *   hit   输入·命中（缓存命中）
 *   write / write1h  缓存写入（5 分钟 / 1 小时 TTL，部分厂商只提供一种）
 *   output 输出
 * 缺省或 null 表示官方未单独公布，表格显示「—」。
 */
import { useState, type ReactNode } from "react";
import { Icon } from "@/lib/icons";
import { t, useLocale } from "@/lib/i18n";
import { openExternal } from "@/lib/api";
import { useStore } from "@/state/store";

/* ------------------------------ 类型定义 ------------------------------ */

/** 区域筛选键 */
type RegionKey = "all" | "cn" | "intl";

/** 行数据：一个模型的一个档位 */
interface PriceRow {
  tier?: string;
  input?: number | null;
  hit?: number | null;
  write?: number | null;
  write1h?: number | null;
  output?: number | null;
  note?: string;
}

/** 模型数据：一个模型下的若干档位 */
interface PriceModel {
  name: string;
  context?: string;
  rows: PriceRow[];
}

/** 厂商数据：一个厂商 = 一张卡片 */
interface Vendor {
  id: string;
  name: string;
  alias?: string;
  region: "cn" | "intl";
  icon: string;
  accent: string;
  currency: "CNY" | "USD";
  url: string;
  note: string;
  models: PriceModel[];
}

/** 命中的厂商及其（过滤后的）模型列表 */
interface MatchedVendor {
  p: Vendor;
  models: PriceModel[];
}

/* ------------------------------ 常量表 ------------------------------- */

const REGIONS: { key: RegionKey; label: string }[] = [
  { key: "all", label: "pricing.region.all" },
  { key: "cn", label: "pricing.region.cn" },
  { key: "intl", label: "pricing.region.intl" },
];

const UPDATE_DATE = "2026-09-28";

/* 价格数据本身是语言中立的（模型名 / 数字 / 币种），只有少量文案需要翻译。
 * 为避免在上百条数据里逐条塞英文字段，这里按「重复短文案走词表、厂商说明按 id 归组」
 * 的方式补充英文，渲染时按当前语言取值。 */
const CTX_EN: Record<string, string> = {
  "V4.1-Flash · 1M 上下文": "V4.1-Flash · 1M context",
  "128K 上下文": "128K context",
  "1M 上下文": "1M context",
  "256K 上下文": "256K context",
  "500K 上下文": "500K context",
  "≤256K 上下文": "≤256K context",
  "≤270K 上下文": "≤270K context",
  "256K 上下文 · 高速版": "256K context · High-speed",
  "1M 上下文 · 轻量": "1M context · Light",
  "≤270K 上下文 · 轻量": "≤270K context · Light",
  "按输入长度阶梯": "Tiered by input length",
  "预览版 · 按输入长度阶梯": "Preview · Tiered by input length",
  "预发布版": "Preview",
  "深度思考": "Deep thinking",
  "通用对话": "General chat",
  "推理模型": "Reasoning model",
  "极速版": "Turbo",
  "轻量 MoE": "Light MoE",
  "标准版": "Standard",
  "高速版": "High-speed",
  "旗舰": "Flagship",
  "高性能": "High performance",
  "均衡": "Balanced",
  "轻量": "Light",
  "上一代": "Previous gen",
};

const TIER_EN: Record<string, string> = {
  "标准": "Standard",
  "高峰": "Peak",
  "空闲": "Off-peak",
};

const ROW_NOTE_EN: Record<string, string> = {
  "2027-01-01 起调整为 1.5 / 0.15 / 7.5": "Adjusts to 1.5 / 0.15 / 7.5 from 2027-01-01",
};

const VENDOR_EN: Record<string, { name: string; note: string }> = {
  deepseek: {
    name: "DeepSeek Official",
    note:
      "Peak hours are Mon–Fri 09:00–12:00 and 14:00–18:00 Beijing time (excluding public holidays); all other hours are off-peak, at roughly half the peak price.",
  },
  aliyun: {
    name: "Alibaba Cloud Bailian · Qwen",
    note:
      "Cache hits are billed at 10% of the input price; explicitly created caches at 125%; batch calls are half price.",
  },
  volcengine: {
    name: "Volcengine · Doubao",
    note:
      "Cache hits cost about 20% of the input price; cache storage is billed separately at ¥0.017 per 1M tokens per hour.",
  },
  moonshot: {
    name: "Moonshot AI · Kimi",
    note:
      "Cache hits cost 1/10 of the input (miss) price; cache writes are tiered by TTL (kimi-k3 supports 5-minute and 1-hour tiers); batch is 60% of the standard price.",
  },
  zhipu: {
    name: "Zhipu AI · GLM",
    note:
      "Cache hits are billed at a fraction of the input price; cache storage is free for a limited time.",
  },
  hunyuan: {
    name: "Tencent Hunyuan",
    note: "No separate cache-hit price is published, shown as “—” in the table.",
  },
  minimax: {
    name: "MiniMax",
    note: "Cache hits cost 10%–20% of the input price; cache writes are billed separately.",
  },
  mimo: {
    name: "Xiaomi · MiMo",
    note:
      "The overseas API is priced in USD; cache hits cost about 0.8% of the input price.",
  },
  openai: {
    name: "OpenAI",
    note: "Cache hits cost 10% of the input price; batch calls are 50% off.",
  },
  anthropic: {
    name: "Anthropic · Claude",
    note:
      "Cache hits cost 0.1x the input price (0.025x for Fable 5.1, 0.05x for Opus 5.5); cache writes are 1.25x for a 5-minute TTL and 2x for a 1-hour TTL.",
  },
  google: {
    name: "Google · Gemini",
    note: "Cache storage is billed separately at $0.50 per 1M tokens per hour.",
  },
  xai: {
    name: "xAI · Grok",
    note:
      "Tiered by the input length of each request; long-context pricing doubles.",
  },
};

/* ------------------------------ 数据表 ------------------------------- */

const DATA: Vendor[] = [
  /* =============================== 国 内 =============================== */
  {
    id: "deepseek",
    name: "DeepSeek 官方",
    alias: "深度求索 deepseek",
    region: "cn",
    icon: "DS",
    accent: "#4d6bfe",
    currency: "CNY",
    url: "https://api-docs.deepseek.com/zh-cn/quick_start/pricing",
    note:
      "高峰时段为北京时间周一至周五 09:00–12:00、14:00–18:00（不含法定节假日），其余时段为空闲，空闲价约为高峰半价。",
    models: [
      {
        name: "deepseek-flash",
        context: "V4.1-Flash · 1M 上下文",
        rows: [
          { tier: "高峰", input: 2, hit: 0.04, output: 8 },
          { tier: "空闲", input: 1, hit: 0.02, output: 4 },
        ],
      },
      {
        name: "deepseek-v4-pro",
        context: "128K 上下文",
        rows: [
          { tier: "高峰", input: 9, hit: 0.3, output: 27 },
          { tier: "空闲", input: 4.5, hit: 0.15, output: 13.5 },
        ],
      },
    ],
  },
  {
    id: "aliyun",
    name: "阿里云百炼 · 通义千问",
    alias: "阿里 aliyun dashscope qwen 通义",
    region: "cn",
    icon: "QW",
    accent: "#ff6a00",
    currency: "CNY",
    url: "https://help.aliyun.com/zh/model-studio/models",
    note:
      "缓存命中按输入价 10% 计费，显式创建缓存按输入价 125% 计费；Batch 调用半价。",
    models: [
      {
        name: "qwen3.8-max",
        context: "≤256K 上下文",
        rows: [{ tier: "标准", input: 12, hit: 1.2, output: 36 }],
      },
      {
        name: "qwen3.8-max-prime",
        context: "≤256K 上下文",
        rows: [{ tier: "标准", input: 24, hit: 2.4, output: 72 }],
      },
      {
        name: "qwen3.6-max-preview",
        context: "预发布版",
        rows: [
          { tier: "≤128K", input: 9, hit: 0.9, output: 54 },
          { tier: "128K–256K", input: 15, hit: 1.5, output: 90 },
        ],
      },
      {
        name: "qwen3-max",
        context: "按输入长度阶梯",
        rows: [
          { tier: "≤32K", input: 2.5, hit: 0.25, output: 10 },
          { tier: "32K–128K", input: 4, hit: 0.4, output: 16 },
          { tier: "128K–256K", input: 7, hit: 0.7, output: 28 },
        ],
      },
      {
        name: "qwen3.6-flash",
        context: "按输入长度阶梯",
        rows: [
          { tier: "≤256K", input: 1.2, hit: 0.12, write: 1.5, output: 7.2 },
          { tier: "256K–1M", input: 4.8, hit: 0.48, write: 6, output: 28.8 },
        ],
      },
    ],
  },
  {
    id: "volcengine",
    name: "火山引擎 · 豆包",
    alias: "字节 bytedance volcengine doubao 豆包",
    region: "cn",
    icon: "DB",
    accent: "#1664ff",
    currency: "CNY",
    url: "https://www.volcengine.com/docs/82379/1544106",
    note:
      "缓存命中价约为输入价的 20%；缓存存储另计 0.017 元 / 百万 token / 小时。",
    models: [
      {
        name: "Doubao-Seed-Evolving",
        context: "1M 上下文",
        rows: [{ tier: "标准", input: 6, hit: 1.2, output: 30 }],
      },
      {
        name: "Doubao-Seed-2.1-pro",
        context: "256K 上下文",
        rows: [{ tier: "标准", input: 6, hit: 1.2, output: 30 }],
      },
      {
        name: "Doubao-Seed-2.1-turbo",
        context: "256K 上下文",
        rows: [{ tier: "标准", input: 3, hit: 0.6, output: 15 }],
      },
      {
        name: "Doubao-Seed-2.1-lite",
        context: "256K 上下文",
        rows: [{ tier: "标准", input: 0.8, hit: 0.16, output: 2.7 }],
      },
      {
        name: "Doubao-Seed-2.0-pro",
        context: "按输入长度阶梯",
        rows: [
          { tier: "≤32K", input: 3.2, hit: 0.64, output: 16 },
          { tier: "32K–128K", input: 4.8, hit: 0.96, output: 24 },
          { tier: "128K–256K", input: 9.6, hit: 1.92, output: 48 },
        ],
      },
    ],
  },
  {
    id: "moonshot",
    name: "月之暗面 · Kimi",
    alias: "moonshot kimi 月之暗面",
    region: "cn",
    icon: "KM",
    accent: "#0b0b0f",
    currency: "CNY",
    url: "https://platform.moonshot.cn/docs/pricing/chat",
    note:
      "缓存命中价为未命中输入价的 1/10；缓存写入按 TTL 分档（kimi-k3 支持 5 分钟 / 1 小时两档）；Batch 为标准价 60%。",
    models: [
      {
        name: "kimi-k3",
        context: "1M 上下文",
        rows: [{ tier: "标准", input: 20, hit: 2, write: 20, write1h: 40, output: 100 }],
      },
      {
        name: "kimi-k2.7-code",
        context: "256K 上下文",
        rows: [{ tier: "标准", input: 6.5, hit: 1.3, output: 27 }],
      },
      {
        name: "kimi-k2.7-code-highspeed",
        context: "256K 上下文 · 高速版",
        rows: [{ tier: "标准", input: 13, hit: 2.6, output: 54 }],
      },
      {
        name: "kimi-k2.6",
        context: "256K 上下文",
        rows: [{ tier: "标准", input: 6.5, hit: 1.1, output: 27 }],
      },
    ],
  },
  {
    id: "zhipu",
    name: "智谱 AI · GLM",
    alias: "zhipu bigmodel glm 智谱 清言",
    region: "cn",
    icon: "GLM",
    accent: "#3859ff",
    currency: "CNY",
    url: "https://open.bigmodel.cn/pricing",
    note: "缓存命中价按输入价的一定比例计费，缓存存储当前限时免费。",
    models: [
      {
        name: "GLM-5.3",
        context: "1M 上下文",
        rows: [{ tier: "标准", input: 8, hit: 2, output: 28 }],
      },
      {
        name: "GLM-5.3-Flash",
        context: "1M 上下文 · 轻量",
        rows: [{ tier: "标准", input: 0.8, hit: 0.23, output: 2.8 }],
      },
      {
        name: "GLM-5.2",
        context: "1M 上下文",
        rows: [{ tier: "标准", input: 8, hit: 2, output: 28 }],
      },
      {
        name: "GLM-5.1",
        context: "按输入长度阶梯",
        rows: [
          { tier: "≤32K", input: 6, hit: 1.3, output: 24 },
          { tier: "≥32K", input: 8, hit: 1.3, output: 28 },
        ],
      },
    ],
  },
  {
    id: "hunyuan",
    name: "腾讯混元",
    alias: "tencent hunyuan 腾讯 混元",
    region: "cn",
    icon: "HY",
    accent: "#0052d9",
    currency: "CNY",
    url: "https://cloud.tencent.com/document/product/1729/97731",
    note: "官方未单独公布缓存命中价，表格以「—」表示。",
    models: [
      {
        name: "Hunyuan HY 2.0 Think",
        context: "深度思考",
        rows: [
          { tier: "≤32K", input: 3.975, output: 15.9 },
          { tier: "32K–128K", input: 5.3, output: 21.2 },
        ],
      },
      {
        name: "Hunyuan HY 2.0 Instruct",
        context: "通用对话",
        rows: [
          { tier: "≤32K", input: 3.18, output: 7.95 },
          { tier: "32K–128K", input: 4.505, output: 11.13 },
        ],
      },
      {
        name: "Hunyuan-T1",
        context: "推理模型",
        rows: [{ tier: "标准", input: 1, output: 4 }],
      },
      {
        name: "Hunyuan-TurboS",
        context: "极速版",
        rows: [{ tier: "标准", input: 0.8, output: 2 }],
      },
      {
        name: "Hunyuan-a13b",
        context: "轻量 MoE",
        rows: [{ tier: "标准", input: 0.5, output: 2 }],
      },
    ],
  },
  {
    id: "minimax",
    name: "MiniMax",
    alias: "minimax 稀宇 abab",
    region: "cn",
    icon: "MM",
    accent: "#e0426b",
    currency: "CNY",
    url: "https://platform.minimaxi.com/document/price",
    note: "缓存命中价为输入价的 10%–20%，缓存写入单独计费。",
    models: [
      {
        name: "MiniMax-M2.7",
        context: "标准版",
        rows: [{ tier: "标准", input: 2.1, hit: 0.42, write: 2.625, output: 8.4 }],
      },
      {
        name: "MiniMax-M2.7-highspeed",
        context: "高速版",
        rows: [{ tier: "标准", input: 4.2, hit: 0.42, output: 16.8 }],
      },
      {
        name: "MiniMax-M2.5",
        context: "标准版",
        rows: [{ tier: "标准", input: 2.1, hit: 0.21, output: 8.4 }],
      },
      {
        name: "MiniMax-M2.5-highspeed",
        context: "高速版",
        rows: [{ tier: "标准", input: 4.2, hit: 0.21, output: 16.8 }],
      },
    ],
  },
  {
    id: "mimo",
    name: "小米 · MiMo",
    alias: "xiaomi mimo 小米",
    region: "cn",
    icon: "Mi",
    accent: "#ff6900",
    currency: "USD",
    url: "https://platform.mimo.ai/docs/pricing",
    note: "海外 API 以美元计价；缓存命中价约为输入价的 0.8%。",
    models: [
      {
        name: "MiMo-V2.6-Pro",
        context: "1M 上下文",
        rows: [{ tier: "标准", input: 0.435, hit: 0.0036, output: 0.87 }],
      },
    ],
  },

  /* =============================== 海 外 =============================== */
  {
    id: "openai",
    name: "OpenAI",
    alias: "openai gpt chatgpt",
    region: "intl",
    icon: "OA",
    accent: "#10a37f",
    currency: "USD",
    url: "https://openai.com/api/pricing/",
    note: "缓存命中价为输入价 10%；Batch 调用 5 折。",
    models: [
      {
        name: "GPT-5.6 Sol",
        context: "≤270K 上下文",
        rows: [{ tier: "标准", input: 5, hit: 0.5, output: 30 }],
      },
      {
        name: "GPT-5.6 Terra",
        context: "≤270K 上下文",
        rows: [{ tier: "标准", input: 2, hit: 0.2, output: 12 }],
      },
      {
        name: "GPT-5.6 Luna",
        context: "≤270K 上下文 · 轻量",
        rows: [{ tier: "标准", input: 0.2, hit: 0.02, output: 1.2 }],
      },
      {
        name: "GPT-5.5",
        context: "≤270K 上下文",
        rows: [{ tier: "标准", input: 5, hit: 0.5, output: 30 }],
      },
    ],
  },
  {
    id: "anthropic",
    name: "Anthropic · Claude",
    alias: "anthropic claude 克劳德",
    region: "intl",
    icon: "AN",
    accent: "#d97757",
    currency: "USD",
    url: "https://www.anthropic.com/pricing#api",
    note:
      "缓存命中价为输入价 0.1x（Fable 5.1 为 0.025x、Opus 5.5 为 0.05x）；缓存写入 5 分钟 TTL 为 1.25x、1 小时 TTL 为 2x。",
    models: [
      {
        name: "Claude Fable 5.1",
        context: "旗舰",
        rows: [{ tier: "标准", input: 10, hit: 0.25, write: 12.5, write1h: 20, output: 50 }],
      },
      {
        name: "Claude Opus 5.5",
        context: "高性能",
        rows: [{ tier: "标准", input: 4, hit: 0.2, write: 5, write1h: 8, output: 20 }],
      },
      {
        name: "Claude Sonnet 5",
        context: "均衡",
        rows: [{ tier: "标准", input: 2, hit: 0.2, write: 2.5, write1h: 4, output: 10 }],
      },
      {
        name: "Claude Haiku 4.5",
        context: "轻量",
        rows: [{ tier: "标准", input: 1, hit: 0.1, write: 1.25, write1h: 2, output: 5 }],
      },
      {
        name: "Claude Opus 4.5",
        context: "上一代",
        rows: [{ tier: "标准", input: 5, hit: 0.5, write: 6.25, write1h: 10, output: 25 }],
      },
      {
        name: "Claude Sonnet 4.5",
        context: "上一代",
        rows: [{ tier: "标准", input: 3, hit: 0.3, write: 3.75, write1h: 6, output: 15 }],
      },
    ],
  },
  {
    id: "google",
    name: "Google · Gemini",
    alias: "google gemini 谷歌 双子星",
    region: "intl",
    icon: "GM",
    accent: "#4285f4",
    currency: "USD",
    url: "https://ai.google.dev/gemini-api/docs/pricing",
    note: "缓存存储另计 $0.50 / 百万 token / 小时。",
    models: [
      {
        name: "Gemini 3.8 Flash",
        context: "1M 上下文",
        rows: [
          {
            tier: "标准",
            input: 0.75,
            hit: 0.075,
            output: 3.75,
            note: "2027-01-01 起调整为 1.5 / 0.15 / 7.5",
          },
        ],
      },
      {
        name: "Gemini 3.1 Pro",
        context: "预览版 · 按输入长度阶梯",
        rows: [
          { tier: "≤200K", input: 2, hit: 0.2, output: 12 },
          { tier: ">200K", input: 4, hit: 0.4, output: 18 },
        ],
      },
    ],
  },
  {
    id: "xai",
    name: "xAI · Grok",
    alias: "xai grok 马斯克",
    region: "intl",
    icon: "xAI",
    accent: "#0f0f0f",
    currency: "USD",
    url: "https://docs.x.ai/docs/models",
    note: "按单次请求的输入长度分档，长上下文单价翻倍。",
    models: [
      {
        name: "grok-4.7",
        context: "500K 上下文",
        rows: [
          { tier: "<200K", input: 2, hit: 0.5, output: 6 },
          { tier: "≥200K", input: 4, hit: 1, output: 12 },
        ],
      },
      {
        name: "grok-4.6",
        context: "500K 上下文",
        rows: [
          { tier: "<200K", input: 2, hit: 0.5, output: 6 },
          { tier: "≥200K", input: 4, hit: 1, output: 12 },
        ],
      },
    ],
  },
];

/* ------------------------------ 文案取值 ------------------------------ */

/** 上下文说明按当前语言取值，缺省回退原文 */
function ctxOf(s: string, en: boolean): string {
  return en ? CTX_EN[s] || s : s;
}

/** 档位标签按当前语言取值，缺省回退原文 */
function tierOf(s: string, en: boolean): string {
  return en ? TIER_EN[s] || s : s;
}

/** 行备注按当前语言取值，缺省回退原文 */
function rowNote(s: string, en: boolean): string {
  return en ? ROW_NOTE_EN[s] || s : s;
}

/** 厂商名按当前语言取值，缺省回退原文 */
function vendorName(p: Vendor, en: boolean): string {
  return en && VENDOR_EN[p.id] ? VENDOR_EN[p.id].name : p.name;
}

/** 厂商说明按当前语言取值，缺省回退原文 */
function vendorNote(p: Vendor, en: boolean): string {
  return en && VENDOR_EN[p.id] ? VENDOR_EN[p.id].note : p.note;
}

/* ------------------------------ 格式化 ------------------------------- */

/** 货币符号：人民币 ¥，其余按美元 $ */
function sym(currency: string): string {
  return currency === "CNY" ? "¥" : "$";
}

/** 价格数字：整数不带小数，小数最多 4 位且去掉末尾 0 */
function num(v: number): string {
  const n = Number(v);
  if (!isFinite(n)) return "—";
  if (Number.isInteger(n)) return String(n);
  return String(parseFloat(n.toFixed(4)));
}

/** 价格单元格：null -> 「—」，否则带货币符号 */
function price(v: number | null | undefined, currency: string): ReactNode {
  if (v == null) return <span className="price-na">—</span>;
  return sym(currency) + num(v);
}

/** 缓存写入：可能同时有 5m / 1h 两档 */
function writeCell(row: PriceRow, currency: string): ReactNode {
  if (row.write == null && row.write1h == null) return <span className="price-na">—</span>;
  return (
    <>
      {price(row.write, currency)}
      {row.write1h != null ? (
        <>
          <span className="price-sep">/</span>
          {price(row.write1h, currency)}
        </>
      ) : null}
    </>
  );
}

/** 档位标签：高峰/忙时用暖色，空闲/闲时用青色 */
function tierChip(row: PriceRow, en: boolean): ReactNode {
  const raw = row.tier || "标准";
  let cls = "price-tier";
  if (/高峰|忙时|peak/i.test(raw)) cls += " is-peak";
  else if (/空闲|闲时|低谷|off/i.test(raw)) cls += " is-off";
  return (
    <>
      <span className={cls}>{tierOf(raw, en)}</span>
      {row.note ? <div className="price-row-note">{rowNote(row.note, en)}</div> : null}
    </>
  );
}

/* ------------------------------ 卡片渲染 ------------------------------ */

function PriceCard({ data, models, en }: { data: Vendor; models: PriceModel[]; en: boolean }) {
  const cur = data.currency;
  return (
    <div className="card price-card">
      <div className="price-head">
        <div className="price-icon" style={{ background: data.accent }}>
          {data.icon}
        </div>
        <div className="price-title">
          <div className="name">
            {vendorName(data, en)}
            <span className={"chip " + (data.region === "cn" ? "chip-accent" : "chip-teal")}>
              {t(data.region === "cn" ? "pricing.regionCnChip" : "pricing.regionIntlChip")}
            </span>
            <span className="chip">{cur === "CNY" ? "CNY" : "USD"}</span>
          </div>
        </div>
        <a
          className="price-link"
          href={data.url}
          target="_blank"
          rel="noreferrer"
          onClick={(e) => {
            e.preventDefault();
            void openExternal(data.url);
          }}
        >
          {t("pricing.official")}
        </a>
      </div>
      <div className="price-note">
        <Icon name="info" />
        <span>{vendorNote(data, en)}</span>
      </div>
      <div className="table-wrap price-table">
        <table className="data">
          <thead>
            <tr>
              <th>{t("pricing.th.model")}</th>
              <th>{t("pricing.th.tier")}</th>
              <th>
                {t("pricing.th.inputMiss")}
                <span className="price-sub">{t("pricing.th.inputMissSub")}</span>
              </th>
              <th>
                {t("pricing.th.inputHit")}
                <span className="price-sub">{t("pricing.th.inputHitSub")}</span>
              </th>
              <th>
                {t("pricing.th.cacheWrite")}
                <span className="price-sub">{t("pricing.th.cacheWriteSub")}</span>
              </th>
              <th>{t("pricing.th.output")}</th>
            </tr>
          </thead>
          <tbody>
            {models.map((m) =>
              m.rows.map((row, i) => (
                <tr key={m.name + "-" + i}>
                  {i === 0 ? (
                    <td className="model-cell" rowSpan={m.rows.length}>
                      <div className="price-model">{m.name}</div>
                      {m.context ? <div className="ctx">{ctxOf(m.context, en)}</div> : null}
                    </td>
                  ) : null}
                  <td className="tier-cell">{tierChip(row, en)}</td>
                  <td className="num">{price(row.input, cur)}</td>
                  <td className="num">{price(row.hit, cur)}</td>
                  <td className="num">{writeCell(row, cur)}</td>
                  <td className="num">{price(row.output, cur)}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/* ------------------------------ 视图主体 ------------------------------ */

export function Pricing() {
  const { state } = useStore();
  const locale = useLocale();
  const en = locale === "en";

  // 当前区域筛选（视图内部状态，不写入全局 state）
  const [region, setRegion] = useState<RegionKey>("all");

  const q = (state.search || "").trim().toLowerCase();

  const list: MatchedVendor[] = [];
  DATA.forEach((p) => {
    if (region !== "all" && p.region !== region) return;
    if (!q) {
      list.push({ p, models: p.models });
      return;
    }
    // 命中厂商名 / 别名时展示该厂商全部模型，否则按模型名过滤
    const nameHit = (p.name + " " + (p.alias || "")).toLowerCase().indexOf(q) >= 0;
    if (nameHit) {
      list.push({ p, models: p.models });
      return;
    }
    const models = p.models.filter(
      (m) => (m.name + " " + (m.context || "")).toLowerCase().indexOf(q) >= 0
    );
    if (models.length) list.push({ p, models });
  });

  const modelCount = list.reduce((n, x) => n + x.models.length, 0);

  const toolbar = (
    <div className="price-toolbar">
      <div className="filters">
        {REGIONS.map((r) => (
          <button
            key={r.key}
            className={"filter-pill" + (region === r.key ? " is-active" : "")}
            onClick={() => setRegion(r.key)}
          >
            {t(r.label)}
          </button>
        ))}
      </div>
      <div className="price-meta">{t("pricing.meta", { vendors: list.length, models: modelCount })}</div>
    </div>
  );

  if (!list.length) {
    return (
      <>
        {toolbar}
        <div className="card card-pad">
          <div className="empty">
            <div className="empty-illu">
              <Icon name="coins" />
            </div>
            <h3>{t("pricing.noMatchTitle")}</h3>
            <p>{t("pricing.noMatchBody")}</p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      {toolbar}
      <div className="price-caveat">
        <Icon name="info" />
        <span>{t("pricing.caveat", { date: UPDATE_DATE })}</span>
      </div>
      <div className="price-list">
        {list.map((x) => (
          <PriceCard key={x.p.id} data={x.p} models={x.models} en={en} />
        ))}
      </div>
    </>
  );
}
