import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

type TranslationTree = Record<string, unknown>;

function flattenStrings(
  value: unknown,
  path: string[] = [],
  result = new Map<string, string>(),
): Map<string, string> {
  if (typeof value === "string") {
    result.set(path.join("."), value);
  } else if (typeof value === "object" && value !== null) {
    for (const [key, child] of Object.entries(value)) {
      flattenStrings(child, [...path, key], result);
    }
  }
  return result;
}

function interpolationVariables(value: string): string[] {
  return Array.from(
    value.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g),
    ([, name]) => name,
  ).sort();
}

const reference = flattenStrings(en);
const piKeysOutsideNamespace = new Set([
  "apps.pi",
  "deeplink.api",
  "sessionManager.piDiscoveryUnavailable",
  "sessionManager.piRelativeSessionDir",
  "settings.browsePlaceholderPi",
  "settings.piConfigDir",
  "settings.piConfigDirDescription",
]);
const piReference = new Map(
  [...reference].filter(
    ([key]) => key.startsWith("pi.") || piKeysOutsideNamespace.has(key),
  ),
);
const piProductReferences = new Map(
  [...reference].filter(([, value]) => /\bPi\b/.test(value)),
);
const locales = [
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;
const optionalBuiltinRuleIds = Object.keys(
  (en.security as { builtinRules: Record<string, unknown> }).builtinRules,
).filter((key) => key.startsWith("catalog_"));

describe("locale coverage", () => {
  it.each(locales)("covers every Pi translation key in %s", (_name, tree) => {
    const translations = flattenStrings(tree as TranslationTree);
    const missing = [...piReference.keys()].filter(
      (key) => !translations.has(key),
    );

    expect(missing).toEqual([]);
  });

  it.each(locales)(
    "preserves every Pi interpolation variable in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const mismatched = [...piReference].flatMap(([key, expected]) => {
        const actual = translations.get(key);
        return actual !== undefined &&
          interpolationVariables(actual).join("\0") !==
            interpolationVariables(expected).join("\0")
          ? [key]
          : [];
      });

      expect(mismatched).toEqual([]);
    },
  );

  it.each(locales)(
    "preserves explicit Pi product mentions in %s",
    (_name, tree) => {
      const translations = flattenStrings(tree as TranslationTree);
      const missingMentions = [...piProductReferences.keys()].filter((key) => {
        const actual = translations.get(key);
        return actual === undefined || !/\bPi\b/.test(actual);
      });

      expect(missingMentions).toEqual([]);
    },
  );

  it.each(locales)(
    "provides a distinct description for every optional built-in rule in %s",
    (_name, tree) => {
      const builtinRules = (
        tree as unknown as {
          security: {
            builtinRules: Record<string, { description?: string }>;
          };
        }
      ).security.builtinRules;
      const descriptions = optionalBuiltinRuleIds.map((key) =>
        builtinRules[key]?.description?.trim(),
      );

      expect(descriptions.every(Boolean)).toBe(true);
      expect(new Set(descriptions).size).toBe(descriptions.length);
    },
  );
});

const eventReasonCodes = [
  "destructive_target",
  "sensitive_target",
  "exfil_to_remote",
  "remote_exec_chain",
  "privilege_escalation",
  "security_config_target",
  "destructive_sql",
  "unclassified_command",
  "unparseable",
  "file_mutate",
  "network_egress",
  "obfuscated",
  "script_exec",
  "privilege",
  "custom_blocklist",
];

const phaseZeroKeys = [
  "toolRisk.colReason",
  "toolRisk.reasonUnknown",
  "toolRisk.capabilityNotReady",
  "toolRisk.databaseCapabilityHint",
  ...eventReasonCodes.map((code) => `toolRisk.reason.${code}`),
];

describe("tool risk Phase 0 locale coverage", () => {
  it.each([["en", en], ...locales] as const)(
    "provides all reason and capability strings in %s",
    (_locale, tree) => {
      const strings = flattenStrings(tree);
      phaseZeroKeys.forEach((key) => {
        expect(strings.get(key)?.trim(), `${_locale}: ${key}`).toBeTruthy();
        expect(strings.get(key)).not.toBe(key);
      });
      expect(Object.keys(tree.toolRisk.reason).sort()).toEqual(
        [...eventReasonCodes].sort(),
      );
    },
  );

  it.each(locales)(
    "keeps tool risk translation keys and interpolation variables aligned in %s",
    (_locale, tree) => {
      const baseline = flattenStrings(en.toolRisk);
      const actual = flattenStrings(tree.toolRisk);
      expect([...actual.keys()].sort()).toEqual([...baseline.keys()].sort());
      for (const [key, value] of baseline) {
        expect(interpolationVariables(actual.get(key)!)).toEqual(
          interpolationVariables(value),
        );
      }
    },
  );
});

describe("P2-11 notification locale coverage", () => {
  it.each([["en", en] as const, ...locales])(
    "provides safe window-summary labels in %s",
    (_locale, tree) => {
      const strings = flattenStrings(tree as TranslationTree);
      for (const key of [
        "notifySummaryBody",
        "notifyUnknownCategory",
        "notifyUnknownSeverity",
      ]) {
        expect(strings.get(`toolRisk.${key}`)?.trim()).toBeTruthy();
      }
      expect(
        interpolationVariables(strings.get("toolRisk.notifySummaryBody") ?? ""),
      ).toEqual(["category", "count", "severity"]);
      expect(strings.get("toolRisk.notifyTitle")).not.toMatch(
        /已阻止|已拦截|已阻擋|blocked|intercepted|ブロック済み/i,
      );
    },
  );
});

const policyKinds = [
  "settings_update",
  "rule_create",
  "rule_update",
  "rule_delete",
  "builtin_toggle",
  "builtin_reset",
  "list_create",
  "list_update",
  "list_delete",
  "unknown",
];
const policyFields = [
  "enabled",
  "name",
  "description",
  "selector",
  "conditions",
  "risk_category",
  "severity",
  "reason_code",
  "local_review",
  "list_type",
  "match_kind",
  "pattern",
];
describe("P2-9 policy receipt and history locale coverage", () => {
  it.each([["en", en] as const, ...locales])(
    "provides all fixed labels and aligned interpolation in %s",
    (_locale, tree) => {
      const strings = flattenStrings(tree);
      const keys = [
        "savedNotApplied",
        "auditUnavailable",
        "policyWriteFailed",
        ...[
          "entry",
          "title",
          "description",
          "metadataOnly",
          "actorHint",
          "clear",
          "clearConfirmTitle",
          "clearConfirmDescription",
          "confirmClear",
          "cleared",
          "clearFailed",
          "loadFailed",
          "empty",
          "page",
          "targetUnknown",
          "noTarget",
          "noFields",
          "colTime",
          "colVersion",
          "colActor",
          "colKind",
          "colTarget",
          "colFields",
          "colAudit",
        ].map((k) => `policyChanges.${k}`),
        ...policyKinds.map((k) => `policyChanges.kind.${k}`),
        ...policyFields.map((k) => `policyChanges.field.${k}`),
        ...["local_ui", "system", "unknown"].map(
          (k) => `policyChanges.actor.${k}`,
        ),
        ...["recorded", "audit_unavailable", "unknown"].map(
          (k) => `policyChanges.audit.${k}`,
        ),
      ];
      for (const key of keys) {
        expect(
          strings.get(`toolRisk.${key}`)?.trim(),
          `${_locale}: ${key}`,
        ).toBeTruthy();
        expect(strings.get(`toolRisk.${key}`)).not.toBe(`toolRisk.${key}`);
        expect(strings.get(`toolRisk.${key}`)).not.toMatch(
          /已阻止|已拦截|已阻擋|blocked|intercepted|ブロック済み/i,
        );
      }
      expect(
        interpolationVariables(
          strings.get("toolRisk.policyChanges.page") ?? "",
        ),
      ).toEqual(["page"]);
      expect(
        interpolationVariables(strings.get("toolRisk.savedNotApplied") ?? ""),
      ).toEqual([]);
      expect(
        interpolationVariables(strings.get("toolRisk.auditUnavailable") ?? ""),
      ).toEqual([]);
      expect(strings.get("toolRisk.policyChanges.description")).toContain(
        "1000",
      );
    },
  );
});

describe("P2-9 saved policy and sandbox wording", () => {
  const wordingLocales = [
    ["zh", zh, "已保存策略版本", "沙箱测试失败。"],
    ["en", en, "Saved policy version", "Sandbox test failed."],
    [
      "ja",
      ja,
      "保存済みポリシーバージョン",
      "サンドボックステストに失敗しました。",
    ],
    ["zh-TW", zhTW, "已儲存策略版本", "沙箱測試失敗。"],
  ] as const;

  it.each(wordingLocales)(
    "labels the persisted policy version as saved in %s",
    (_locale, tree, policyVersion) => {
      expect(tree.toolRisk.policyVersion).toBe(policyVersion);
    },
  );

  it.each(wordingLocales)(
    "provides a complete fixed tool risk sandbox failure sentence in %s",
    (_locale, tree, _policyVersion, sandboxFailed) => {
      expect(tree.toolRisk.sandboxFailed).toBe(sandboxFailed);
      expect(tree.toolRisk.sandboxFailed).not.toContain(": ");
      expect(interpolationVariables(tree.toolRisk.sandboxFailed)).toEqual([]);
    },
  );

  it.each([
    ["zh", zh, "沙箱测试失败: "],
    ["en", en, "Sandbox test failed: "],
  ] as const)(
    "preserves the original unrelated security sandbox prefix in %s",
    (_locale, tree, sandboxFailed) => {
      expect(tree.security.sandboxFailed).toBe(sandboxFailed);
    },
  );
});
