import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LocalProxySettings } from "@/components/settings/LocalProxySettings";
import type { SettingsFormState } from "@/hooks/useSettings";

const { startProxyServer, stopWithRestore, setTakeoverForApp } = vi.hoisted(
  () => ({
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    setTakeoverForApp: vi.fn(),
  }),
);

vi.mock("@/components/proxy", () => ({
  ProxyPanel: (props: {
    onToggleProxy: (enabled: boolean) => Promise<void>;
    isProxyPending: boolean;
  }) => (
    <div
      data-testid="proxy-panel"
      data-props={Object.keys(props).sort().join(",")}
    >
      <button
        disabled={props.isProxyPending}
        onClick={() => void props.onToggleProxy(true)}
      >
        Enable routing
      </button>
      <button
        disabled={props.isProxyPending}
        onClick={() => void props.onToggleProxy(false)}
      >
        Disable routing
      </button>
    </div>
  ),
}));

vi.mock("@/components/ui/accordion", () => ({
  Accordion: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  AccordionContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  AccordionItem: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  AccordionTrigger: ({ children }: { children: React.ReactNode }) => (
    <button type="button">{children}</button>
  ),
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: false,
    startProxyServer,
    stopWithRestore,
    setTakeoverForApp,
    isPending: false,
  }),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    onConfirm,
  }: {
    isOpen: boolean;
    onConfirm: () => void;
  }) => (isOpen ? <button onClick={onConfirm}>Confirm routing</button> : null),
}));

describe("LocalProxySettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    startProxyServer.mockResolvedValue(undefined);
    stopWithRestore.mockResolvedValue(undefined);
    setTakeoverForApp.mockReset().mockResolvedValue(undefined);
  });

  it.each([true, false])(
    "attempts every application when the master switch is %s, even after an application fails",
    async (enabled) => {
      const operations: string[] = [];
      startProxyServer.mockImplementation(async () => {
        operations.push("start");
      });
      stopWithRestore.mockImplementation(async () => {
        operations.push("stop");
      });
      setTakeoverForApp.mockImplementation(
        async ({ appType }: { appType: string }) => {
          operations.push(appType);
          if (appType === "codex") throw new Error("Codex config unavailable");
        },
      );

      render(
        <LocalProxySettings
          settings={{ proxyConfirmed: true } as SettingsFormState}
          onAutoSave={vi.fn()}
        />,
      );
      fireEvent.click(
        screen.getByText(enabled ? "Enable routing" : "Disable routing"),
      );

      await waitFor(() => expect(setTakeoverForApp).toHaveBeenCalledTimes(11));
      expect(
        setTakeoverForApp.mock.calls.map(([variables]) => variables),
      ).toEqual([
        { appType: "claude", enabled },
        { appType: "codex", enabled },
        { appType: "gemini", enabled },
        { appType: "grokbuild", enabled },
        { appType: "workbuddy", enabled },
        { appType: "qoder", enabled },
        { appType: "trae", enabled },
        { appType: "kimi", enabled },
        { appType: "qwen", enabled },
        { appType: "zcode", enabled },
        { appType: "pi", enabled },
      ]);
      expect(operations).toEqual(
        enabled
          ? [
              "start",
              "claude",
              "codex",
              "gemini",
              "grokbuild",
              "workbuddy",
              "qoder",
              "trae",
              "kimi",
              "qwen",
              "zcode",
              "pi",
            ]
          : [
              "claude",
              "codex",
              "gemini",
              "grokbuild",
              "workbuddy",
              "qoder",
              "trae",
              "kimi",
              "qwen",
              "zcode",
              "pi",
              "stop",
            ],
      );
      await waitFor(() =>
        expect(screen.getByText("Enable routing")).toBeEnabled(),
      );
    },
  );

  it("enables every application after the initial confirmation", async () => {
    const onAutoSave = vi.fn().mockResolvedValue(true);
    render(
      <LocalProxySettings
        settings={{} as SettingsFormState}
        onAutoSave={onAutoSave}
      />,
    );
    fireEvent.click(screen.getByText("Enable routing"));
    expect(startProxyServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Confirm routing"));

    await waitFor(() => expect(setTakeoverForApp).toHaveBeenCalledTimes(11));
    expect(onAutoSave).toHaveBeenCalledWith({ proxyConfirmed: true });
    expect(startProxyServer).toHaveBeenCalledOnce();
  });

  it("keeps the master switch disabled until all applications have finished", async () => {
    let finishLastApp!: () => void;
    setTakeoverForApp.mockImplementation(({ appType }: { appType: string }) =>
      appType === "pi"
        ? new Promise<void>((resolve) => {
            finishLastApp = resolve;
          })
        : Promise.resolve(),
    );
    render(
      <LocalProxySettings
        settings={{ proxyConfirmed: true } as SettingsFormState}
        onAutoSave={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByText("Enable routing"));
    await waitFor(() => expect(setTakeoverForApp).toHaveBeenCalledTimes(11));
    expect(screen.getByText("Enable routing")).toBeDisabled();
    expect(screen.getByText("Disable routing")).toBeDisabled();
    await act(async () => finishLastApp());
    await waitFor(() =>
      expect(screen.getByText("Enable routing")).toBeEnabled(),
    );
  });

  it("does not pass the removed main-page toggle setting to the proxy panel", () => {
    const settings = {
      showInTray: true,
      minimizeToTrayOnClose: true,
      language: "zh",
    } as SettingsFormState;

    render(<LocalProxySettings settings={settings} onAutoSave={vi.fn()} />);

    expect(screen.getByTestId("proxy-panel")).toHaveAttribute(
      "data-props",
      "isProxyPending,onToggleProxy",
    );
  });
});
