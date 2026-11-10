import { render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { AgentUpdatesBar } from "@/components/agents/AgentUpdates";
import type {
  AgentUpdateResult,
  useAgentUpdates,
} from "@/hooks/useAgentUpdates";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

const locales = { en, zh, ja, "zh-TW": zhTW };
async function show(
  results: AgentUpdateResult[],
  language: keyof typeof locales = "zh",
) {
  const i18n = createInstance();
  await i18n.use(initReactI18next).init({
    lng: language,
    resources: { [language]: { translation: locales[language] } },
    interpolation: { escapeValue: false },
  });
  const updates = {
    cli: new Map(),
    desktop: undefined,
    updatable: [],
    busy: false,
    isBusy: () => false,
    active: [],
    progress: { index: results.length, total: results.length },
    pending: null,
    results,
    showBar: true,
    start: vi.fn(async () => {}),
    installTarget: null,
    installing: null,
    installResult: null,
    requestInstall: vi.fn(),
    confirmInstall: vi.fn(async () => {}),
    canInstallCli: () => true,
    cancelInstall: vi.fn(),
    confirm: vi.fn(),
    cancel: vi.fn(),
  } satisfies ReturnType<typeof useAgentUpdates>;
  render(
    <I18nextProvider i18n={i18n}>
      <AgentUpdatesBar
        updates={updates}
        name={(app) =>
          app === "pi" ? "Pi" : app === "codex" ? "Codex" : "OpenCode"
        }
        disabled={false}
      />
    </I18nextProvider>,
  );
  return screen.getByRole("status");
}
const pi: AgentUpdateResult = {
  app: "pi",
  outcome: "changed",
  from: "1.4.0",
  to: "1.5.0",
};

describe("Agent update result wording", () => {
  it.each(["en", "zh", "ja", "zh-TW"] as const)(
    "explains update failures in %s without exposing command output",
    async (language) => {
      const status = await show(
        [{ app: "pi", outcome: "failed", failureReason: "download" }],
        language,
      );
      const messages = locales[language].agentUpdates;
      expect(status.textContent).toBe(
        messages.failedReason
          .replace("{{app}}", "Pi")
          .replace("{{reason}}", messages.failureReasons.download),
      );
      expect(status).toHaveClass("failed");
    },
  );
  it.each([
    ["zh", "Pi 已更新：1.4.0 → 1.5.0"],
    ["zh-TW", "Pi 已更新：1.4.0 → 1.5.0"],
    ["en", "Pi updated: 1.4.0 → 1.5.0"],
    ["ja", "Pi を更新しました：1.4.0 → 1.5.0"],
  ] as const)(
    "reports a single update directly in %s",
    async (language, message) => {
      const status = await show([pi], language);
      expect(status).toHaveTextContent(message);
      expect(status.textContent).toBe(message);
      expect(status.querySelector("[title]")).toHaveAttribute("title", message);
    },
  );
  it.each(["failed", "unverified", "preflightFailed"] as const)(
    "reports a single %s outcome without a zero-success batch summary",
    async (outcome) => {
      const status = await show([{ app: "pi", outcome }]);
      expect(status.textContent).toBe(
        zh.agentUpdates[outcome].replace("{{app}}", "Pi"),
      );
      expect(status).toHaveClass("failed");
      expect(status).not.toHaveTextContent("已更新");
    },
  );
  it("summarizes multiple verified updates and keeps their version changes", async () => {
    const status = await show([
      pi,
      { app: "codex", outcome: "changed", from: "0.165.0", to: "0.166.0" },
    ]);
    expect(status).toHaveTextContent("已更新 2 个工具。");
    expect(status).toHaveTextContent("Pi 已更新：1.4.0 → 1.5.0");
    expect(status).toHaveTextContent("Codex 已更新：0.165.0 → 0.166.0");
    expect(status).not.toHaveClass("failed");
  });
  it.each([
    ["zh", "本次更新：1 个成功 · 1 个失败 · 1 个结果待确认。"],
    ["zh-TW", "本次更新：1 個成功 · 1 個失敗 · 1 個結果待確認。"],
    ["en", "Update results: 1 updated · 1 failed · 1 unverified."],
    ["ja", "更新結果：1 件成功 · 1 件失敗 · 1 件未確認。"],
  ] as const)(
    "distinguishes failures from unverified outcomes in %s",
    async (language, summary) => {
      const status = await show(
        [
          pi,
          { app: "codex", outcome: "failed" },
          { app: "opencode", outcome: "unverified" },
        ],
        language,
      );
      expect(status).toHaveTextContent(summary);
      expect(status).toHaveClass("failed");
      expect(status).not.toHaveTextContent(/{{|agentUpdates\./);
    },
  );
  it("states that no updates started when batch preflight fails", async () => {
    const status = await show([
      { app: "pi", outcome: "preflightFailed" },
      { app: "codex", outcome: "preflightFailed" },
    ]);
    expect(status).toHaveTextContent("本次更新：2 个未开始。");
    expect(status).not.toHaveTextContent("0 个");
  });
});
