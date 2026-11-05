import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { routingUpstreamsApi, type RoutingUpstream } from "./routingUpstreams";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

it("keeps observed subscription plans out of provider saves", async () => {
  const settings = {
    accounts: ["synthetic-account"],
    mode: "vendor" as const,
    models: [],
    groupsOnly: false,
  };
  const upstream: RoutingUpstream = {
    schemaVersion: 1,
    id: "synthetic",
    name: "Synthetic subscription",
    revision: 1,
    source: { kind: "codex" },
    hasApiKey: false,
    codexSettings: settings,
    subscriptionPlans: ["ChatGPT Plus"],
  };
  const { subscriptionPlans: _plans, ...saved } = upstream;
  await routingUpstreamsApi.save(upstream);
  expect(invoke).toHaveBeenLastCalledWith("save_routing_upstream", {
    upstream: saved,
    copyFrom: undefined,
  });
  await routingUpstreamsApi.saveCodex(upstream, settings);
  expect(invoke).toHaveBeenLastCalledWith("save_codex_provider", {
    upstream: saved,
    settings,
  });
});
