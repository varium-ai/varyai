import { expect, it } from "vitest";
import {
  recommendRoutingMemberOrder,
  routingTemplates,
} from "@/lib/routingGroupTransfer";
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
    { compact: true },
    undefined,
    undefined,
  ]);
  expect(smart.group.rules[1].description).toContain("hard task");
  expect(smart.group.intentRecognitionEnabled).toBe(true);
  expect(smart.group.classifierTarget).toEqual({
    providerId: "a",
    modelId: "tiny-nano",
  });
  expect(capability.group.members[0].modelId).toBe("large-pro");
  expect(speed.group.members[0].modelId).toBe("large");
  expect(expiry.group.members[0].modelId).toBe("large");
  for (const template of [capability, speed, expiry]) {
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

it("keeps the speed template's strongest candidates within Magpie's price ceiling", () => {
  const speed = routingTemplates([
    {
      ...providers[0],
      models: [
        {
          id: "flagship-pro",
          name: "Overpriced flagship",
          outputPrice: 101,
          maker: true,
          released: "2026-04-01",
        },
        {
          id: "fast-strong",
          name: "Fast strong",
          outputPrice: 20,
          maker: true,
          released: "2026-03-01",
        },
      ],
    },
    {
      ...providers[1],
      models: [
        {
          id: "other-strong",
          name: "Other strong",
          outputPrice: 20,
          maker: true,
          released: "2026-02-01",
        },
      ],
    },
  ]).find((template) => template.key === "templateSpeed")!;
  expect(speed.group.members.map(({ modelId }) => modelId)).toEqual(
    expect.arrayContaining(["fast-strong", "other-strong"]),
  );
  expect(speed.group.members).toHaveLength(2);
  expect(
    speed.group.members.some(({ modelId }) => modelId === "flagship-pro"),
  ).toBe(false);
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
  for (const template of routingTemplates([only])) {
    const { group } = template;
    if (template.key === "templateSpeed") {
      expect(template.need).toBe("providers");
      expect(group.members).toHaveLength(0);
    } else {
      expect(group.members).toHaveLength(1);
    }
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

it("keeps selected identities and tie order when applying capability recommendations", () => {
  const stale = { id: "stale", providerId: "a", modelId: "missing" };
  const fromB = { id: "from-b", providerId: "b", modelId: "large" };
  const mini = { id: "mini", providerId: "a", modelId: "small-mini" };
  const fromA = { id: "from-a", providerId: "a", modelId: "large" };
  const selected = [stale, fromB, mini, fromA];
  expect(recommendRoutingMemberOrder(selected, providers)).toEqual([
    fromB,
    fromA,
    mini,
    stale,
  ]);
  expect(selected).toEqual([stale, fromB, mini, fromA]);
});

it("ranks model tiers first and newer versions within the same family and tier", () => {
  const models = [
    { id: "gpt-6.1-sol", maker: true, outputPrice: 10, released: "2026-09-29" },
    { id: "gpt-6.2-sol" },
    { id: "gpt-6.2-astra" },
    {
      id: "gpt-6.1-terra",
      maker: true,
      outputPrice: 5,
      released: "2026-10-26",
    },
    {
      id: "gemini-3.8-flash",
      maker: true,
      outputPrice: 3,
      released: "2026-09-02",
    },
    {
      id: "gemini-4-argon",
      maker: true,
      outputPrice: 10,
      released: "2026-10-13",
    },
    { id: "zai-org/GLM-5.3-Flash", maker: true, outputPrice: 1 },
    { id: "zai-org/GLM-5.3", maker: true, outputPrice: 4 },
  ];
  const catalog = [
    {
      ...providers[0],
      models: models.map((model) => ({ ...model, name: model.id })),
    },
  ];
  const members = models.map(({ id }) => ({
    id,
    providerId: "a",
    modelId: id,
  }));
  const sorted = recommendRoutingMemberOrder(members, catalog).map(
    ({ modelId }) => modelId,
  );
  expect(sorted.slice(0, 3)).toEqual([
    "gemini-4-argon",
    "gpt-6.2-astra",
    "gpt-6.2-sol",
  ]);
  expect(sorted.indexOf("gpt-6.2-sol")).toBeLessThan(
    sorted.indexOf("gpt-6.1-sol"),
  );
  expect(sorted.indexOf("zai-org/GLM-5.3")).toBeLessThan(
    sorted.indexOf("zai-org/GLM-5.3-Flash"),
  );
  const [smart, capability] = routingTemplates(catalog);
  expect(capability.group.members.map(({ modelId }) => modelId)).toEqual(
    sorted.slice(0, 3),
  );
  expect(smart.group.members[0].modelId).toBe("gemini-4-argon");
});

it("compares version segments numerically and uses catalog order across families", () => {
  const models = [
    { id: "gpt-6.9-sol", maker: true },
    { id: "gpt-6.10-sol" },
    { id: "claude-opus-4-6", maker: true },
    { id: "claude-opus-4-8" },
    { id: "qwen-15-pro", maker: true },
    { id: "gpt-7-flash", maker: true },
  ];
  const catalog = [
    {
      ...providers[0],
      models: models.map((model) => ({ ...model, name: model.id })),
    },
  ];
  const members = models.map(({ id }) => ({
    id,
    providerId: "a",
    modelId: id,
  }));
  expect(
    recommendRoutingMemberOrder(members, catalog).map(({ modelId }) => modelId),
  ).toEqual([
    "claude-opus-4-8",
    "qwen-15-pro",
    "claude-opus-4-6",
    "gpt-6.10-sol",
    "gpt-6.9-sol",
    "gpt-7-flash",
  ]);
});

it("skips Codex automatic review models in template suggestions", () => {
  const reviewModels = ["codex-auto-review", "relay/CODEX-AUTO-REVIEW"];
  const templates = routingTemplates([
    {
      ...providers[0],
      models: [
        ...reviewModels.map((id) => ({
          id,
          name: "Automatic review",
          outputPrice: 0,
        })),
        ...providers[0].models!,
      ],
    },
  ]);
  expect(templates[0].group.classifierTarget).toEqual({
    providerId: "a",
    modelId: "tiny-nano",
  });
  for (const { group } of templates) {
    expect(
      group.members.some(({ modelId }) => reviewModels.includes(modelId)),
    ).toBe(false);
  }
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
