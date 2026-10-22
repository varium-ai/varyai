import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";

vi.mock("@/lib/api/env", () => ({
  deleteEnvVars: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    warning: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
  },
}));

describe("EnvWarningBanner", () => {
  it("renders as a compact bottom-centered floating card", () => {
    render(
      <EnvWarningBanner
        conflicts={[
          {
            varName: "ANTHROPIC_API_KEY",
            varValue: "secret",
            sourceType: "system",
            sourcePath: "~/.profile:12",
          },
        ]}
        onDismiss={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );

    const banner = screen.getByTestId("env-warning-banner");
    expect(banner).toHaveClass("fixed", "inset-x-0", "bottom-20");
    expect(banner).not.toHaveClass("top-0");
    expect(screen.getByTestId("env-warning-card")).toHaveClass(
      "max-w-2xl",
      "bg-amber-50/90",
      "border-amber-200/80",
    );
    expect(screen.getByTestId("env-warning-card")).not.toHaveClass(
      "border-l-2",
    );
  });
});
