import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

describe("platform-specific restart translations", () => {
  it.each([
    [en, /immediately/i, /interrupt/i],
    [ja, /すぐに/, /中断/],
    [zhTW, /立即/, /中斷/],
    [zh, /立即/, /中断/],
  ] as const)(
    "warns that a confirmed Codex restart interrupts running tasks",
    (locale, immediate, interrupted) => {
      for (const hint of [
        locale.agentConnections.codexRestartConfirmHint,
        locale.agentConnections.codexRestartConfirmHintWindows,
      ]) {
        expect(hint).toMatch(immediate);
        expect(hint).toMatch(interrupted);
      }
    },
  );

  it.each([en, ja, zhTW, zh])(
    "keeps operating system guidance separate",
    (locale) => {
      const hints = locale.agentConnections;
      expect(hints.restartConfirmHint).not.toMatch(/macOS|Windows/);
      expect(hints.restartConfirmHintMac).toContain("macOS");
      expect(hints.restartConfirmHintMac).not.toContain("Windows");
      expect(hints.restartConfirmHintWindows).toContain("Windows");
      expect(hints.restartConfirmHintWindows).not.toContain("macOS");
      expect(hints.codexRestartConfirmHint).not.toMatch(/macOS|Windows/);
      expect(hints.codexRestartConfirmHintWindows).toContain("Windows");
      expect(hints.codexRestartConfirmHintWindows).not.toContain("macOS");
    },
  );
});
