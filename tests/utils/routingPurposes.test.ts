import { describe, expect, it } from "vitest";
import {
  routingPurpose,
  routingPurposeTranslation,
} from "@/lib/routingPurposes";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

describe("routing purposes", () => {
  it.each([
    ["guardian", ["guardian", "auto_review", "guardian_review"]],
    ["memory_consolidation", ["memory_consolidation", "memory", "memgen"]],
    [
      "thread_title",
      [
        "thread_title",
        "thread_title_reconsideration",
        "title_generation",
        "title",
      ],
    ],
    ["collab_spawn", ["collab_spawn", "thread_spawn", "agent_job"]],
    [
      "ambient_suggestions",
      ["ambient_suggestions", "ambient_suggestion_safety"],
    ],
  ])(
    "groups %s aliases using the shared purpose catalog",
    (purpose, aliases) => {
      for (const kind of aliases) {
        expect(routingPurpose(kind)).toBe(`kind:${purpose}`);
      }
    },
  );

  it("keeps absent, unknown and unmarked-literal kinds distinct and treats future kinds as text", () => {
    expect(routingPurpose()).toBe("unmarked");
    expect(routingPurpose(null)).toBe("unmarked");
    expect(routingPurpose("")).toBe("unmarked");
    expect(routingPurpose("unmarked")).toBe("kind:unmarked");
    expect(routingPurpose("__proto__")).toBe("kind:__proto__");
    expect(routingPurposeTranslation("kind:__proto__")).toBeNull();
    expect(routingPurpose("future<&purpose>")).toBe("kind:future<&purpose>");
    expect(routingPurposeTranslation("kind:future<&purpose>")).toBeNull();
    expect(routingPurposeTranslation("kind:unknown")).toBe(
      "routingPage.purposes.unknown",
    );
  });

  it("provides the same purpose menu labels in every runtime language", () => {
    for (const locale of [zh, ja, zhTW]) {
      expect(Object.keys(locale.routingPage.purposes).sort()).toEqual(
        Object.keys(en.routingPage.purposes).sort(),
      );
    }
    for (const kind of Object.keys(en.routingPage.purposes)) {
      expect(
        routingPurposeTranslation(
          routingPurpose(kind === "unmarked" ? undefined : kind),
        ),
      ).not.toBeNull();
    }
  });
});
