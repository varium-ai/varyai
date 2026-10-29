import { render, screen, within } from "@testing-library/react";
import { it, expect, vi } from "vitest";
import type { ReactNode } from "react";
import { UsageHero } from "@/components/usage/UsageHero";

const usage = vi.hoisted(() => ({ app: "codex", creation: 12 }));
vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  },
}));
vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: () => ({
    isLoading: false,
    data: [
      {
        appType: usage.app,
        summary: {
          totalRequests: 1,
          totalCost: "0",
          totalInputTokens: 100,
          totalOutputTokens: 10,
          totalCacheCreationTokens: usage.creation,
          totalCacheReadTokens: 0,
          successRate: 100,
          realTotalTokens: 122,
          cacheHitRate: 0,
        },
      },
    ],
  }),
}));

it.each(["codex", "gemini"])("shows reported cache creation for %s", (app) => {
  usage.app = app;
  usage.creation = 12;
  render(
    <UsageHero
      appType={app}
      range={{ preset: "today" }}
      refreshIntervalMs={0}
    />,
  );
  const card = screen.getByText("缓存写入").parentElement!.parentElement!;
  expect(within(card).getByText("12")).toBeVisible();
  expect(screen.queryByText("N/A")).not.toBeInTheDocument();
});

it("explains missing cache creation instead of displaying N/A or measured zero", () => {
  usage.app = "codex";
  usage.creation = 0;
  render(
    <UsageHero
      appType="codex"
      range={{ preset: "today" }}
      refreshIntervalMs={0}
    />,
  );
  expect(screen.getByText("usage.cacheCreationNotReported")).toBeVisible();
  expect(screen.queryByText("N/A")).not.toBeInTheDocument();
});
