import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { ToolRiskPolicyChangesPanel } from "./ToolRiskPolicyChangesPanel";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/lib/api", async () => ({
  toolRiskApi: (await import("@/lib/api/toolRisk")).toolRiskApi,
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { page?: number }) =>
      args?.page ? `${key} ${args.page}` : key,
  }),
}));
const row = {
  id: 1,
  at: 1_700_000_000_000,
  policyVersion: 8,
  actor: "local_ui",
  kind: "rule_update",
  targetId: "builtin.sensitive_read",
  changedFields: ["enabled", "conditions"],
  auditStatus: "recorded",
};
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ToolRiskPolicyChangesPanel />
    </QueryClientProvider>,
  );
}
const button = (name: string) => screen.getByRole("button", { name });
describe("P2-9 policy changes panel with real hooks and API", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset().mockResolvedValue([]);
    vi.mocked(toast.success).mockClear();
    vi.mocked(toast.error).mockClear();
  });
  it.each([false, true])(
    "reports manual refresh failure=%s without exposing errors",
    async (failed) => {
      mount();
      await screen.findByText("toolRisk.policyChanges.empty");
      if (failed)
        vi.mocked(invoke).mockRejectedValueOnce(
          new Error("synthetic-private-detail"),
        );
      fireEvent.click(button("common.refresh"));
      await waitFor(() =>
        expect(failed ? toast.error : toast.success).toHaveBeenCalledWith(
          `toolRisk.policyChanges.${failed ? "refreshFailed" : "refreshed"}`,
        ),
      );
      expect(document.body.textContent).not.toContain(
        "synthetic-private-detail",
      );
    },
  );
  it("shows localized builtin names and the current custom rule name beside stable IDs", async () => {
    const id = "custom-12345678-1234-1234-1234-123456789abc";
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "get_tool_risk_rules"
        ? [{ id, origin: "custom", name: "Synthetic custom rule" }]
        : [row, { ...row, id: 2, targetId: id }],
    );
    mount();
    expect(
      await screen.findByText("toolRisk.builtinRule.sensitive_read.name"),
    ).toBeVisible();
    expect(await screen.findByText("Synthetic custom rule")).toBeVisible();
    expect(screen.getByText(id)).toBeVisible();
    expect(screen.getByText("builtin.sensitive_read")).toBeVisible();
    expect(
      screen.getByText("toolRisk.policyChanges.currentNamesHint"),
    ).toBeVisible();
  });
  it.each([false, true])(
    "refreshes current rule names and waits for their result when failed=%s",
    async (failed) => {
      const id = "custom-12345678-1234-1234-1234-123456789abc";
      vi.mocked(invoke).mockImplementation(async (command) =>
        command === "get_tool_risk_rules"
          ? [{ id, origin: "custom", name: "Synthetic original name" }]
          : [{ ...row, targetId: id }],
      );
      mount();
      await screen.findByText("Synthetic original name");
      let finish!: () => void;
      vi.mocked(invoke).mockImplementation((command) =>
        command === "get_tool_risk_rules"
          ? new Promise((resolve, reject) => {
              finish = () =>
                failed
                  ? reject(new Error("synthetic-private-name-error"))
                  : resolve([
                      { id, origin: "custom", name: "Synthetic updated name" },
                    ]);
            })
          : Promise.resolve([{ ...row, targetId: id }]),
      );
      fireEvent.click(button("common.refresh"));
      await waitFor(() => expect(finish).toBeTypeOf("function"));
      expect(button("common.refresh")).toBeDisabled();
      expect(toast.success).not.toHaveBeenCalled();
      await act(async () => finish());
      await waitFor(() =>
        expect(failed ? toast.error : toast.success).toHaveBeenCalledWith(
          `toolRisk.policyChanges.${failed ? "refreshFailed" : "refreshed"}`,
        ),
      );
      if (!failed)
        expect(await screen.findByText("Synthetic updated name")).toBeVisible();
      expect(document.body.textContent).not.toContain(
        "synthetic-private-name-error",
      );
    },
  );
  it("starts at 50 records, displays fixed metadata and the honest local-history boundary", async () => {
    vi.mocked(invoke).mockResolvedValue([row]);
    mount();
    expect(await screen.findByText("builtin.sensitive_read")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("get_tool_risk_policy_changes", {
      limit: 50,
      offset: 0,
    });
    for (const key of ["description", "metadataOnly", "actorHint"])
      expect(screen.getByText(`toolRisk.policyChanges.${key}`)).toBeVisible();
    for (const key of [
      "actor.local_ui",
      "kind.rule_update",
      "field.enabled",
      "field.conditions",
      "audit.recorded",
    ])
      expect(screen.getByText(`toolRisk.policyChanges.${key}`)).toBeVisible();
    expect(button("security.prevPage")).toBeDisabled();
    expect(button("security.nextPage")).toBeDisabled();
  });
  it("pages by 50, returns to the previous page, and refreshes the active page", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) =>
      command === "get_tool_risk_rules"
        ? []
        : (args as { offset: number }).offset === 0
          ? Array.from({ length: 50 }, (_, i) => ({ ...row, id: i + 1 }))
          : [{ ...row, id: 51 }],
    );
    mount();
    await waitFor(() => expect(button("security.nextPage")).toBeEnabled());
    fireEvent.click(button("security.nextPage"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_tool_risk_policy_changes", {
        limit: 50,
        offset: 50,
      }),
    );
    expect(screen.getByText("toolRisk.policyChanges.page 2")).toBeVisible();
    await waitFor(() => expect(button("common.refresh")).toBeEnabled());
    const count = vi.mocked(invoke).mock.calls.length;
    fireEvent.click(button("common.refresh"));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(count + 2));
    expect(invoke).toHaveBeenCalledWith("get_tool_risk_policy_changes", {
      limit: 50,
      offset: 50,
    });
    fireEvent.click(button("security.prevPage"));
    await waitFor(() =>
      expect(screen.getByText("toolRisk.policyChanges.page 1")).toBeVisible(),
    );
  });
  it("requires confirmation, supports cancel, clears with no args and resets paging", async () => {
    let cleared = false;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "clear_tool_risk_policy_changes") {
        cleared = true;
        return 51;
      }
      return cleared
        ? []
        : Array.from({ length: 50 }, (_, i) => ({ ...row, id: i + 1 }));
    });
    mount();
    await waitFor(() => expect(button("security.nextPage")).toBeEnabled());
    fireEvent.click(button("security.nextPage"));
    await waitFor(() =>
      expect(button("toolRisk.policyChanges.clear")).toBeEnabled(),
    );
    fireEvent.click(button("toolRisk.policyChanges.clear"));
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(
      screen.getByText("toolRisk.policyChanges.clearConfirmDescription"),
    ).toBeVisible();
    expect(invoke).not.toHaveBeenCalledWith("clear_tool_risk_policy_changes");
    fireEvent.click(button("common.cancel"));
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(button("toolRisk.policyChanges.clear"));
    fireEvent.click(button("toolRisk.policyChanges.confirmClear"));
    expect(
      await screen.findByText("toolRisk.policyChanges.empty"),
    ).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("clear_tool_risk_policy_changes");
    expect(screen.getByText("toolRisk.policyChanges.page 1")).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  it("shows loading, disables duplicate actions, then shows empty", async () => {
    let resolve!: (value: unknown[]) => void;
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_tool_risk_rules"
        ? Promise.resolve([])
        : new Promise((done) => {
            resolve = done;
          }),
    );
    mount();
    expect(screen.getByText("common.loading")).toBeVisible();
    expect(button("common.refresh")).toBeDisabled();
    expect(button("toolRisk.policyChanges.clear")).toBeDisabled();
    await act(async () => resolve([]));
    expect(
      await screen.findByText("toolRisk.policyChanges.empty"),
    ).toBeVisible();
  });
  it("shows a fixed read error and can retry without echoing rejected values", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("secret database path"));
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "toolRisk.policyChanges.loadFailed",
    );
    expect(document.body.textContent).not.toContain("secret");
    expect(screen.queryByText("toolRisk.policyChanges.empty")).toBeNull();
    fireEvent.click(button("common.refresh"));
    expect(
      await screen.findByText("toolRisk.policyChanges.empty"),
    ).toBeVisible();
  });
  it("keeps clear confirmation open on failure, suppresses secrets and allows retry", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "clear_tool_risk_policy_changes")
        throw new Error("secret clear");
      return [row];
    });
    mount();
    await screen.findByText(row.targetId);
    fireEvent.click(button("toolRisk.policyChanges.clear"));
    fireEvent.click(button("toolRisk.policyChanges.confirmClear"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "toolRisk.policyChanges.clearFailed",
    );
    expect(screen.getByRole("dialog")).toBeVisible();
    expect(document.body.textContent).not.toContain("secret");
  });
  it("never renders arbitrary enum, target, field or before/after content", async () => {
    vi.mocked(invoke).mockResolvedValue([
      {
        ...row,
        actor: "secret actor",
        kind: "secret kind",
        targetId: "custom-secret",
        changedFields: ["enabled", "secret field"],
        auditStatus: "secret audit",
        before: "secret before",
        after: "secret after",
      },
    ]);
    mount();
    expect(
      await screen.findByText("toolRisk.policyChanges.targetUnknown"),
    ).toBeVisible();
    for (const key of ["actor.unknown", "kind.unknown", "audit.unknown"])
      expect(screen.getByText(`toolRisk.policyChanges.${key}`)).toBeVisible();
    expect(document.body.textContent).not.toContain("secret");
  });
});
