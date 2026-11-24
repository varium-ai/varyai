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
it("opens automatically when there are fewer than 1 providers even after prior acknowledgement", async () => {
  vi.mocked(settingsApi.get).mockResolvedValue({
    firstRunNoticeConfirmed: true,
  } as never);
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
it("opens onboarding when the last provider is removed", async () => {
  vi.mocked(settingsApi.get).mockResolvedValue({
    firstRunNoticeConfirmed: true,
  } as never);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    { id: "existing" },
  ] as never);
  const { client } = mount();
  await waitFor(() =>
    expect(client.getQueryState(["routing-upstreams"])?.status).toBe("success"),
  );
  expect(screen.queryByRole("dialog")).toBeNull();

  client.setQueryData(["routing-upstreams"], []);
  expect(await screen.findByRole("dialog")).toBeVisible();
});
it("does not allow skipping when there are fewer than 1 providers", async () => {
  const { onConnected } = mount();
  fireEvent.click(await screen.findByRole("button", { name: "Skip fixture" }));
  expect(screen.getByRole("dialog")).toBeVisible();
  expect(settingsApi.save).not.toHaveBeenCalled();
  expect(onConnected).not.toHaveBeenCalled();
});
it("keeps the guide open as providers refresh, then navigates to Tools on completion", async () => {
  const { client, onConnected } = mount();
  await screen.findByRole("dialog");
  client.setQueryData(["routing-upstreams"], [{ id: "newly-saved" }]);
  expect(screen.getByRole("dialog")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Complete fixture" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalled());
});
