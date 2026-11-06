import { act, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { CodexProxyWarning } from "@/components/agents/CodexProxyWarning";
import { agentConnectionsApi } from "@/lib/api/agentConnections";

vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: { diagnoseCodexProxy: vi.fn() },
}));

const issue = {
  target: "http://127.0.0.1:15722",
  proxy: "http://127.0.0.1:7890",
  source: "system" as const,
};
const keys = {
  title: "agentConnections.codexProxy.title",
  action: "agentConnections.codexProxy.howToFix",
  acknowledge: "agentConnections.codexProxy.acknowledge",
  check: "agentConnections.codexProxy.checkAgain",
  disconnect: "agentConnections.disconnectRestore",
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function mount(
  props: Partial<React.ComponentProps<typeof CodexProxyWarning>> = {},
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onDisconnect = vi.fn();
  const onIssueChange = vi.fn();
  let current = {
    enabled: true,
    busy: false,
    onDisconnect,
    onIssueChange,
    ...props,
  };
  const view = () => (
    <QueryClientProvider client={client}>
      <CodexProxyWarning {...current} />
    </QueryClientProvider>
  );
  const rendered = render(view());
  return {
    ...rendered,
    client,
    onDisconnect,
    onIssueChange,
    update(next: Partial<typeof current>) {
      current = { ...current, ...next };
      rendered.rerender(view());
    },
    async refetch() {
      await act(async () => {
        await client.refetchQueries({ queryKey: ["codex-proxy-diagnostic"] });
      });
    },
  };
}

beforeEach(() => {
  vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockReset();
  vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValue(issue);
});

describe("Codex proxy warning", () => {
  it("shows a persistent action without an automatic dialog, and can acknowledge and reopen", async () => {
    const view = mount();
    const action = await screen.findByRole("button", { name: keys.action });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(view.onIssueChange).toHaveBeenLastCalledWith(issue);
    expect(view.onDisconnect).not.toHaveBeenCalled();
    await userEvent.click(action);
    expect(
      screen.getByRole("dialog", { name: keys.title }),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: keys.acknowledge }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await view.refetch();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await userEvent.click(action);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(view.onDisconnect).not.toHaveBeenCalled();
    const query = view.client
      .getQueryCache()
      .find({ queryKey: ["codex-proxy-diagnostic"] });
    expect(query?.options).toMatchObject({
      retry: false,
      refetchInterval: 30000,
    });
  });

  it("only requests the parent's disconnect confirmation after the explicit click", async () => {
    const view = mount();
    await userEvent.click(
      await screen.findByRole("button", { name: keys.action }),
    );
    expect(view.onDisconnect).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: keys.disconnect }),
    );
    expect(view.onDisconnect).toHaveBeenCalledExactlyOnceWith();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await view.refetch();
    expect(view.onDisconnect).toHaveBeenCalledTimes(1);
  });

  it("checks again and closes the dialog and row action when healthy", async () => {
    const view = mount();
    await userEvent.click(
      await screen.findByRole("button", { name: keys.action }),
    );
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValueOnce(
      null,
    );
    await userEvent.click(screen.getByRole("button", { name: keys.check }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenLastCalledWith(
      true,
    );
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    expect(view.onIssueChange).toHaveBeenLastCalledWith(null);
    expect(view.onDisconnect).not.toHaveBeenCalled();
    await view.refetch();
    expect(
      await screen.findByRole("button", { name: keys.action }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does not diagnose while disabled and hides stale results immediately when eligibility changes", async () => {
    const view = mount({ enabled: false });
    expect(agentConnectionsApi.diagnoseCodexProxy).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    view.update({ enabled: true });
    await userEvent.click(
      await screen.findByRole("button", { name: keys.action }),
    );
    view.update({ enabled: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    expect(view.onIssueChange).toHaveBeenLastCalledWith(null);
    const pending = deferred<typeof issue | null>();
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockReturnValueOnce(
      pending.promise,
    );
    view.update({ enabled: true });
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    await act(async () => {
      pending.resolve(null);
    });
    await waitFor(() =>
      expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenCalledTimes(2),
    );
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
  });

  it("ignores an in-flight issue that finishes after disconnect and reconnect", async () => {
    const old = deferred<typeof issue | null>();
    const fresh = deferred<typeof issue | null>();
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy)
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(fresh.promise);
    const view = mount();
    await waitFor(() =>
      expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenCalledTimes(1),
    );
    view.update({ enabled: false });
    view.update({ enabled: true });
    await waitFor(() =>
      expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      old.resolve(issue);
    });
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    await act(async () => {
      fresh.resolve(null);
    });
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    expect(view.onIssueChange).toHaveBeenLastCalledWith(null);
  });

  it("waits for a fresh diagnostic instead of showing an issue cached by a previous mount", async () => {
    const first = mount();
    await screen.findByRole("button", { name: keys.action });
    first.unmount();
    const pending = deferred<typeof issue | null>();
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockReturnValueOnce(
      pending.promise,
    );
    render(
      <QueryClientProvider client={first.client}>
        <CodexProxyWarning enabled busy={false} onDisconnect={vi.fn()} />
      </QueryClientProvider>,
    );
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    await act(async () => {
      pending.resolve(null);
    });
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
  });

  it("does not open over other dialogs and disables actions during an operation", async () => {
    const view = mount({ suspended: true });
    const action = await screen.findByRole("button", { name: keys.action });
    expect(action).toBeDisabled();
    await userEvent.click(action);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    view.update({ suspended: false });
    await userEvent.click(action);
    view.update({ busy: true });
    expect(
      screen.getByRole("button", { name: keys.disconnect }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: keys.check })).toBeDisabled();
    view.update({ busy: false, suspended: true });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    view.update({ suspended: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("uses only the sanitized result fields and reports unavailable checks without raw errors", async () => {
    const view = mount();
    await userEvent.click(
      await screen.findByRole("button", { name: keys.action }),
    );
    expect(screen.getByText(issue.target)).toBeInTheDocument();
    expect(screen.getByText(issue.proxy)).toBeInTheDocument();
    expect(
      screen.getByText("agentConnections.codexProxy.sourceSystem"),
    ).toBeInTheDocument();
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockRejectedValueOnce(
      new Error("synthetic-secret:user@host/path?token=private"),
    );
    await userEvent.click(screen.getByRole("button", { name: keys.check }));
    expect(
      await screen.findByText("agentConnections.codexProxy.unavailable"),
    ).toBeInTheDocument();
    expect(screen.queryByText(issue.proxy)).not.toBeInTheDocument();
    expect(
      screen.queryByText(/synthetic-secret|token=private/),
    ).not.toBeInTheDocument();
    expect(view.onIssueChange).toHaveBeenLastCalledWith(null);
    expect(view.onDisconnect).not.toHaveBeenCalled();
  });

  it("does not treat an unavailable command or a healthy result as a confirmed issue", async () => {
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockRejectedValueOnce(
      new Error("unavailable"),
    );
    const view = mount();
    await waitFor(() =>
      expect(agentConnectionsApi.diagnoseCodexProxy).toHaveBeenCalledTimes(1),
    );
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(view.onIssueChange).not.toHaveBeenCalledWith(issue);
    vi.mocked(agentConnectionsApi.diagnoseCodexProxy).mockResolvedValueOnce(
      null,
    );
    await view.refetch();
    expect(
      screen.queryByRole("button", { name: keys.action }),
    ).not.toBeInTheDocument();
  });
});
