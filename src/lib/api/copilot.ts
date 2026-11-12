/** Copilot model and quota bindings; login uses the shared managed-auth API. */
import { invoke } from "@tauri-apps/api/core";

/**
 * GitHub 账号信息（公开信息）
 */
export interface GitHubAccount {
  /** GitHub 用户 ID（唯一标识） */
  id: string;
  /** GitHub 用户名 */
  login: string;
  /** 头像 URL */
  avatar_url: string | null;
  /** 认证时间戳（Unix 秒） */
  authenticated_at: number;
  /** GitHub 域名（github.com 或 GHES 域名） */
  github_domain: string;
}

/**
 * Copilot 可用模型
 */
export interface CopilotModel {
  id: string;
  name: string;
  vendor: string;
  model_picker_enabled: boolean;
  capabilities?: Record<string, unknown>;
  supported_endpoints?: string[];
  [key: string]: unknown;
}

/**
 * 获取 Copilot 可用模型列表
 *
 * @returns 可用模型列表
 */
export async function copilotGetModels(): Promise<CopilotModel[]> {
  return invoke<CopilotModel[]>("copilot_get_models");
}

/**
 * 配额详情
 */
export interface QuotaDetail {
  entitlement: number | null;
  remaining: number | null;
  percent_remaining: number | null;
  unlimited: boolean;
  has_quota?: boolean | null;
  overage_permitted?: boolean | null;
}

/**
 * 配额快照
 */
export interface QuotaSnapshots {
  chat?: QuotaDetail | null;
  completions?: QuotaDetail | null;
  premium_interactions?: QuotaDetail | null;
}

/**
 * Copilot 使用量响应
 */
export interface CopilotUsageResponse {
  copilot_plan: string;
  quota_reset_date: string;
  quota_snapshots: QuotaSnapshots;
}

/**
 * 获取 Copilot 使用量信息
 *
 * @returns 使用量信息，包含计划类型、重置日期和配额快照
 */
export async function copilotGetUsage(): Promise<CopilotUsageResponse> {
  return invoke<CopilotUsageResponse>("copilot_get_usage");
}

/**
 * 获取指定账号的 Copilot 可用模型列表
 *
 * @param accountId - GitHub 用户 ID
 * @returns 可用模型列表
 */
export async function copilotGetModelsForAccount(
  accountId: string,
): Promise<CopilotModel[]> {
  return invoke<CopilotModel[]>("copilot_get_models_for_account", {
    accountId,
  });
}

/**
 * 获取指定账号的 Copilot 使用量信息
 *
 * @param accountId - GitHub 用户 ID
 * @returns 使用量信息
 */
export async function copilotGetUsageForAccount(
  accountId: string,
): Promise<CopilotUsageResponse> {
  return invoke<CopilotUsageResponse>("copilot_get_usage_for_account", {
    accountId,
  });
}
