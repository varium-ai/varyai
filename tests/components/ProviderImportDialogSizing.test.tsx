import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import postcss from "postcss";
import { expect, it } from "vitest";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";

const providerSheet = postcss.parse(
  readFileSync("src/components/providers/providers.css", "utf8"),
);
const providerRules = providerSheet.nodes
  .filter((node) => node.type === "rule")
  .map((node) => node.toString())
  .join("\n");
const sharedRules = readFileSync("src/components/ui/dialog.css", "utf8");

function topLevelDecls(selector: string) {
  const decls = new Map<string, string>();
  for (const node of providerSheet.nodes) {
    if (
      node.type === "rule" &&
      node.selector
        .split(",")
        .map((part) => part.trim())
        .includes(selector)
    ) {
      node.walkDecls((decl) => {
        decls.set(decl.prop, decl.value);
      });
    }
  }
  return decls;
}

it.each([true, false])(
  "keeps the importer compact regardless of shared dialog CSS order: %s",
  (sharedLast) => {
    render(
      <Dialog open>
        <DialogContent id="fixture-import-dialog" className="pv-import-dialog">
          <DialogTitle>Import providers</DialogTitle>
          <DialogDescription>Synthetic import sizing fixture</DialogDescription>
        </DialogContent>
      </Dialog>,
    );
    const sheet = document.createElement("style");
    sheet.textContent = (
      sharedLast ? [providerRules, sharedRules] : [sharedRules, providerRules]
    ).join("\n");
    document.head.appendChild(sheet);
    try {
      const maximum = Number.parseFloat(
        getComputedStyle(screen.getByRole("dialog")).maxWidth,
      );
      expect(maximum).toBeGreaterThan(0);
      expect(maximum).toBeLessThanOrEqual(620);
    } finally {
      sheet.remove();
    }
  },
);

it("keeps a fixed viewport-clamped height for import and picker dialogs", () => {
  const importDialog = topLevelDecls(
    '.pv-import-dialog.vary-dialog-surface[role="dialog"]',
  );
  expect(importDialog.get("height")).toBe("min(580px, calc(100dvh - 48px))");
  expect(importDialog.get("max-height")).toBe(
    "min(580px, calc(100dvh - 48px))",
  );

  const importList = topLevelDecls(".pv-import-dialog .pv-import-list");
  expect(importList.get("display")).toBe("flex");
  expect(importList.get("flex-direction")).toBe("column");
  expect(importList.get("flex")).toBe("1 1 0");
  expect(importList.get("min-height")).toBe("0");
  expect(importList.get("max-height")).toBe("none");
  expect(importList.get("overflow-y")).toBe("auto");

  const importNonEmpty = topLevelDecls(
    ".pv-import-dialog .pv-import-list > :not(.search-empty-state)",
  );
  expect(importNonEmpty.get("flex")).toBe("none");

  const importEmptyState = topLevelDecls(
    ".pv-import-dialog .pv-import-list > .search-empty-state",
  );
  expect(importEmptyState.get("flex")).toBe("1 1 auto");
  expect(importEmptyState.get("align-content")).toBe("center");

  const importEmpty = topLevelDecls(".pv-import-dialog > .pv-import-empty");
  expect(importEmpty.get("flex")).toBe("1 1 0");
  expect(importEmpty.get("min-height")).toBe("0");

  const importFooter = topLevelDecls(".pv-import-footer");
  expect(importFooter.get("margin-top")).toBe("auto");

  const importFooterBtn = topLevelDecls(".pv-import-footer button");
  expect(importFooterBtn.get("height")).toBe("var(--button-height)");

  const pickerDialog = topLevelDecls(
    '.pv-picker-dialog.vary-dialog-surface[role="dialog"]',
  );
  expect(pickerDialog.get("height")).toBe("min(520px, calc(100dvh - 48px))");
  expect(pickerDialog.get("max-height")).toBe(
    "min(520px, calc(100dvh - 48px))",
  );

  const pickerBody = topLevelDecls(".pv-picker-dialog .pv-picker-body");
  expect(pickerBody.get("flex")).toBe("1 1 0");
  expect(pickerBody.get("min-height")).toBe("0");
  expect(pickerBody.get("overflow-y")).toBe("auto");
});
