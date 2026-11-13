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

const providerRules = postcss
  .parse(readFileSync("src/components/providers/providers.css", "utf8"))
  .nodes.filter((node) => node.type === "rule")
  .map((node) => node.toString())
  .join("\n");
const sharedRules = readFileSync("src/components/ui/dialog.css", "utf8");

it.each([true, false])(
  "caps provider width when shared dialog CSS loads last: %s",
  (sharedLast) => {
    render(
      <Dialog open>
        <DialogContent className="mp-dialog pv-provider-dialog">
          <DialogTitle>Synthetic provider</DialogTitle>
          <DialogDescription>Provider form sizing fixture</DialogDescription>
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
      expect(maximum).toBeLessThanOrEqual(560);
    } finally {
      sheet.remove();
    }
  },
);
