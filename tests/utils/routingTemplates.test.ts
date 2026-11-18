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
      { id: "large-pro", name: "Another model", outputPrice: 101, maker: true },
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
        images: false,
      },
      {
        id: "tiny-nano",
        name: "Nano",
        outputPrice: 0.1,
        maker: true,
        released: "2026-03-01",
        reasoning: false,
        images: false,
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

it("creates four ordinary group drafts with distinct defaults", () => {
  const [smart, capability, speed, expiry] = routingTemplates(providers);
  expect([smart.key, capability.key, speed.key, expiry.key]).toEqual([
    "templateSmart",
    "templateCapability",
    "templateSpeed",
    "templateExpiry",
  ]);
  expect(smart.group.routing).toBe("ordered");
  expect(smart.group.members[0].modelId).toBe("large");
  expect(smart.group.rules.map((rule) => rule.conditions)).toEqual([
    { images: true },
    { effort: "high" },
    undefined,
    undefined,
  ]);
  expect(smart.group.rules[2].description).toContain("hard task");
  expect(smart.group.intentRecognitionEnabled).toBe(true);
  expect(smart.group.classifierTarget).toEqual({
    providerId: "a",
    modelId: "tiny-nano",
  });
  for (const template of [capability, speed, expiry]) {
    expect(template.group.members[0].modelId).toBe("large");
    expect(template.group.fallbackMemberIds).toEqual(
      template.group.members.slice(1).map((member) => member.id),
    );
    expect(template.group.rules).toEqual([]);
    expect(template.group.intentRecognitionEnabled).toBe(false);
    expect(template.group.classifierTarget).toBeNull();
  }
  expect(capability.group.routing).toBe("ordered");
  expect(speed.group.routing).toBe("fastest");
  expect(expiry.group).toMatchObject({
    routing: "ordered",
    quotaOrder: "earliest_expiry",
  });
  for (const { group } of [smart, capability, speed, expiry]) {
    expect(group).toMatchObject({
      client: "global",
      revision: 0,
      affinity: "turn",
    });
    expect(group).not.toHaveProperty("policy");
  }
});

it("keeps all template drafts editable with no catalog and deduplicates repeated models", () => {
  expect(routingTemplates([]).map((template) => template.need)).toEqual([
    "provider",
    "provider",
    "provider",
    "provider",
  ]);
  const model = providers[0].models![3];
  const only = { ...providers[0], models: [model, { ...model }] };
  for (const { group } of routingTemplates([only])) {
    expect(group.members).toHaveLength(1);
    expect(group.fallbackMemberIds).toEqual([]);
    expect(group.classifierTarget).toBeNull();
  }
  expect(
    routingTemplates(
      providers.map((p) => ({ ...p, taskEligible: false })),
    ).every((template) => template.need === "provider"),
  ).toBe(true);
});

it("keeps unknown prices from qualifying a model as cheaper", () => {
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
  ]);
  expect(templates[0].group.classifierTarget).toBeNull();
});

it("never silently enables an ineligible classifier", () => {
  const [smart, capability] = routingTemplates(
    providers.map((provider) => ({ ...provider, classifierEligible: false })),
  );
  expect(smart.need).toBe("classifier");
  expect(smart.group.members.length).toBeGreaterThan(1);
  expect(smart.group.intentRecognitionEnabled).toBe(false);
  expect(capability.need).toBeUndefined();
});
