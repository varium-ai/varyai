import { expect, it } from "vitest";
import { modelTells } from "./modelTells";

it.each([
  [
    ["gs/vendor/glm-5.3", "gs-2/vendor/glm-5.3"],
    ["gs", "gs-2"],
  ],
  [
    ["magpie/gs/vendor/glm-5.3", "magpie/gs-2/vendor/glm-5.3"],
    ["gs", "gs-2"],
  ],
  [
    ["gs/vendor/old", "gs/vendor/new"],
    ["old", "new"],
  ],
  [
    ["a", "a/a"],
    ["a", "a/a"],
  ],
])("uses Magpie's distinguishing ID segments for %j", (ids, expected) => {
  const tells = modelTells(
    ids,
    () => "Shared name",
    (id) => id,
  );
  expect(ids.map((id) => tells.get(id))).toEqual(expected);
});

it("does not invent distinctions between repeated references to the same model", () => {
  const tells = modelTells(
    ["gs/vendor/model", "gs/vendor/model"],
    () => "Shared name",
    (id) => id,
  );
  expect(tells.size).toBe(0);
});
