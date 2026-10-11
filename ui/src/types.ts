/* TokenSonar · 与后端 models.rs 对齐的类型定义（serde 统一 camelCase） */

export type ProviderKind = "deepseek" | "newapi" | "glm" | "volcengine" | "kimi" | "bailian" | "hunyuan";
export type AuthType = "token" | "password";
export type AccessMode = "api" | "login";
export type Currency = string;

export type KeyType = "api" | "aksk" | "system";

export interface ApiKeyInfo {
  id: string;
  name: string;
  keyType?: KeyType | null;
  masked?: string | null;
  secret?: string | null;
  createdAt?: string | null;
  lastUsedAt?: string | null;
  status?: string | null;
  /** api = 平台接口读取；manual = 手动录入 */
  source: "api" | "manual";
}

export interface ProviderConfig {
  id: string;
  name: string;
  kind: ProviderKind;
  planType?: "bss" | "tokenPlanTeam" | "tokenPlanPersonal" | "codingPlan" | null;
  cliProfile?: string | null;
  enabled: boolean;
  baseUrl?: string | null;
  authType?: AuthType | null;
  accessMode?: AccessMode | null;
  token?: string | null;
  credentialName?: string | null;
  accessKeyId?: string | null;
  accessKeySecret?: string | null;
  sessionToken?: string | null;
  username?: string | null;
  password?: string | null;
  platformToken?: string | null;
  apiKeys?: ApiKeyInfo[] | null;
  icon?: string | null;
  accent?: string | null;
  currency: Currency;
  quotaPerUnit: number;
  displayInCurrency: boolean;
  notes?: string | null;
  /** metrics = 用量指标行；quota = 套餐额度行 */
  cardView?: "metrics" | "quota" | null;
  createdAt: string;
  updatedAt: string;
}

export interface PeriodUsage {
  requests: number;
  tokens: number;
  cost: number;
  from: string;
  to: string;
}

export interface DailyUsage {
  date: string;
  requests: number;
  tokens: number;
  cost: number;
}

export interface ModelUsage {
  model: string;
  requests: number;
  tokens: number;
  cost: number;
}

export interface RequestLog {
  id: string;
  time: string;
  model: string;
  tokens: number;
  cost: number;
  durationMs: number;
  status: string;
}

export interface Metric {
  label: string;
  value: string;
  hint?: string | null;
}

export interface QuotaWindow {
  label: string;
  used: number;
  quota: number;
  percent: number;
  resetAt?: string | null;
  resetIn?: string | null;
}

export interface UsageSnapshot {
  providerId: string;
  providerName: string;
  kind: string;
  currency: Currency;
  fetchedAt: string;
  ok: boolean;
  error?: string | null;
  account?: string | null;
  balance?: number | null;
  balanceRaw?: number | null;
  granted?: number | null;
  used?: number | null;
  requestsTotal?: number | null;
  tokensTotal?: number | null;
  today?: PeriodUsage | null;
  month?: PeriodUsage | null;
  days: DailyUsage[];
  models: ModelUsage[];
  quotas: QuotaWindow[];
  logs: RequestLog[];
  metrics: Metric[];
  apiKeys: ApiKeyInfo[];
  supportsUsageApi: boolean;
}

export interface LedgerEntry {
  id: string;
  providerId: string;
  date: string;
  model: string;
  promptTokens: number;
  completionTokens: number;
  requests: number;
  cost: number;
  note?: string | null;
  createdAt: string;
}

export interface AppSettings {
  /** dark / light / system */
  theme: string;
  /** zh / en；空串表示用户还没选过，前端弹首次启动引导页 */
  language: string;
  currency: Currency;
  autoRefresh: boolean;
  refreshIntervalMin: number;
  launchAtLoginHint: boolean;
  lastView: string;
  providersView: "card" | "list";
}

export interface AppInfo {
  name: string;
  version: string;
  dataDir: string;
}

export interface SyncConfig {
  repoUrl: string;
  username: string;
  password: string;
  branch: string;
  lastSyncAt: string;
  lastMessage: string;
}

export interface SyncResult {
  ok: boolean;
  message: string;
  commit?: string | null;
  changed: boolean;
  fileCount: number;
  syncedAt: string;
}

export interface RemoteBackup {
  providers: ProviderConfig[];
  settings?: AppSettings | null;
  branch: string;
  commit?: string | null;
}
