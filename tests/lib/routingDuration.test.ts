import { createInstance } from "i18next";
import { expect, it } from "vitest";
import { formatRoutingDuration } from "@/lib/routingDuration";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

it.each([
  [-1, "1 s"],
  [0, "1 s"],
  [59_499, "59 s"],
  [59_500, "1 min"],
  [59 * 60_000, "59 min"],
  [59 * 60_000 + 30_000, "1 h"],
  [60 * 60_000, "1 h"],
  [(9 * 60 + 59) * 60_000, "9 h 59 min"],
  [10 * 3_600_000, "10 h"],
  [(10 * 60 + 31) * 60_000, "11 h"],
  [24 * 3_600_000, "24 h"],
  [(47 * 60 + 59) * 60_000, "48 h"],
  [(47 * 60 + 59) * 60_000 + 30_000, "2 d"],
  [48 * 3_600_000, "2 d"],
  [60 * 3_600_000, "3 d"],
  [(88 * 60 + 46) * 60_000, "4 d"],
])(
  "matches Magpie's duration display for %i milliseconds",
  async (ms, expected) => {
    const i18n = createInstance();
    await i18n.init({ lng: "en", resources: { en: { translation: en } } });
    expect(formatRoutingDuration(ms, i18n.t)).toBe(expected);
  },
);

it.each([
  ["en", en, "resets in 4 d"],
  ["zh", zh, "4 天后重置"],
  ["ja", ja, "4 日後にリセット"],
  ["zh-TW", zhTW, "4 天後重置"],
])(
  "localizes the screenshot's multi-day reset in %s",
  async (language, resource, expected) => {
    const i18n = createInstance();
    await i18n.init({
      lng: language,
      resources: { [language]: { translation: resource } },
    });
    expect(
      i18n.t("routingPage.resetIn", {
        time: formatRoutingDuration((88 * 60 + 46) * 60_000, i18n.t),
      }),
    ).toBe(expected);
  },
);
