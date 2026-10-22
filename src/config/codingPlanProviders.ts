/**
 * Coding-plan endpoint matching shared by provider creation and quota detection.
 * Keep matching aligned with services/coding_plan.rs::detect_provider.
 */
import { createUsageScript } from "@/types";
import { TEMPLATE_TYPES } from "@/config/constants";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";

export interface CodingPlanProviderEntry {
  /** 与后端 QuotaTier 的 `codingPlanProvider` 取值对齐 */
  id:
    | "kimi"
    | "zhipu"
    | "zhipu_team"
    | "minimax"
    | "zenmux"
    | "volcengine"
    | "opencode_go";
  /** base_url 匹配规则 */
  pattern: RegExp;
}

export const CODING_PLAN_PROVIDERS: readonly CodingPlanProviderEntry[] = [
  { id: "kimi", pattern: /api\.kimi\.com\/coding/i },
  {
    id: "zhipu",
    pattern: /bigmodel\.cn|api\.z\.ai/i,
  },
  {
    // Team plans share the personal endpoint. Keep personal matching first;
    // existing explicit zhipu_team configurations are handled by the backend.
    id: "zhipu_team",
    pattern: /bigmodel\.cn/i,
  },
  {
    id: "minimax",
    pattern: /api\.minimaxi?\.com|api\.minimax\.io/i,
  },
  {
    id: "zenmux",
    pattern: /zenmux\./i,
  },
  {
    // 火山方舟 Agent Plan / Coding Plan。base_url 形如
    // ark.cn-beijing.volces.com/api/plan[/v3]（Agent Plan）或
    // /api/coding[/v3]（Coding Plan）；与后端 detect_provider 的
    // `volces.com/api/plan` / `volces.com/api/coding` 子串判断同效。
    id: "volcengine",
    pattern: /volces\.com\/api\/(plan|coding)/i,
  },
  {
    // OpenCode Go（$10/月订阅，三时间窗口美元额度）。用量端点
    // GET /zen/go/v1/usage 是官方第一方但未文档化的路由，只认
    // Authorization: Bearer（与推理侧 /messages 只认 x-api-key 相反）。
    // base 分两档：/zen/go（claude/claude-desktop 直连 /messages）与
    // /zen/go/v1（codex/opencode/pi 走 Chat），子串同时覆盖；
    // Zen 按量版（/zen/v1）没有用量 API，刻意不命中。
    id: "opencode_go",
    pattern: /opencode\.ai\/zen\/go/i,
  },
] as const;

/** 根据 Base URL 自动检测 Coding Plan 供应商；未命中返回 null */
export function detectCodingPlanProvider(
  baseUrl: string | undefined | null,
): CodingPlanProviderEntry["id"] | null {
  if (!baseUrl) return null;
  for (const cp of CODING_PLAN_PROVIDERS) {
    if (cp.pattern.test(baseUrl)) return cp.id;
  }
  return null;
}

/**
 * 按 app 从 settingsConfig 里取出 base_url，供自动注入检测用。
 * 提取路径与后端 `Provider::resolve_usage_credentials` 的各 app 分支对齐
 * （token_plan 查询最终用的就是那份凭据，两边不一致会注入了却查不到）。
 */
export function extractBaseUrlForUsageDetection(
  appId: string,
  settingsConfig: Record<string, any> | undefined,
): string | null {
  if (!settingsConfig) return null;
  let raw: unknown;
  switch (appId) {
    case "claude":
    case "claude-desktop":
      raw = settingsConfig.env?.ANTHROPIC_BASE_URL;
      break;
    case "codex":
      raw = extractCodexBaseUrl(
        typeof settingsConfig.config === "string"
          ? settingsConfig.config
          : null,
      );
      break;
    case "opencode":
      raw = settingsConfig.options?.baseURL;
      break;
    case "pi":
      raw = settingsConfig.baseUrl;
      break;
    default:
      return null;
  }
  return typeof raw === "string" ? raw : null;
}

/**
 * Add a native token-plan usage configuration only when none exists.
 * Claude retains detection for all recognized plans; other clients only receive
 * automatic OpenCode Go configuration. Empty script code selects the native
 * coding_plan quota path instead of executing JavaScript.
 */
export function injectCodingPlanUsageScript<
  T extends {
    settingsConfig?: Record<string, any>;
    meta?: Record<string, any>;
  },
>(appId: string, provider: T): T {
  if (provider.meta?.usage_script) return provider;

  const baseUrl = extractBaseUrlForUsageDetection(
    appId,
    provider.settingsConfig,
  );
  const codingPlanProvider = detectCodingPlanProvider(baseUrl);
  if (!codingPlanProvider) return provider;
  if (appId !== "claude" && codingPlanProvider !== "opencode_go") {
    return provider;
  }

  return {
    ...provider,
    meta: {
      ...(provider.meta ?? {}),
      usage_script: createUsageScript({
        enabled: true,
        templateType: TEMPLATE_TYPES.TOKEN_PLAN,
        codingPlanProvider,
      }),
    },
  };
}
