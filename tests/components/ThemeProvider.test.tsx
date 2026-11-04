import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ThemeProvider, useTheme } from "@/components/theme-provider";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => {}) }));

let systemDark = false;
let changeSystemTheme: (() => void) | undefined;

function ThemeValue() {
  return <output>{useTheme().theme}</output>;
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("light", "dark");
  systemDark = false;
  changeSystemTheme = undefined;
  vi.stubGlobal("matchMedia", () => ({
    get matches() {
      return systemDark;
    },
    addEventListener: (_: string, listener: () => void) => {
      changeSystemTheme = listener;
    },
    removeEventListener: vi.fn(),
  }));
});

afterEach(() => {
  document.documentElement.classList.remove("light", "dark");
  vi.unstubAllGlobals();
});

it("applies the saved dark theme to a newly opened window", () => {
  localStorage.setItem("cc-switch-theme", "dark");
  render(
    <ThemeProvider>
      <ThemeValue />
    </ThemeProvider>,
  );
  expect(screen.getByRole("status")).toHaveTextContent("dark");
  expect(document.documentElement).toHaveClass("dark");
  expect(invoke).toHaveBeenCalledWith("set_window_theme", { theme: "dark" });
});

it("follows theme changes made by another application window", async () => {
  render(
    <ThemeProvider defaultTheme="light">
      <ThemeValue />
    </ThemeProvider>,
  );
  for (const theme of ["dark", "light", "system"]) {
    act(() => {
      localStorage.setItem("cc-switch-theme", theme);
      window.dispatchEvent(
        new StorageEvent("storage", {
          key: "cc-switch-theme",
          newValue: theme,
          storageArea: localStorage,
        }),
      );
    });
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent(theme);
      expect(document.documentElement).toHaveClass(
        theme === "dark" ? "dark" : "light",
      );
      expect(invoke).toHaveBeenCalledWith("set_window_theme", { theme });
    });
  }
  act(() => {
    systemDark = true;
    changeSystemTheme?.();
  });
  expect(document.documentElement).toHaveClass("dark");
});

it("ignores unrelated storage events and unsupported theme values", () => {
  render(
    <ThemeProvider defaultTheme="dark">
      <ThemeValue />
    </ThemeProvider>,
  );
  act(() => {
    for (const [key, newValue] of [
      ["other-preference", "light"],
      ["cc-switch-theme", "invalid"],
    ]) {
      window.dispatchEvent(
        new StorageEvent("storage", {
          key,
          newValue,
          storageArea: localStorage,
        }),
      );
    }
  });
  expect(screen.getByRole("status")).toHaveTextContent("dark");
  expect(document.documentElement).toHaveClass("dark");
});
