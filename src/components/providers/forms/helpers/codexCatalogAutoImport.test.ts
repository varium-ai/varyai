import { describe, expect, it } from "vitest";
import { computeAutoImportCatalogModels } from "./codexCatalogAutoImport";
import type { CodexCatalogModel } from "@/types";
import type { FetchedModel } from "@/lib/api/model-fetch";

describe("computeAutoImportCatalogModels", () => {
  it("imports all models when the current catalog is empty", () => {
    const fetched: FetchedModel[] = [
      { id: "model-1", ownedBy: "test" },
      { id: "model-2", ownedBy: "test" },
      { id: "model-3", ownedBy: "test" },
    ];

    const result = computeAutoImportCatalogModels([], fetched, "");

    expect(result.totalValidCount).toBe(3);
    expect(result.importedCount).toBe(3);
    expect(result.nextCatalogModels).toEqual([
      { model: "model-1", displayName: "model-1" },
      { model: "model-2", displayName: "model-2" },
      { model: "model-3", displayName: "model-3" },
    ]);
    expect(result.suggestedDefaultModel).toBe("model-1");
  });

  it("imports all models when the supplier returns more than ten", () => {
    const fetched: FetchedModel[] = Array.from({ length: 15 }, (_, i) => ({
      id: `model-${i + 1}`,
      ownedBy: "test",
    }));

    const result = computeAutoImportCatalogModels([], fetched, "");

    expect(result.totalValidCount).toBe(15);
    expect(result.importedCount).toBe(15);
    expect(result.nextCatalogModels).toHaveLength(15);
    expect(result.nextCatalogModels[0]).toEqual({
      model: "model-1",
      displayName: "model-1",
    });
    expect(result.nextCatalogModels[14]).toEqual({
      model: "model-15",
      displayName: "model-15",
    });
    expect(result.suggestedDefaultModel).toBe("model-1");
  });

  it("preserves existing rows and deduplicates fetched models", () => {
    const current: CodexCatalogModel[] = [
      {
        model: "custom-a",
        displayName: "My Custom A",
        contextWindow: 64000,
      },
      {
        model: "model-2",
        displayName: "Already Configured Model 2",
      },
    ];

    const fetched: FetchedModel[] = [
      { id: "model-1", ownedBy: "test" },
      { id: "model-2", ownedBy: "test" }, // duplicate of existing
      { id: "model-3", ownedBy: "test" },
      { id: "model-4", ownedBy: "test" },
    ];

    const result = computeAutoImportCatalogModels(current, fetched, "custom-a");

    expect(result.totalValidCount).toBe(4);
    expect(result.importedCount).toBe(3);
    expect(result.nextCatalogModels).toHaveLength(5);
    expect(result.nextCatalogModels[0]).toEqual(current[0]);
    expect(result.nextCatalogModels[1]).toEqual(current[1]);
    expect(result.nextCatalogModels[2]).toEqual({
      model: "model-1",
      displayName: "model-1",
    });
    expect(result.nextCatalogModels[3]).toEqual({
      model: "model-3",
      displayName: "model-3",
    });
    expect(result.nextCatalogModels[4]).toEqual({
      model: "model-4",
      displayName: "model-4",
    });
    // Already has default model "custom-a", should not override
    expect(result.suggestedDefaultModel).toBe("custom-a");
  });

  it("still imports new models when the catalog already has ten rows", () => {
    const current: CodexCatalogModel[] = Array.from({ length: 10 }, (_, i) => ({
      model: `existing-${i + 1}`,
      displayName: `Existing ${i + 1}`,
    }));

    const fetched: FetchedModel[] = [
      { id: "model-1", ownedBy: "test" },
      { id: "model-2", ownedBy: "test" },
    ];

    const result = computeAutoImportCatalogModels(
      current,
      fetched,
      "existing-1",
    );

    expect(result.importedCount).toBe(2);
    expect(result.nextCatalogModels).toHaveLength(12);
    expect(result.nextCatalogModels.slice(0, 10)).toEqual(current);
  });

  it("ignores invalid or empty model ids from fetched models", () => {
    const fetched: FetchedModel[] = [
      { id: "  ", ownedBy: "test" },
      { id: "valid-1", ownedBy: "test" },
      { id: "", ownedBy: "test" },
    ];

    const result = computeAutoImportCatalogModels([], fetched, "");

    expect(result.totalValidCount).toBe(1);
    expect(result.importedCount).toBe(1);
    expect(result.nextCatalogModels).toEqual([
      { model: "valid-1", displayName: "valid-1" },
    ]);
  });

  it("filters out empty placeholder rows from existing catalog", () => {
    const current: CodexCatalogModel[] = [
      { model: "", displayName: "" },
      { model: "valid-existing", displayName: "Valid Existing" },
    ];
    const fetched: FetchedModel[] = [{ id: "model-1", ownedBy: "test" }];

    const result = computeAutoImportCatalogModels(current, fetched, "");

    expect(result.importedCount).toBe(1);
    expect(result.nextCatalogModels).toEqual([
      { model: "valid-existing", displayName: "Valid Existing" },
      { model: "model-1", displayName: "model-1" },
    ]);
  });
});
