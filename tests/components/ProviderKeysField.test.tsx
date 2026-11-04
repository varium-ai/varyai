import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { ProviderKeysField } from "@/components/providers/ProviderKeysField";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";
import { copyText } from "@/lib/clipboard";

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: { keys: vi.fn(), revealKey: vi.fn() },
}));
const record: RoutingUpstream = {
  schemaVersion: 1,
  id: "fixture",
  name: "Fixture",
  revision: 4,
  hasApiKey: true,
  source: {
    kind: "api",
    baseUrl: "https://fixture.example/v1",
    apiFormat: "openai_chat",
    models: [],
    catalog: [],
    mode: "picked",
    groupsOnly: false,
  },
  transport: {
    endpoints: { openai_chat: "https://fixture.example/v1" },
    fullUrl: false,
    keys: [{ id: "backup", name: "Backup", enabled: true }],
  },
};
const changed = vi.fn();
function Harness() {
  const [draft, setDraft] = useState(record);
  const [key, setKey] = useState("");
  return (
    <ProviderKeysField
      draft={draft}
      apiKey={key}
      disabled={false}
      onDraftChange={(next) => {
        changed(next);
        setDraft(next);
      }}
      onKeyChange={setKey}
      onBusyChange={() => {}}
    />
  );
}
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <Harness />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(routingUpstreamsApi.keys).mockResolvedValue([
    { id: "", label: "sk-p…1111", running: 2 },
    { id: "backup", label: "sk-b…2222", running: 3 },
  ]);
});
it("shows masked credentials and live request counts without revealing secrets", async () => {
  mount();
  expect(await screen.findByText("sk-p…1111")).toBeVisible();
  expect(screen.getByText("sk-b…2222")).toBeVisible();
  expect(screen.getAllByText("providerEditor.keyRunning")).toHaveLength(2);
  expect(routingUpstreamsApi.revealKey).not.toHaveBeenCalled();
});
it("stages enabling, preferred-key selection, edits and additions in the provider draft", async () => {
  mount();
  const row = (await screen.findByText("sk-b…2222")).closest("li")!;
  fireEvent.click(within(row).getByRole("checkbox"));
  expect(changed.mock.lastCall?.[0].transport.keys[0].enabled).toBe(false);
  fireEvent.click(
    within(row).getByRole("button", { name: "providerEditor.makePreferred" }),
  );
  expect(changed.mock.lastCall?.[0].transport.preferredKey).toBe("backup");
  expect(changed.mock.lastCall?.[0].transport.keys[0].enabled).toBe(true);
  fireEvent.click(
    within(row).getByRole("button", { name: "providerEditor.editKey" }),
  );
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-edited-key" },
  });
  expect(
    changed.mock.lastCall?.[0].transport.keys[0].key === "synthetic-edited-key",
  ).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "common.done" }));
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.addKey" }),
  );
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-new-key" },
  });
  expect(changed.mock.lastCall?.[0].transport.keys).toHaveLength(2);
  expect(
    changed.mock.lastCall?.[0].transport.keys[1].key === "synthetic-new-key",
  ).toBe(true);
});
it("copies only the explicitly selected stored key and never fills the editor with it", async () => {
  const writeText = vi.mocked(copyText).mockResolvedValue(undefined);
  vi.mocked(routingUpstreamsApi.revealKey).mockResolvedValue(
    "synthetic-copy-key",
  );
  mount();
  const row = (await screen.findByText("sk-b…2222")).closest("li")!;
  fireEvent.click(
    within(row).getByRole("button", { name: "providerEditor.copyKey" }),
  );
  await waitFor(() =>
    expect(routingUpstreamsApi.revealKey).toHaveBeenCalledWith(
      "fixture",
      4,
      "backup",
    ),
  );
  await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
  expect(writeText.mock.calls[0][0] === "synthetic-copy-key").toBe(true);
  expect(
    screen.queryByDisplayValue("synthetic-copy-key"),
  ).not.toBeInTheDocument();
});
