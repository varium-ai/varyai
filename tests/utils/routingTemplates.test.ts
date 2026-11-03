import { expect, it } from "vitest";
import { routingTemplates } from "@/lib/routingGroupTransfer";
import type { RoutingProviderOption } from "@/lib/api/intentRouting";

const providers: RoutingProviderOption[] = [
  {
    id: "a",
    name: "Synthetic A",
    taskEligible: true,
    classifierEligible: true,
    models: [
      { id: "embed-large", name: "Not chat", outputPrice: 99, maker: true },
      { id: "large-pro", name: "Too expensive", outputPrice: 101, maker: true },
      {
        id: "large-old",
        name: "Old",
        outputPrice: 30,
        maker: true,
        released: "2025-01-01",
      },
      {
        id: "large",
        name: "Large",
        outputPrice: 15,
        maker: true,
        released: "2026-01-01",
        images: true,
        reasoning: true,
      },
      {
        id: "small-mini",
        name: "Mini",
        outputPrice: 2,
        maker: true,
        released: "2026-02-01",
        reasoning: false,
      },
      {
        id: "tiny-nano",
        name: "Nano",
        outputPrice: 0.1,
        maker: true,
        released: "2026-03-01",
        reasoning: false,
      },
    ],
  },
  {
    id: "b",
    name: "Synthetic B",
    taskEligible: true,
    classifierEligible: true,
    models: [
      {
        id: "large",
        name: "Large copy",
        outputPrice: 15,
        maker: true,
        released: "2026-01-01",
        images: true,
      },
      {
        id: "other-mini",
        name: "Other",
        outputPrice: 1,
        maker: true,
        released: "2025-01-01",
      },
    ],
  },
];

it("ports the reference implementation's ranked smart, thrifty and steady templates instead of generic presets", () => {
  const [smart, thrifty, steady] = routingTemplates(providers);
  expect([smart.key, thrifty.key, steady.key]).toEqual([
    "templateSmart",
    "templateThrifty",
    "templateSteady",
  ]);
  expect(smart.group.routing).toBe("ordered");
  expect(smart.group.members.map((m) => m.modelId)).toEqual([
    "large",
    "small-mini",
  ]);
  expect(
    smart.group.rules.map((rule) => [rule.conditions, rule.targetMemberId]),
  ).toEqual([
    [{ compact: true }, smart.group.members[1].id],
    [{ images: true }, smart.group.members[0].id],
    [undefined, smart.group.members[0].id],
    [undefined, smart.group.members[1].id],
  ]);
  expect(smart.group.rules[2].description).toContain("a hard task:");
  expect(smart.group.intentRecognitionEnabled).toBe(true);
  expect(smart.group.classifierTarget).toEqual({
    providerId: "b",
    modelId: "other-mini",
  });
  expect(thrifty.group.members.map((m) => [m.providerId, m.modelId])).toEqual([
    ["a", "small-mini"],
    ["b", "other-mini"],
    ["a", "large"],
  ]);
  expect(thrifty.group.rules[0]).toMatchObject({
    conditions: { effort: "high" },
    targetMemberId: thrifty.group.members[2].id,
  });
  expect(steady.group.members.map((m) => [m.providerId, m.modelId])).toEqual([
    ["a", "large"],
    ["b", "large"],
  ]);
  expect(steady.group.rules).toEqual([]);
});

it("keeps unavailable templates with an actionable reason instead of inventing models", () => {
  expect(routingTemplates([]).map((template) => template.need)).toEqual([
    "provider",
    "provider",
    "provider",
  ]);
  const one = routingTemplates([
    { ...providers[0], models: [providers[0].models![3]] },
  ]);
  expect(one.map((template) => template.need)).toEqual([
    "cheap",
    "cheap",
    "providers",
  ]);
  expect(one.every((template) => template.group.members.length === 0)).toBe(
    true,
  );
  expect(
    routingTemplates(
      providers.map((p) => ({ ...p, taskEligible: false })),
    ).every((template) => template.need === "provider"),
  ).toBe(true);
});

it("uses whole model words, numeric sizes and the newest model without fabricated catalog prices", () => {
  const templates = routingTemplates([
    {
      ...providers[0],
      models: [
        { id: "gemini-pro", name: "Gemini", released: "2026-01-01" },
        { id: "model-8b", name: "Tiny", released: "2026-03-01" },
        { id: "model-72b", name: "Small", released: "2026-02-01" },
      ],
    },
  ]);
  expect(templates[0].group.members.map((m) => m.modelId)).toEqual([
    "gemini-pro",
    "model-72b",
  ]);
  expect(templates[0].group.classifierTarget).toEqual({
    providerId: "a",
    modelId: "model-72b",
  });
});

it("never silently enables an ineligible classifier", () => {
  const [smart, thrifty] = routingTemplates(
    providers.map((provider) => ({ ...provider, classifierEligible: false })),
  );
  expect(smart.need).toBe("classifier");
  expect(smart.group.members).toEqual([]);
  expect(thrifty.need).toBeUndefined();
});
