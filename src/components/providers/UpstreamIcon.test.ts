import { describe, expect, it } from "vitest";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import { withUpstreamIcons } from "./UpstreamIcon";

const makeUpstream = (
  overrides: Partial<RoutingUpstream> & Pick<RoutingUpstream, "id" | "name">,
): RoutingUpstream => ({
  schemaVersion: 1,
  revision: 1,
  hasApiKey: true,
  source: {
    kind: "api",
    baseUrl: "https://example.invalid/v1",
    apiFormat: "openai_chat",
    models: [],
    catalog: [],
    mode: "picked",
    groupsOnly: false,
  },
  ...overrides,
});

describe("withUpstreamIcons", () => {
  it("resolves preset, host, codex, and explicit icons for provider models", () => {
    const upstreams: RoutingUpstream[] = [
      makeUpstream({
        id: "gemini-1",
        name: "my1",
        icon: null,
        source: {
          kind: "api",
          baseUrl: "https://generativelanguage.googleapis.com/v1beta",
          apiFormat: "gemini_native",
          models: [],
          catalog: [],
          mode: "picked",
          groupsOnly: false,
        },
      }),
      makeUpstream({
        id: "groq-1",
        name: "Groq",
        icon: null,
        presetId: "groq",
        source: {
          kind: "api",
          baseUrl: "https://api.groq.com/openai/v1",
          apiFormat: "openai_chat",
          models: [],
          catalog: [],
          mode: "picked",
          groupsOnly: false,
        },
      }),
      makeUpstream({
        id: "codex-1",
        name: "Codex",
        icon: null,
        source: { kind: "codex" },
      }),
      makeUpstream({
        id: "custom-1",
        name: "公司2",
        icon: null,
        source: {
          kind: "api",
          baseUrl: "https://newapi.typex-test.cn/v1",
          apiFormat: "openai_chat",
          models: [],
          catalog: [],
          mode: "picked",
          groupsOnly: false,
        },
      }),
    ];

    const models = [
      {
        id: "vary-provider/gemini-2.5-pro",
        reference: "gemini-1/gemini-2.5-pro",
        icon: null,
      },
      {
        id: "vary-provider/llama-3.3",
        reference: "groq-1/llama-3.3",
        icon: null,
      },
      {
        id: "vary-provider/gpt-5",
        reference: "codex-1/gpt-5",
        icon: null,
      },
      {
        id: "vary-provider/custom-model",
        reference: "custom-1/custom-model",
        icon: null,
      },
      {
        id: "vary-auto/group-1",
        reference: "group/group-1",
        icon: null,
      },
    ];

    const enriched = withUpstreamIcons(models, upstreams);
    expect(enriched[0].icon).toBe("gemini");
    expect(enriched[1].icon).toBe("groq");
    expect(enriched[2].icon).toBe("openai");
    expect(enriched[3].icon).toBeNull();
    expect(enriched[4].icon).toBeNull();
  });
});
