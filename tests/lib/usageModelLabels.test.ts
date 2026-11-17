import { describe, expect, it } from "vitest";
import { normalizeUsageModelId } from "@/types/usage";
import { getUsageModelOptions } from "@/components/usage/modelOptions";

const bare = `model-${"a".repeat(64)}`;
const qualified = `vary-provider/${bare}`;
const row = (model: string, aliases: string[] = []) => ({
  model,
  aliases,
  requestCount: 1,
  totalTokens: 10,
  totalCost: "0.5",
  avgCostPerRequest: "0.5",
});

describe("usage routing model names", () => {
  it("treats only the exact generated namespace as the same analytics identity", () => {
    expect(normalizeUsageModelId(qualified)).toBe(bare);
    for (const custom of [
      "vary-provider/model-custom",
      `custom/${bare}`,
      "model-custom",
    ])
      expect(normalizeUsageModelId(custom)).toBe(custom);
  });

  it("deduplicates the prefixed and bare forms and never uses one as a readable name", () => {
    const options = getUsageModelOptions([row(qualified, [bare])]);
    expect(options).toEqual([
      {
        value: bare,
        routingId: bare,
        routingModels: [],
        resolvedLabel: undefined,
      },
    ]);
  });

  it("never promotes unknown or incomplete aliases to readable routing labels", () => {
    const options = getUsageModelOptions([
      row("unknown", [bare, qualified, "-codex"]),
    ]);
    expect(
      options.find((option) => option.value === bare)?.routingModels,
    ).toEqual([]);
  });

  it("uses exact catalog metadata even when a session has no upstream model", () => {
    const label = {
      id: bare,
      model: "synthetic-model",
      displayName: "Synthetic Model",
      providerName: "Synthetic supplier",
    };
    const options = getUsageModelOptions([row(qualified)], undefined, [label]);
    expect(options[0]).toEqual({
      value: bare,
      routingId: bare,
      routingModels: [],
      resolvedLabel: label,
    });
  });
});
