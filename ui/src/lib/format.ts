/* TokenSonar · 格式化工具（数字 / 金额 / 时间） */
import { t } from "./i18n";

const SYMBOL: Record<string, string> = {
  CNY: "¥",
  USD: "$",
  EUR: "€",
  JPY: "¥",
  HKD: "HK$",
  POINT: "",
};

export const fmt = {
  symbol(currency?: string | null): string {
    if (currency != null && SYMBOL[currency] != null) return SYMBOL[currency];
    // 站点自定义货币时后端直接给符号（如 ¤ / ₩），原样使用；
    // 未收录的三字母代码（GBP 等）仍按「无符号」处理，避免把代码当符号显示
    if (typeof currency !== "string" || /^[A-Za-z]{3}$/.test(currency)) return "";
    return currency;
  },

  /** 紧凑数字：1.2k / 3.4M / 1.2B */
  compact(n?: number | null): string {
    if (n == null || Number.isNaN(n)) return "—";
    const v = Number(n);
    const abs = Math.abs(v);
    if (abs >= 1e9) return (v / 1e9).toFixed(2) + "B";
    if (abs >= 1e6) return (v / 1e6).toFixed(2) + "M";
    if (abs >= 1e3) return (v / 1e3).toFixed(1) + "k";
    if (abs >= 100) return v.toFixed(0);
    if (abs >= 10) return v.toFixed(1);
    if (abs === 0) return "0";
    return v.toFixed(2);
  },

  /** 整数（带千分位） */
  int(n?: number | null): string {
    if (n == null || Number.isNaN(n)) return "—";
    return Math.round(Number(n)).toLocaleString("en-US");
  },

  /** 金额：¥12.34，大额自动紧凑 */
  money(n?: number | null, currency?: string | null, opts?: { compact?: boolean }): string {
    if (n == null || Number.isNaN(n)) return "—";
    const o = opts || {};
    const sym = fmt.symbol(currency);
    const v = Number(n);
    const abs = Math.abs(v);
    let body: string;
    if (o.compact !== false && abs >= 10000) {
      body = fmt.compact(v);
    } else if (abs >= 100) {
      body = v.toFixed(1);
    } else {
      body = v.toFixed(2);
    }
    return sym + body;
  },

  /** Token 数：带单位提示的紧凑写法 */
  tokens(n?: number | null): string {
    if (n == null || Number.isNaN(n)) return "—";
    return fmt.compact(Number(n));
  },

  pct(n?: number | null): string {
    if (n == null || Number.isNaN(n)) return "—";
    return (Number(n) * 100).toFixed(1) + "%";
  },

  /** 秒/毫秒 -> 人类可读 */
  duration(ms?: number | null): string {
    if (!ms) return "—";
    const v = Number(ms);
    if (v < 1000) return Math.round(v) + "ms";
    if (v < 60000) return (v / 1000).toFixed(2) + "s";
    return Math.floor(v / 60000) + "m" + Math.round((v % 60000) / 1000) + "s";
  },

  /** "2026-09-28 12:30" -> "09-28 12:30" */
  shortTime(s?: string | null): string {
    if (!s) return "—";
    const str = String(s);
    if (/^\d+$/.test(str)) {
      const ts = str.length > 10 ? Number(str) : Number(str) * 1000;
      const d = new Date(ts);
      return (
        fmt.pad(d.getMonth() + 1) + "-" + fmt.pad(d.getDate()) + " " +
        fmt.pad(d.getHours()) + ":" + fmt.pad(d.getMinutes())
      );
    }
    const m = str.match(/(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})/);
    if (m) return m[2] + "-" + m[3] + " " + m[4] + ":" + m[5];
    return str.slice(0, 16);
  },

  /** yyyy-mm-dd -> mm/dd */
  shortDate(s?: string | null): string {
    if (!s) return "";
    const p = String(s).split("-");
    return p.length >= 3 ? p[1] + "/" + p[2] : s;
  },

  pad(n: number): string {
    return String(n).padStart(2, "0");
  },

  /** 相对时间（跟随当前语言） */
  ago(s?: string | null): string {
    if (!s) return t("time.never");
    const ts = Date.parse(String(s).replace(" ", "T"));
    if (Number.isNaN(ts)) return s;
    const diff = Math.floor((Date.now() - ts) / 1000);
    if (diff < 60) return t("time.secondsAgo", { n: diff });
    if (diff < 3600) return t("time.minutesAgo", { n: Math.floor(diff / 60) });
    if (diff < 86400) return t("time.hoursAgo", { n: Math.floor(diff / 3600) });
    if (diff < 86400 * 30) return t("time.daysAgo", { n: Math.floor(diff / 86400) });
    return String(s).slice(0, 10);
  },
};
