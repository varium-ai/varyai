import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createInstance } from "i18next";
import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

const locales = [
  ["zh", zh],
  ["en", en],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;
const variables = (text: string) =>
  [...text.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((match) => match[1]).sort();

describe("unified security event translations", () => {
  it.each([
    ["zh", zh, "刷新记录"],
    ["en", en, "Refresh records"],
    ["ja", ja, "記録を更新"],
    ["zh-TW", zhTW, "重新整理記錄"],
  ] as const)(
    "labels the refresh action as records in %s",
    async (language, tree, label) => {
      const instance = createInstance();
      await instance.init({
        lng: language,
        fallbackLng: false,
        resources: { [language]: { translation: tree } },
      });
      expect(instance.t("security.refreshLogs")).toBe(label);
    },
  );

  it.each(locales)(
    "covers the same keys and interpolation variables in %s",
    (_locale, tree) => {
      expect(Object.keys(tree.securityEvents).sort()).toEqual(
        Object.keys(en.securityEvents).sort(),
      );
      for (const key of Object.keys(
        en.securityEvents,
      ) as (keyof typeof en.securityEvents)[]) {
        expect(tree.securityEvents[key].trim()).toBeTruthy();
        expect(variables(tree.securityEvents[key])).toEqual(
          variables(en.securityEvents[key]),
        );
      }
    },
  );

  it.each(locales)(
    "renders count receipts with real interpolation in %s",
    async (language, tree) => {
      const instance = createInstance();
      await instance.init({
        lng: language,
        resources: { [language]: { translation: tree } },
        interpolation: { escapeValue: false },
      });
      const receipt = instance.t("securityEvents.clearSuccess", {
        total: 17,
        dlp: 12,
        toolRisk: 5,
      });
      expect(receipt).toContain("17");
      expect(receipt).toContain("12");
      expect(receipt).toContain("5");
      expect(receipt).not.toMatch(/\{\{|securityEvents\./);
      expect(instance.t("homeAudit.blocked")).not.toMatch(
        /拦截|攔截|block|遮断/i,
      );
    },
  );

  it.each(locales)(
    "provides cleanup safeguards and actual tool observation metadata in %s",
    async (language, tree) => {
      const instance = createInstance();
      await instance.init({
        lng: language,
        resources: { [language]: { translation: tree } },
        fallbackLng: false,
      });
      for (const key of [
        "security.riskCritical",
        "securityEvents.clearIrreversible",
        "securityEvents.sourceMcpRpc",
        "securityEvents.stageProposed",
        "securityEvents.stagePreExecution",
        "securityEvents.stageRpcSubmitted",
      ]) {
        expect(instance.getResource(language, "translation", key), key).toEqual(
          expect.any(String),
        );
        expect(instance.t(key), key).not.toBe(key);
      }
    },
  );

  it("provides every literal unified key used by the entry components", () => {
    const files = [
      "SecurityEventsPanel.tsx",
      "SecurityEventDetailsDialog.tsx",
      "securityEventLabels.ts",
      "SecurityActivitySummary.tsx",
      "ToolRiskRulesPanel.tsx",
    ];
    for (const file of files) {
      const source = readFileSync(
        resolve("src/components/security", file),
        "utf8",
      );
      for (const [, key] of source.matchAll(
        /["']securityEvents\.([\w]+)["']/g,
      )) {
        expect(
          Object.prototype.hasOwnProperty.call(en.securityEvents, key),
          `${file}: ${key}`,
        ).toBe(true);
      }
    }
  });
});
