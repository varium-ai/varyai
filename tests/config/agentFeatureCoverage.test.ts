import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import { APP_IDS } from "@/config/appConfig";

it("keeps every feature coverage matrix aligned with the full Agent inventory", () => {
  const ledger = readFileSync("docs/qa/agent-feature-coverage.md", "utf8");
  const backend = readFileSync(
    "src-tauri/src/services/agent_connections.rs",
    "utf8",
  ).match(/const APPS: [^=]+?= &\[([\s\S]*?)\];/);
  expect(backend).not.toBeNull();
  const inventory = new Set([
    ...APP_IDS,
    ...Array.from(backend![1].matchAll(/"([\w-]+)"/g), ([, id]) => id),
  ]);
  const matrices = ledger
    .split(/^## /m)
    .filter((section) => /\|\s*Agent ID\s*\|/.test(section));
  expect(matrices.length).toBeGreaterThan(0);
  for (const matrix of matrices) {
    const rows = Array.from(
      matrix.matchAll(/^\|\s*`([^`]+)`\s*\|/gm),
      ([, id]) => id,
    );
    expect(rows.sort(), matrix.split("\n")[0]).toEqual([...inventory].sort());
  }
});
