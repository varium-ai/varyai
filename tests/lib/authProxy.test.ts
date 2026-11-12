import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { authStartLogin } from "@/lib/api/auth";
import { codexBrowserLoginApi } from "@/lib/api/codexBrowserLogin";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

it.each([
  [undefined, null],
  ["", ""],
  ["direct", "direct"],
  ["http://127.0.0.1:7890", "http://127.0.0.1:7890"],
])(
  "preserves proxy selection %s across both login commands",
  async (proxy, expected) => {
    await authStartLogin("codex_oauth", undefined, "fixture-account", proxy);
    expect(invoke).toHaveBeenLastCalledWith(
      "auth_start_login",
      expect.objectContaining({
        targetAccountId: "fixture-account",
        proxy: expected,
      }),
    );
    await codexBrowserLoginApi.start("fixture-account", proxy);
    expect(invoke).toHaveBeenLastCalledWith(
      "codex_browser_login_start",
      expect.objectContaining({
        targetAccountId: "fixture-account",
        proxy: expected,
      }),
    );
  },
);
