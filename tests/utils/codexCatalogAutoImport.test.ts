import { describe, expect, it } from "vitest";
import { computeAutoImportCatalogModels } from "@/components/providers/forms/helpers/codexCatalogAutoImport";

describe("Codex catalog capacity import", () => {
  it("preserves vendor facts and explicit existing overrides", () => {
    const existing = {
      model: "existing",
      contextWindow: 500000,
      contextSource: "user" as const,
    };
    const result = computeAutoImportCatalogModels(
      [existing],
      [
        { id: "existing", ownedBy: null, context_window: 128000 },
        {
          id: "fresh",
          ownedBy: null,
          context_window: 1000000,
          max_context_window: 1200000,
          effective_context_window_percent: 95,
          auto_compact_token_limit: 272000,
          reasoningLevels: ["low", "high"],
          reasoning: true,
          inputModalities: ["text", "image"],
        },
        { id: "unknown", ownedBy: null },
      ],
    );
    expect(result.nextCatalogModels).toEqual([
      existing,
      {
        model: "fresh",
        displayName: "fresh",
        contextWindow: 1000000,
        contextSource: "vendor",
        maxContextWindow: 1200000,
        effectiveContextWindowPercent: 95,
        autoCompactTokenLimit: 272000,
        reasoningLevels: ["low", "high"],
        reasoning: true,
        inputModalities: ["text", "image"],
      },
      { model: "unknown", displayName: "unknown" },
    ]);
  });
});
