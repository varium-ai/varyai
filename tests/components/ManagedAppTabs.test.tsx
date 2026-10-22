import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { ManagedAppTabs } from "@/components/home/ManagedAppTabs";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import zh from "@/i18n/locales/zh.json";

vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));

const storageKey = "cc-switch-managed-app-order";
const visibleApps = {
  ...DEFAULT_VISIBLE_APPS,
  claude: false,
  opencode: false,
  openclaw: false,
  hermes: false,
  pi: false,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  grokbuild: true,
};

function tabNames() {
  return screen
    .getAllByRole("button")
    .filter((tab) => !tab.getAttribute("aria-label")?.startsWith("拖动排序 "))
    .map((tab) => tab.textContent);
}

beforeEach(() => {
  localStorage.removeItem(storageKey);
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  // Give the actual dnd-kit sensors a horizontal layout in jsdom.
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const tab = this.closest("[data-app-tab]") ?? this;
      const tabs = Array.from(
        tab.parentElement?.querySelectorAll("[data-app-tab]") ?? [],
      );
      const index = Math.max(0, tabs.indexOf(tab));
      const preview = this.querySelector<HTMLElement>(
        '[data-testid="managed-app-drag-preview"]',
      );
      const left = preview ? parseFloat(this.style.left) || 0 : index * 120;
      const width = preview ? parseFloat(preview.style.width) : 120;
      return {
        x: left,
        y: 0,
        left,
        top: 0,
        right: left + width,
        bottom: 36,
        width,
        height: 36,
        toJSON: () => ({}),
      };
    },
  );
  vi.stubGlobal(
    "PointerEvent",
    class extends MouseEvent {
      pointerId = 1;
      isPrimary = true;
      pointerType = "mouse";
    },
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.removeItem(storageKey);
});

function renderTabs(onAppChange = vi.fn()) {
  return render(
    <ManagedAppTabs
      activeApp="codex"
      visibleApps={visibleApps}
      onAppChange={onAppChange}
    />,
  );
}

describe("managed app sorting", () => {
  it("only starts dragging from the handle and keeps handle clicks from switching apps", async () => {
    const onAppChange = vi.fn();
    renderTabs(onAppChange);
    const tab = screen.getByRole("button", { name: "Codex" });
    fireEvent.pointerDown(tab, { button: 0, clientX: 180, clientY: 18 });
    fireEvent.pointerMove(document, { clientX: 300, clientY: 18 });
    fireEvent.pointerUp(document, { clientX: 300, clientY: 18 });
    expect(
      screen.queryByTestId("managed-app-drag-preview"),
    ).not.toBeInTheDocument();
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(tabNames()).toEqual([
      "Claude Desktop",
      "Codex",
      "Gemini",
      "Grok Build",
    ]);
    await userEvent.click(
      screen.getByRole("button", { name: "拖动排序 Gemini" }),
    );
    expect(onAppChange).not.toHaveBeenCalled();
  });
  it("prevents native image dragging from cancelling pointer sorting", () => {
    renderTabs();
    const label = screen
      .getByRole("button", { name: "Codex" })
      .querySelector("span")!;
    expect(fireEvent.dragStart(label)).toBe(false);
  });
  it("keeps click switching and the selected app", async () => {
    const onAppChange = vi.fn();
    renderTabs(onAppChange);
    await userEvent.click(screen.getByRole("button", { name: "Gemini" }));
    expect(onAppChange).toHaveBeenCalledWith("gemini");
    expect(screen.getByRole("button", { name: "Codex" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(localStorage.getItem(storageKey)).toBeNull();
  });

  it("drags a tab to a new position, persists it, and does not switch apps", async () => {
    const onAppChange = vi.fn();
    const { unmount } = renderTabs(onAppChange);
    fireEvent.pointerDown(
      screen.getByRole("button", { name: "拖动排序 Codex" }),
      {
        button: 0,
        clientX: 180,
        clientY: 18,
      },
    );
    fireEvent.pointerMove(document, { clientX: 190, clientY: 18 });
    fireEvent.pointerMove(document, { clientX: 300, clientY: 18 });
    fireEvent.pointerUp(document, { clientX: 300, clientY: 18 });
    await waitFor(() =>
      expect(tabNames()).toEqual([
        "Claude Desktop",
        "Gemini",
        "Codex",
        "Grok Build",
      ]),
    );
    expect(onAppChange).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem(storageKey)!)).toContain("claude");
    unmount();
    renderTabs();
    expect(tabNames()).toEqual([
      "Claude Desktop",
      "Gemini",
      "Codex",
      "Grok Build",
    ]);
  });

  it("does not start sorting from the keyboard", async () => {
    renderTabs();
    screen.getByRole("button", { name: "Codex" }).focus();
    await userEvent.keyboard("[Space][ArrowLeft][Space]");
    expect(tabNames()).toEqual([
      "Claude Desktop",
      "Codex",
      "Gemini",
      "Grok Build",
    ]);
    expect(localStorage.getItem(storageKey)).toBeNull();
  });

  it("keeps the drag preview at its original width over a wider tab", async () => {
    vi.mocked(HTMLElement.prototype.getBoundingClientRect).mockImplementation(
      function (this: HTMLElement) {
        const tab = this.closest("[data-app-tab]") ?? this;
        const tabs = Array.from(
          tab.parentElement?.querySelectorAll("[data-app-tab]") ?? [],
        );
        const index = Math.max(0, tabs.indexOf(tab));
        const widths = [200, 100, 120, 140];
        const left = widths
          .slice(0, index)
          .reduce((sum, width) => sum + width, 0);
        const width = widths[index] ?? 120;
        return {
          x: left,
          y: 0,
          left,
          top: 0,
          right: left + width,
          bottom: 36,
          width,
          height: 36,
          toJSON: () => ({}),
        };
      },
    );
    renderTabs();
    fireEvent.pointerDown(
      screen.getByRole("button", { name: "拖动排序 Codex" }),
      {
        button: 0,
        clientX: 250,
        clientY: 18,
      },
    );
    fireEvent.pointerMove(document, { clientX: 240, clientY: 18 });
    fireEvent.pointerMove(document, { clientX: 100, clientY: 18 });
    const preview = screen.getByTestId("managed-app-drag-preview");
    expect(preview).toHaveStyle({ width: "100px" });
    expect(preview).toHaveTextContent("Codex");
    expect(preview.parentElement?.style.transform).not.toMatch(
      /scale[XY]\((?!1\))/,
    );
    expect(preview.closest(".overflow-x-auto")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Codex" }).closest("[data-app-tab]"),
    ).toHaveClass("opacity-0");
    await act(async () => {
      fireEvent.pointerCancel(document);
    });
    expect(
      screen.queryByTestId("managed-app-drag-preview"),
    ).not.toBeInTheDocument();
  });

  it("cancels a drag without saving an order or switching apps", async () => {
    const onAppChange = vi.fn();
    renderTabs(onAppChange);
    fireEvent.pointerDown(
      screen.getByRole("button", { name: "拖动排序 Codex" }),
      {
        button: 0,
        clientX: 180,
        clientY: 18,
      },
    );
    fireEvent.pointerMove(document, { clientX: 170, clientY: 18 });
    fireEvent.pointerMove(document, { clientX: 60, clientY: 18 });
    await act(async () => {
      fireEvent.pointerCancel(document);
    });
    expect(tabNames()).toEqual([
      "Claude Desktop",
      "Codex",
      "Gemini",
      "Grok Build",
    ]);
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(onAppChange).not.toHaveBeenCalled();
  });

  it("restores saved positions when apps are hidden and re-enabled, and appends new apps", () => {
    localStorage.setItem(
      storageKey,
      JSON.stringify(["gemini", "codex", "grokbuild", "claude-desktop"]),
    );
    const { rerender } = renderTabs();
    expect(tabNames()).toEqual([
      "Gemini",
      "Codex",
      "Grok Build",
      "Claude Desktop",
    ]);
    rerender(
      <ManagedAppTabs
        activeApp="codex"
        visibleApps={{ ...visibleApps, gemini: false }}
        onAppChange={vi.fn()}
      />,
    );
    expect(tabNames()).toEqual(["Codex", "Grok Build", "Claude Desktop"]);
    rerender(
      <ManagedAppTabs
        activeApp="codex"
        visibleApps={{ ...visibleApps, workbuddy: true }}
        onAppChange={vi.fn()}
      />,
    );
    expect(tabNames()).toEqual([
      "Gemini",
      "Codex",
      "Grok Build",
      "Claude Desktop",
      "WorkBuddy",
    ]);
  });

  it.each(["invalid json", "{}", "null"])(
    "falls back to defaults for corrupt saved data: %s",
    (saved) => {
      localStorage.setItem(storageKey, saved);
      renderTabs();
      expect(tabNames()).toEqual([
        "Claude Desktop",
        "Codex",
        "Gemini",
        "Grok Build",
      ]);
    },
  );

  it("ignores unknown and duplicate app IDs without dropping valid apps", () => {
    localStorage.setItem(
      storageKey,
      JSON.stringify(["gemini", "removed-app", "gemini", 1, "codex"]),
    );
    renderTabs();
    expect(tabNames()).toEqual([
      "Gemini",
      "Codex",
      "Claude Desktop",
      "Grok Build",
    ]);
  });

  it("still reorders when saving to local storage fails", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("Storage unavailable");
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    renderTabs();
    fireEvent.pointerDown(
      screen.getByRole("button", { name: "拖动排序 Codex" }),
      {
        button: 0,
        clientX: 180,
        clientY: 18,
      },
    );
    fireEvent.pointerMove(document, { clientX: 170, clientY: 18 });
    fireEvent.pointerMove(document, { clientX: 60, clientY: 18 });
    await act(async () => {
      fireEvent.pointerUp(document, { clientX: 60, clientY: 18 });
    });
    expect(tabNames()[0]).toBe("Codex");
  });

  it("uses the default visibility when settings omit an app", () => {
    render(
      <ManagedAppTabs
        activeApp="codex"
        visibleApps={{ ...DEFAULT_VISIBLE_APPS, workbuddy: undefined }}
        onAppChange={vi.fn()}
      />,
    );
    expect(tabNames()).toHaveLength(
      Object.values(DEFAULT_VISIBLE_APPS).filter(Boolean).length,
    );
    expect(
      screen.queryByRole("button", { name: "WorkBuddy" }),
    ).not.toBeInTheDocument();
  });
});
