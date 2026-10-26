import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { GatewayKeysPanel } from "@/components/proxy/GatewayKeysPanel";
import { gatewayApi } from "@/lib/api/gateway";
vi.mock("@/lib/api/gateway", () => ({
  gatewayApi: {
    list: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    revoke: vi.fn(),
  },
}));
const policy = {
  name: "Synthetic",
  models: ["allowed"],
  tokenLimit: 100,
  period: "daily" as const,
};
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <GatewayKeysPanel port="15721" />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(gatewayApi.list).mockResolvedValue([]);
  vi.mocked(gatewayApi.create).mockResolvedValue({
    key: {
      id: "synthetic",
      policy,
      revoked: false,
      usedTokens: 0,
      resetsAt: null,
    },
    secret: "synthetic-secret",
  });
});
it("creates an exact allowlist and shows the secret only until dismissed", async () => {
  mount();
  fireEvent.change(screen.getByLabelText("gatewayKeys.name"), {
    target: { value: "Project" },
  });
  fireEvent.change(screen.getByLabelText("gatewayKeys.models"), {
    target: { value: " a, b, a " },
  });
  fireEvent.change(screen.getByLabelText("gatewayKeys.limit"), {
    target: { value: "50" },
  });
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.create" }));
  await waitFor(() =>
    expect(gatewayApi.create).toHaveBeenCalledWith({
      name: "Project",
      models: ["a", "b"],
      tokenLimit: 50,
      period: "daily",
    }),
  );
  await screen.findByText("synthetic-secret");
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.dismiss" }));
  expect(screen.queryByText("synthetic-secret")).not.toBeInTheDocument();
});
it("updates budgets without replacing a credential and revokes by ID", async () => {
  vi.mocked(gatewayApi.list).mockResolvedValue([
    { id: "fixture", policy, revoked: false, usedTokens: 42, resetsAt: null },
  ]);
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "gatewayKeys.edit" }),
  );
  fireEvent.change(screen.getByLabelText("gatewayKeys.limit"), {
    target: { value: "10" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(gatewayApi.update).toHaveBeenCalledWith("fixture", {
      ...policy,
      tokenLimit: 10,
    }),
  );
  expect(gatewayApi.create).not.toHaveBeenCalled();
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "gatewayKeys.revoke" }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "gatewayKeys.revoke" }));
  await waitFor(() =>
    expect(gatewayApi.revoke).toHaveBeenCalledWith("fixture"),
  );
});
