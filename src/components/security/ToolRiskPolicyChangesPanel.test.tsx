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
  });
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
    vi.mocked(invoke).mockImplementation(async (_command, args) =>
      (args as { offset: number }).offset === 0
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
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(count + 1));
    expect(invoke).toHaveBeenLastCalledWith("get_tool_risk_policy_changes", {
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
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((done) => {
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
