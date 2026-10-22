import type { AppId } from "@/lib/api/types";
import type { PerApp, Profile, ProfileScope } from "@/lib/api/profiles";

/**
 * Frontend mirror of ProfileScope::for_app; keep both mappings aligned.
 * Legacy single-client profiles use this mapping; other clients use agent bundles.
 */
export const APP_PROFILE_SCOPE: Partial<Record<AppId, ProfileScope>> = {
  claude: "claude",
  "claude-desktop": "claude-desktop",
  codex: "codex",
};

/** Payload slot keys within a scope; mirrors ProfileScope::apps. */
const SCOPE_SLOT_KEYS: Record<ProfileScope, (keyof PerApp<unknown>)[]> = {
  claude: ["claude"],
  "claude-desktop": ["claude-desktop"],
  codex: ["codex"],
};

/**
 * A scope has a snapshot when any slot is non-null.
 * Applying an uncaptured scope only binds the current marker, without changing configuration.
 */
export function hasScopeSnapshot(profile: Profile, scope: ProfileScope) {
  const { providers, mcp, skills, prompts } = profile.payload;
  return SCOPE_SLOT_KEYS[scope].some(
    (app) =>
      providers[app] !== null ||
      mcp[app] !== null ||
      skills[app] !== null ||
      prompts[app] !== null,
  );
}
