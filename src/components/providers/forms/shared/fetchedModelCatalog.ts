import type { Dispatch, SetStateAction } from "react";
import type { FetchedModel } from "@/lib/api/model-fetch";

export type FetchedModelCatalog = readonly [
  FetchedModel[],
  Dispatch<SetStateAction<FetchedModel[]>>,
];
