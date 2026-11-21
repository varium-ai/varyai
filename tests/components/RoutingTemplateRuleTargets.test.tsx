import { act, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { it, expect, vi } from "vitest";
import { RoutingTemplateRuleTargets } from "@/components/routing/RoutingTemplateRuleTargets";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";
import { blankRoutingGroup } from "@/lib/routingGroupTransfer";

const hardPrompt =
  "A hard task: complex reasoning, architecture design, root-cause analysis, changes across several files, or a task whose difficulty is unclear.";
const easyPrompt =
  "An easy task: a straightforward question with enough information, formatting, a lookup, or a small well-defined edit.";

async function renderSavedRules(hardDescription = hardPrompt) {
  const i18n = createInstance();
  await i18n.init({
    lng: "zh",
    resources: {
      en: { translation: en },
      ja: { translation: ja },
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
    },
  });
  const group = {
    ...blankRoutingGroup(),
    revision: 1,
    rules: [
      {
        id: "hard",
        name: "Hard task",
        description: hardDescription,
        enabled: true,
        targetMemberId: "strong",
      },
      {
        id: "easy",
        name: "Easy task",
        description: easyPrompt,
        enabled: true,
        targetMemberId: "economy",
      },
    ],
  };
  const onChange = vi.fn();
  render(
    <I18nextProvider i18n={i18n}>
      <RoutingTemplateRuleTargets
        providers={[]}
        group={group}
        disabled={false}
        onChange={onChange}
      />
    </I18nextProvider>,
  );
  return { i18n, onChange };
}

it("renders saved built-in descriptions in the current language without changing the group", async () => {
  const { i18n, onChange } = await renderSavedRules();
  for (const [language, hard, easy] of [
    ["zh", /复杂推理/, /范围清楚/],
    ["zh-TW", /複雜推理/, /範圍清楚/],
    ["ja", /複雑な推論/, /範囲が明確/],
    ["en", hardPrompt, easyPrompt],
  ] as const) {
    await act(() => i18n.changeLanguage(language));
    expect(screen.getByText(hard)).toBeVisible();
    expect(screen.getByText(easy)).toBeVisible();
  }
  expect(onChange).not.toHaveBeenCalled();
});

it("shows a saved custom description verbatim across languages", async () => {
  const custom = "Classify API migrations as difficult.";
  const { i18n } = await renderSavedRules(custom);
  expect(screen.getByText(custom)).toBeVisible();
  await act(() => i18n.changeLanguage("ja"));
  expect(screen.getByText(custom)).toBeVisible();
});
