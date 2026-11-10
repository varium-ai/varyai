import { expect, it } from "vitest";
import fixtures from "../../tests/fixtures/provider-image-models.json";
import { isImageGenerationModel } from "./providerModelKind";

it.each(fixtures)(
  "classifies generation separately from image input: $value",
  ({ value, image }) => {
    expect(isImageGenerationModel(value)).toBe(image);
  },
);
