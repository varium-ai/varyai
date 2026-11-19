import { invoke } from "@tauri-apps/api/core";

export type TrayUsagePeriod = "today" | "week" | "month" | "all";

export interface TrayUsageTotals {
  requests: number;
  failed: number;
  input: number;
  output: number;
  reads: number;
  writes: number;
  cost: number;
  unpriced: number;
  firstMs: number;
  firstCount: number;
  timedOutput: number;
  generationMs: number;
  timedCount: number;
}

export interface TrayUsageRow extends TrayUsageTotals {
  bucket: string;
  providerId: string;
  providerName: string;
  model: string;
}

export const trayUsageKey = ["usage", "tray"] as const;

export const trayUsageApi = {
  read: (period: TrayUsagePeriod | "activity") =>
    invoke<TrayUsageRow[]>("get_tray_usage", { period }),
};
