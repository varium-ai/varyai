import type { AppId } from "./types";
import { invoke } from "@tauri-apps/api/core";

/**
 * Profile scopes mirror ProfileScope in services/profile.rs.
 * Profiles are shared, while snapshots, application and current markers are scoped.
 * Claude Code and Claude Desktop switch providers independently and have separate scopes.
 */
export type ProfileScope = "claude" | "claude-desktop" | "codex";

/** Per-client payload slots; mirrors PerApp<T> in services/profile.rs. */
export interface PerApp<T> {
  claude: T;
  "claude-desktop": T;
  codex: T;
}

/**
 * Configuration snapshot; mirrors ProfilePayload.
 * A null slot was never captured and leaves configuration unchanged on application.
 * An empty array is a captured empty set and clears the corresponding enabled entries.
 */
export interface ProfilePayload {
  bundle?: {
    selections: {
      app: AppId;
      providerId: string;
      models: Record<string, unknown>;
      providers?: { id: string; models: Record<string, unknown> }[];
      nativeModels?: unknown;
      library?: ProfileLibrary;
    }[];
  };
  providers: PerApp<string | null>;
  mcp: PerApp<string[] | null>;
  skills: PerApp<string[] | null>;
  prompts: PerApp<string | null>;
}

export interface ProfileLibrary {
  mcp: string[] | null;
  skills: string[] | null;
  prompts: string[] | null;
  instructions: string | null;
  templates?: { slug: string; content: string; revision: string }[] | null;
}
export interface ProfileLibraryPreview {
  mcp: string[] | null;
  skills: string[] | null;
  prompts: string[] | null;
  instructions: boolean;
  templates: string[];
}
export interface ProfileAgent {
  app: AppId;
  provider: boolean;
  mcp: boolean;
  skills: boolean;
  instructions: boolean;
  available: boolean;
  reason: string | null;
}
export interface ProfilePreview {
  revision: string;
  changes: {
    app: string;
    currentProvider: string;
    targetProvider: string;
    models: string[];
    enabledProviders?: string[];
    library?: ProfileLibraryPreview | null;
    currentLibrary?: ProfileLibraryPreview | null;
    currentModels?: string[];
    clientSyncPending?: boolean;
  }[];
}

export interface ProfileApplyResult {
  success: boolean;
  undoId: string | null;
  warnings: string[];
  failure?: { app: string; reason: string } | null;
}

export interface Profile {
  id: string;
  name: string;
  payload: ProfilePayload;
  createdAt?: number;
  updatedAt?: number;
}

/**
 * Active profile ID per scope, or null when no profile is selected.
 * Response keys use camelCase (claudeDesktop); command scope arguments use kebab-case.
 */
export interface CurrentProfileIds {
  claude: string | null;
  claudeDesktop: string | null;
  codex: string | null;
  byApp?: Partial<Record<AppId, string | null>>;
}

export interface ProfileUndo {
  id: string;
  profileId: string;
  name: string;
  apps: string[];
}
export interface ProfilesResponse {
  profiles: Profile[];
  currentIds: CurrentProfileIds;
  currentBundleId?: string | null;
  agents?: ProfileAgent[];
  undo?: ProfileUndo | null;
}

export const profilesApi = {
  captureBundle: (name: string, apps: AppId[], id?: string): Promise<Profile> =>
    invoke("capture_profile_bundle", { name, apps, id }),
  previewBundle: (id: string): Promise<ProfilePreview> =>
    invoke("preview_profile_bundle", { id }),
  applyBundle: (id: string, revision: string): Promise<ProfileApplyResult> =>
    invoke("apply_profile_bundle", { id, revision }),
  undoBundle: (undoId: string): Promise<void> =>
    invoke("undo_profile_bundle", { undoId }),
  /** List shared profiles and the active profile ID for each scope. */
  async list(): Promise<ProfilesResponse> {
    return await invoke("list_profiles");
  },

  /** Create a profile from the initiating scope, leaving other scope slots uncaptured. */
  async create(name: string, scope: ProfileScope): Promise<Profile> {
    return await invoke("create_profile", { name, scope });
  },

  /**
   * Rename the shared profile and/or refresh the current scope snapshot.
   * Refreshing preserves snapshots in every other scope.
   */
  async update(
    id: string,
    options: { name?: string; resnapshot?: boolean; scope?: ProfileScope },
  ): Promise<Profile> {
    return await invoke("update_profile", {
      id,
      name: options.name,
      resnapshot: options.resnapshot,
      scope: options.scope,
    });
  },

  /** Delete a shared profile. */
  async delete(id: string): Promise<void> {
    return await invoke("delete_profile", { id });
  },

  /**
   * Apply the initiating scope snapshot and return best-effort warnings.
   * Partial failures do not stop the remaining configuration operations.
   */
  async apply(id: string, scope: ProfileScope): Promise<string[]> {
    return await invoke("apply_profile", { id, scope });
  },

  /** Clear the active marker for a scope without changing client configuration. */
  async clearCurrent(scope: AppId | "bundle"): Promise<void> {
    return await invoke("clear_current_profile", { scope });
  },
};
