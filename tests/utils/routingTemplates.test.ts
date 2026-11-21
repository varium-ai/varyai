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
      {
        id: "gpt-5.4-pro",
        name: "Another model",
        outputPrice: 101,
        maker: true,
      },
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
  expect(capability.group.members[0].modelId).toBe("gpt-5.4-pro");
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

it("prefers different models across suppliers in all four templates", () => {
  const [smart, ...other] = routingTemplates([
    {
      ...providers[0],
      models: [
        { id: "fixture-pro", name: "Primary", outputPrice: 20 },
        { id: "fixture-mini", name: "Economy", outputPrice: 2 },
      ],
    },
    {
      ...providers[1],
      models: [
        { id: "vendor/FIXTURE-PRO", name: "Mirror", outputPrice: 15 },
        { id: "fixture-standard", name: "Fallback", outputPrice: 20 },
      ],
    },
    {
      ...providers[1],
      id: "c",
      models: [{ id: "relay-alias", sameAs: "fixture-mini", name: "Alias" }],
    },
  ]);
  expect(smart.group.members.map(({ modelId }) => modelId)).toEqual([
    "fixture-pro",
    "fixture-mini",
  ]);
  expect(smart.group.rules[2].targetMemberId).toBe(smart.group.members[1].id);
  for (const template of other) {
    expect(template.group.members.map(({ modelId }) => modelId)).toEqual([
      "fixture-pro",
      "fixture-standard",
      "relay-alias",
    ]);
  }
});

it("revisits a supplier's first choice when needed for three distinct models", () => {
  const models = [
    { id: "first", name: "First", outputPrice: 20 },
    { id: "second", name: "Second", outputPrice: 20 },
    { id: "third", name: "Third", outputPrice: 20 },
  ];
  const [, capability, speed, expiry] = routingTemplates([
    { ...providers[0], models: models.slice(0, 2) },
    { ...providers[1], models: [models[0]] },
    { ...providers[1], id: "c", models: models.slice(1) },
  ]);
  for (const template of [capability, speed, expiry]) {
    expect(
      template.group.members.map(({ providerId, modelId }) => [
        providerId,
        modelId,
      ]),
    ).toEqual([
      ["a", "second"],
      ["b", "first"],
      ["c", "third"],
    ]);
  }
});

it("keeps a same-model alternate route when models are scarce", () => {
  const model = { id: "fixture", name: "Fixture", outputPrice: 15 };
  for (const { group } of routingTemplates(
    providers.map((provider) => ({ ...provider, models: [model] })),
  )) {
    expect(
      group.members.map(({ providerId, modelId }) => [providerId, modelId]),
    ).toEqual([
      ["a", "fixture"],
      ["b", "fixture"],
    ]);
  }
});

it("uses a distinct smart fallback when there is no cheaper model", () => {
  const primary = { id: "fixture-main", name: "Primary", outputPrice: 15 };
  const [smart] = routingTemplates([
    {
      ...providers[0],
      models: [
        primary,
        { id: "fixture-other", name: "Other", outputPrice: 15 },
      ],
    },
    { ...providers[1], models: [primary] },
  ]);
  expect(smart.group.members.map(({ modelId }) => modelId)).toEqual([
    "fixture-main",
    "fixture-other",
  ]);
  expect(smart.group.rules).toEqual([]);
});

it("uses configured equivalence before spending a slot on an alias", () => {
  const catalog = [
    {
      ...providers[0],
      models: [
        { id: "fixture-1", name: "First", outputPrice: 20 },
        { id: "fixture-3", name: "Third", outputPrice: 20 },
      ],
    },
    {
      ...providers[1],
      models: [{ id: "fixture-2", name: "Second", outputPrice: 15 }],
    },
    {
      ...providers[1],
      id: "c",
      models: [
        {
          id: "relay-alias",
          sameAs: "fixture-1",
          name: "Alias",
          outputPrice: 20,
        },
      ],
    },
  ];
  const templates = routingTemplates(catalog);
  for (const { key, group } of templates) {
    const expected =
      key === "templateSmart"
        ? ["fixture-1", "fixture-2"]
        : ["fixture-1", "fixture-2", "fixture-3"];
    expect(group.members).toHaveLength(expected.length);
    expect(
      new Set(
        group.members.map(({ modelId }) =>
          modelId === "relay-alias" ? "fixture-1" : modelId,
        ),
      ),
    ).toEqual(new Set(expected));
  }
  expect(
    templates[2].group.members.map(({ providerId }) => providerId),
  ).toEqual(["a", "b", "c"]);
});

it("keeps selected identities and resolves ranking ties in catalog order", () => {
  const stale = { id: "stale", providerId: "a", modelId: "missing" };
  const fromB = { id: "from-b", providerId: "b", modelId: "large" };
  const mini = { id: "mini", providerId: "a", modelId: "small-mini" };
  const fromA = { id: "from-a", providerId: "a", modelId: "large" };
  const selected = [stale, fromB, mini, fromA];
  expect(recommendRoutingMemberOrder(selected, providers)).toEqual([
    fromA,
    fromB,
    mini,
    stale,
  ]);
  expect(recommendRoutingMemberOrder([fromA, fromB], providers)).toEqual([
    fromA,
    fromB,
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
    "zai-org/GLM-5.3",
    "gpt-6.2-astra",
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
    { id: "qwen4-max", maker: true },
    { id: "gpt-7-mini", maker: true },
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
    "qwen4-max",
    "claude-opus-4-6",
    "gpt-6.10-sol",
    "gpt-6.9-sol",
    "gpt-7-mini",
  ]);
});

it.each([
  [
    "OpenAI",
    ["openai/gpt-6.2-astra", "gpt-6.2-sol", "gpt-6.1-terra", "gpt-6-luna"],
  ],
  [
    "Anthropic",
    [
      "global.anthropic.claude-fable-5-2",
      "anthropic.claude-opus-5-5",
      "anthropic.claude-opus-4-6-v1:0",
      "claude-sonnet-4-6",
      "claude-3-5-sonnet-20241022",
      "claude-haiku-4-5@20251001",
    ],
  ],
  [
    "Gemini",
    [
      "gemini-4-carbon",
      "gemini-4-neon",
      "gemini-4-hydrogen",
      "gemini-3.5-flash-lite",
    ],
  ],
  ["DeepSeek", ["deepseek-v4-pro-0813", "deepseek-flash"]],
  ["Qwen", ["Qwen/qwen4-max", "qwen3.7-plus", "qwen3.8-flash"]],
  ["GLM", ["zai-org/GLM-5.3", "glm-5-turbo", "glm-5.3-flashx"]],
  ["Kimi", ["kimi-k3.1-pro-highspeed", "kimi-k3", "kimi-k2.7-code-highspeed"]],
  [
    "Grok",
    ["grok-4.7-fast", "grok-4.6", "grok-4.3", "grok-4.20-0309-reasoning"],
  ],
  ["MiniMax", ["MiniMax-M2.7-highspeed", "MiniMax-M2.5"]],
  ["Mistral", ["mistral-large-4", "mistral-medium-2508", "mistral-small-2603"]],
  [
    "ByteDance Seed",
    [
      "doubao-seed-2-1-pro-260628",
      "dola-seed-2-1-turbo-260628",
      "doubao-seed-2-0-lite",
      "seed-2-0-mini",
    ],
  ],
  [
    "Xiaomi MiMo",
    [
      "xiaomi/mimo-v2.6-pro",
      "mimo-v2.5-pro-ultraspeed",
      "mimo-v2.5",
      "mimo-v2-flash",
    ],
  ],
] as const)("uses documented %s model families and editions", (_, ordered) => {
  const models = [...ordered]
    .reverse()
    .map((id) => ({ id, name: `Unrelated display label for ${id}` }));
  const members = models.map(({ id }) => ({
    id,
    modelId: id,
    providerId: "a",
  }));
  const catalog = [{ ...providers[0], models }];
  expect(
    recommendRoutingMemberOrder(members, catalog).map(({ modelId }) => modelId),
  ).toEqual(ordered);
  expect(
    routingTemplates(catalog)
      .find(({ key }) => key === "templateCapability")
      ?.group.members.map(({ modelId }) => modelId),
  ).toEqual(ordered.slice(0, 3));
});

it.each([
  ["unfamiliar IDs", ["example-fast", "example-pro", "example-8b"]],
  [
    "Kimi equivalent serving variants",
    ["kimi-k3.1-pro-highspeed", "kimi-k3.1-pro"],
  ],
  ["MiniMax distinct frontier lines", ["MiniMax-M3.1-Flash", "MiniMax-M3"]],
  [
    "Xiaomi equivalent serving variants",
    ["mimo-v2.5-pro-ultraspeed", "mimo-v2.5-pro"],
  ],
  [
    "unknown editions within a known vendor",
    ["gpt-6.2-nova-pro", "example-pro"],
  ],
] as const)(
  "preserves metadata ties for %s without treating suffixes as universal",
  (_, ids) => {
    for (const order of [ids, [...ids].reverse()]) {
      const members = order.map((id) => ({ id, modelId: id, providerId: "a" }));
      const catalog = [
        { ...providers[0], models: order.map((id) => ({ id, name: id })) },
      ];
      expect(recommendRoutingMemberOrder(members, catalog)).toEqual(members);
    }
  },
);

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

it("draws different models while retaining the constraints of each template", () => {
  const catalog = Array.from({ length: 4 }, (_, index) => ({
    ...providers[0],
    id: `supplier-${index}`,
    models: [
      {
        id: `gpt-6.${index + 1}-pro`,
        name: `Strong ${index}`,
        outputPrice: 20 + index,
        maker: true,
        reasoning: true,
      },
      {
        id: `qwen4-mini-${index}`,
        name: `Economy ${index}`,
        outputPrice: 2,
        reasoning: false,
      },
      { id: "fixture-alias", sameAs: "gpt-6.1-pro", name: "Mirror" },
      { id: "codex-auto-review", name: "Automatic review", outputPrice: 0 },
    ],
  }));
  const first = routingTemplates(catalog, () => 0.999);
  const second = routingTemplates(catalog, () => 0);
  for (const [index, { key, group }] of second.entries()) {
    const ids = group.members.map(({ modelId }) => modelId);
    expect(new Set(ids)).not.toEqual(
      new Set(first[index].group.members.map(({ modelId }) => modelId)),
    );
    const expectedCount = key === "templateSmart" ? 2 : 3;
    expect(group.members).toHaveLength(expectedCount);
    expect(
      new Set(ids.map((id) => (id === "fixture-alias" ? "gpt-6.1-pro" : id)))
        .size,
    ).toBe(expectedCount);
    expect(ids).not.toContain("codex-auto-review");
    if (key === "templateSpeed")
      expect(
        new Set(group.members.map(({ providerId }) => providerId)).size,
      ).toBe(3);
    if (key === "templateSmart") {
      expect(
        new Set(group.rules.map(({ targetMemberId }) => targetMemberId)),
      ).toEqual(new Set(group.members.map(({ id }) => id)));
      const easy = group.members.find(
        ({ id }) => id === group.rules[2].targetMemberId,
      );
      const prices = new Map(
        catalog.flatMap(({ models }) =>
          models.map((model) => [model.id, model.outputPrice]),
        ),
      );
      expect(prices.get(easy!.modelId)).toBeLessThan(prices.get(ids[0])!);
    }
  }
});
