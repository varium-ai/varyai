import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingExecution } from "@/components/routing/RoutingExecution";
import { RoutingGroupSlot } from "@/components/routing/RoutingGroupSlot";
import {
  routingExecutionApi,
  type RoutingGroup,
} from "@/lib/api/intentRouting";
vi.mock("@/lib/api/intentRouting", () => ({
  routingExecutionApi: { start: vi.fn(), stop: vi.fn() },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
const group = { id: "fixture" } as RoutingGroup;
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(routingExecutionApi.start).mockResolvedValue({
    token: "synthetic-local-token",
    model: "vary-auto/fixture",
    baseUrl: "http://127.0.0.1:1234/intent/v1",
    modelCatalogPath: "/synthetic/session models/catalog's.json",
    expiresInSeconds: 3600,
  });
  vi.mocked(routingExecutionApi.stop).mockResolvedValue();
});
it("requires consent and offers an isolated CLI command, revoking on close", async () => {
  const user = userEvent.setup();
  const { unmount } = render(<RoutingExecution group={group} />);
  await user.click(screen.getByText("intentRouting.executionTitle"));
  expect(screen.getByRole("button")).toBeDisabled();
  await user.click(screen.getByRole("checkbox"));
  await user.click(screen.getByRole("button"));
  expect(routingExecutionApi.start).toHaveBeenCalledWith(group, true);
  await waitFor(() =>
    expect(screen.getByText(/VARYAI_ROUTING_TOKEN=/)).toHaveTextContent(
      'wire_api="responses"',
    ),
  );
  expect(screen.getByText(/VARYAI_ROUTING_TOKEN=/)).toHaveTextContent(
    'model_catalog_json="/synthetic/session models/catalog',
  );
  expect(screen.getByText(/VARYAI_ROUTING_TOKEN=/).textContent).toContain(
    "'\\''",
  );
  unmount();
  expect(routingExecutionApi.stop).toHaveBeenCalledWith(
    "synthetic-local-token",
  );
});
it("revokes a late-created session after its editor closes", async () => {
  let resolve!: (
    value: Awaited<ReturnType<typeof routingExecutionApi.start>>,
  ) => void;
  vi.mocked(routingExecutionApi.start).mockReturnValue(
    new Promise((r) => {
      resolve = r;
    }),
  );
  const user = userEvent.setup();
  const { unmount } = render(<RoutingExecution group={group} />);
  await user.click(screen.getByText("intentRouting.executionTitle"));
  await user.click(screen.getByRole("checkbox"));
  await user.click(screen.getByRole("button"));
  unmount();
  resolve({
    token: "late-token",
    model: "vary-auto/fixture",
    baseUrl: "http://127.0.0.1:1234/intent/v1",
    modelCatalogPath: "/synthetic/catalog.json",
    expiresInSeconds: 3600,
  });
  await waitFor(() =>
    expect(routingExecutionApi.stop).toHaveBeenCalledWith("late-token"),
  );
});

it("revokes immediately when the group starts folding, with fresh consent on reopen", async () => {
  const user = userEvent.setup();
  const content = (active: boolean) => (
    <ul>
      <RoutingGroupSlot
        expanded={active}
        editor={active ? <RoutingExecution group={group} /> : null}
      >
        <button>Edit</button>
      </RoutingGroupSlot>
    </ul>
  );
  const panel = render(content(true));
  await user.click(screen.getByText("intentRouting.executionTitle"));
  await user.click(screen.getByRole("checkbox"));
  await user.click(screen.getByRole("button"));
  await screen.findByText(/VARYAI_ROUTING_TOKEN=/);
  panel.rerender(content(false));
  expect(routingExecutionApi.stop).toHaveBeenCalledExactlyOnceWith(
    "synthetic-local-token",
  );
  expect(screen.queryByText(/VARYAI_ROUTING_TOKEN=/)).not.toBeInTheDocument();
  panel.rerender(content(true));
  await user.click(screen.getByText("intentRouting.executionTitle"));
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  expect(screen.getByRole("button")).toBeDisabled();
  panel.unmount();
  expect(routingExecutionApi.stop).toHaveBeenCalledOnce();
});
