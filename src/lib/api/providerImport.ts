import { invoke } from "@tauri-apps/api/core";

export interface ProviderImportItem {
  reference: string;
  fingerprint: string;
  name: string;
  from?: string;
  host: string;
  modelCount: number;
  status: "new" | "same" | "vendor" | "taken";
  existing?: string | null;
  skip?: string | null;
  enabled: boolean;
  subscription?: boolean;
}
export interface ProviderImportSource {
  id: string;
  name: string;
  path: string;
  found: boolean;
  error?: string | null;
  items: ProviderImportItem[];
}
export interface ProviderImportPick {
  source: string;
  reference: string;
  fingerprint: string;
  mode: "add" | "key";
  name?: string;
}
export const providerImportApi = {
  preview: (sourcePath?: string) =>
    invoke<ProviderImportSource[]>("preview_provider_import", {
      magpiePath: sourcePath,
    }),
  import: (picks: ProviderImportPick[], sourcePath?: string) =>
    invoke<{ added: number; skipped: number }>("import_provider_sources", {
      picks,
      magpiePath: sourcePath,
    }),
};
