import type { CodexCatalogModel } from "@/types";
import type { FetchedModel } from "@/lib/api/model-fetch";

export const MAX_AUTO_IMPORT_CATALOG_MODELS = 10;

export interface AutoImportCatalogResult {
  nextCatalogModels: CodexCatalogModel[];
  importedCount: number;
  totalValidCount: number;
  suggestedDefaultModel?: string;
}

export function computeAutoImportCatalogModels(
  currentCatalogModels: CodexCatalogModel[],
  fetchedModels: FetchedModel[],
  currentDefaultModel: string = "",
  maxQuota: number = MAX_AUTO_IMPORT_CATALOG_MODELS,
): AutoImportCatalogResult {
  const validModels: FetchedModel[] = [];
  const seenFetchedIds = new Set<string>();

  for (const m of fetchedModels) {
    const id = m.id ? m.id.trim() : "";
    if (id && !seenFetchedIds.has(id)) {
      seenFetchedIds.add(id);
      validModels.push({ ...m, id });
    }
  }

  const existingRows = currentCatalogModels.filter(
    (row) =>
      (row.model && row.model.trim().length > 0) ||
      (row.displayName && row.displayName.trim().length > 0),
  );

  const existingModelIds = new Set(
    existingRows
      .map((row) => (row.model ? row.model.trim() : ""))
      .filter((id) => id.length > 0),
  );

  const remainingQuota = Math.max(0, maxQuota - existingRows.length);
  const newModelsToAdd =
    remainingQuota > 0
      ? validModels
          .filter((m) => !existingModelIds.has(m.id))
          .slice(0, remainingQuota)
      : [];

  const createdRows: CodexCatalogModel[] = newModelsToAdd.map((m) => ({
    model: m.id,
    displayName: m.id,
  }));

  const nextCatalogModels: CodexCatalogModel[] = [
    ...existingRows,
    ...createdRows,
  ];

  let suggestedDefaultModel = currentDefaultModel.trim();
  if (!suggestedDefaultModel) {
    if (existingRows.length > 0 && existingRows[0].model.trim()) {
      suggestedDefaultModel = existingRows[0].model.trim();
    } else if (newModelsToAdd.length > 0) {
      suggestedDefaultModel = newModelsToAdd[0].id;
    } else if (validModels.length > 0) {
      suggestedDefaultModel = validModels[0].id;
    }
  }

  return {
    nextCatalogModels,
    importedCount: createdRows.length,
    totalValidCount: validModels.length,
    suggestedDefaultModel: suggestedDefaultModel || undefined,
  };
}
