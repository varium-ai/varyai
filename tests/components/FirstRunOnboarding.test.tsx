import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { FirstRunNoticeDialog } from "@/components/FirstRunNoticeDialog";
import { settingsApi } from "@/lib/api";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

vi.mock("@/lib/api", () => ({ settingsApi: { get: vi.fn(), save: vi.fn() } }));
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { list: vi.fn() },
}));
vi.mock("@/components/onboarding/OnboardingWizard", () => ({
  OnboardingWizard: ({
    onFinish,
  }: {
    onFinish: (connected: boolean) => Promise<void>;
  }) => (
    <div role="dialog">
      <button onClick={() => void onFinish(false)}>Skip fixture</button>
      <button onClick={() => void onFinish(true)}>Complete fixture</button>
    </div>
  ),
}));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(settingsApi.get).mockResolvedValue({ language: "en" } as never);
  vi.mocked(settingsApi.save).mockResolvedValue(true);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
});
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const onConnected = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <FirstRunNoticeDialog onConnected={onConnected} />
    </QueryClientProvider>,
  );
  return { client, onConnected };
}
it("opens automatically only for an unacknowledged installation without providers", async () => {
  mount();
  expect(await screen.findByRole("dialog")).toBeVisible();
});
it("does not interrupt installations that already have a provider", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    { id: "existing" },
  ] as never);
  const { client } = mount();
  await waitFor(() =>
    expect(client.getQueryState(["routing-upstreams"])?.status).toBe("success"),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("does not reopen after acknowledgement", async () => {
  vi.mocked(settingsApi.get).mockResolvedValue({
    firstRunNoticeConfirmed: true,
  } as never);
  const { client } = mount();
  await waitFor(() =>
    expect(client.getQueryState(["settings"])?.status).toBe("success"),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("persists skipping and dismisses without navigating away", async () => {
  const { onConnected } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "Skip fixture" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(settingsApi.save).toHaveBeenCalledWith({
    language: "en",
    firstRunNoticeConfirmed: true,
  });
  expect(onConnected).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).toBeNull();
});
it("keeps the guide open as providers refresh, then navigates to Tools on completion", async () => {
  const { client, onConnected } = mount();
  await screen.findByRole("dialog");
  client.setQueryData(["routing-upstreams"], [{ id: "newly-saved" }]);
  expect(screen.getByRole("dialog")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Complete fixture" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalled());
});
