import { expect, it } from "vitest";
import { mergeModelMetadata, modelContextBadge } from "./providerModelContext";
it("keeps catalog capacity when selected rows contain only an ID", () => {
  const rich = {
    model: "relay/model",
    contextWindow: 1_048_576,
    displayName: "Model",
  };
  expect(mergeModelMetadata(rich, { model: "relay/model" })).toEqual(rich);
  expect(mergeModelMetadata(rich, "relay/model")).toEqual(rich);
});
it("preserves explicit selected capacity and preferences", () => {
  const prefs = { name: "Custom" };
  expect(
    mergeModelMetadata(
      { contextWindow: 2_000_000 },
      { model: "m", context_window: 128_000, modelPreferences: prefs },
    ),
  ).toMatchObject({ contextWindow: 128_000, modelPreferences: prefs });
});
it.each([
  [1_000_000, "Model", "1M"],
  [1_048_576, "Model", "1.0M"],
  [2_000_000, "Model", "2M"],
  [128_000, "Model", ""],
  [undefined, "Model", ""],
  [1_000_000, "Model (1M)", ""],
])("matches Magpie context tags for %s / %s", (n, name, badge) => {
  expect(modelContextBadge(n, name)).toBe(badge);
});
