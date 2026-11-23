import { act, fireEvent, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { showAppMessage } from "@/lib/appMessage";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.stubGlobal(
  "matchMedia",
  vi.fn((media: string) => ({
    matches: false,
    media,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
);
afterEach(() => vi.unstubAllGlobals());

it("requires acknowledgement before resolving and removes the themed dialog afterward", async () => {
  let settled = false;
  const completion = showAppMessage({
    title: "Fixture configuration error",
    description: "Synthetic diagnostic",
    confirmLabel: "Confirm",
  }).then(() => {
    settled = true;
  });
  const dialog = await screen.findByRole("dialog", {
    name: "Fixture configuration error",
  });
  expect(dialog).toHaveClass("vary-dialog-surface");
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(settled).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await act(async () => completion);
  expect(settled).toBe(true);
  expect(
    screen.queryByRole("dialog", { name: "Fixture configuration error" }),
  ).not.toBeInTheDocument();
});
