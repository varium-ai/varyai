import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { Toaster } from "@/components/ui/sonner";

const themeState = vi.hoisted(() => ({ theme: "light" }));
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => themeState,
}));

beforeEach(() => {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  );
});

afterEach(() => {
  act(() => {
    toast.dismiss();
  });
  vi.unstubAllGlobals();
});

describe.each(["light", "dark"])("Toaster in %s mode", (theme) => {
  it.each(["error", "warning", "success", "info"] as const)(
    "renders %s notifications with semantic colors by default",
    async (type) => {
      themeState.theme = theme;
      render(<Toaster />);
      const title = `${theme}-${type}`;
      act(() => {
        toast[type](title, { description: "Details", closeButton: true });
      });
      const notification = (await screen.findByText(title)).closest(
        "[data-sonner-toast]",
      );

      expect(notification).toHaveAttribute("data-type", type);
      expect(notification).toHaveAttribute("data-rich-colors", "true");
      expect(notification).toHaveAttribute("data-styled", "true");
      await waitFor(() => {
        expect(notification?.closest("[data-sonner-toaster]")).toHaveAttribute(
          "data-sonner-theme",
          theme,
        );
      });
    },
  );
});
