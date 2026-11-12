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
function Harness({ initial = record }: { initial?: RoutingUpstream }) {
  const [draft, setDraft] = useState(initial);
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
function mount(initial = record) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <Harness initial={initial} />
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
it("shows Magpie's per-key model and unlimited statuses with row actions", async () => {
  vi.mocked(routingUpstreamsApi.keys).mockResolvedValue([
    { id: "", label: "sk-p…1111", running: 0 },
    { id: "backup", label: "sk-b…2222", running: 0 },
  ]);
  mount({
    ...record,
    transport: {
      ...record.transport!,
      primaryModels: ["chat-a", "chat-b"],
      keys: [
        { id: "backup", name: "Backup", enabled: false, models: ["chat-b"] },
      ],
    },
  });
  const primary = (await screen.findByText("sk-p…1111")).closest("li")!;
  const backup = screen.getByText("sk-b…2222").closest("li")!;
  expect(
    within(primary).getByRole("button", { name: "providerEditor.keyModels" }),
  ).toBeVisible();
  expect(
    within(primary).getByText("providerEditor.keyUnlimitedConcurrency"),
  ).toBeVisible();
  expect(
    within(primary).getByText("providerEditor.keyNoSpendCap"),
  ).toBeVisible();
  expect(within(primary).getByText("providerEditor.inUse")).toBeVisible();
  expect(screen.queryByText("providerEditor.preferredKey")).toBeNull();
  expect(
    within(backup).queryByRole("button", {
      name: "providerEditor.makePreferred",
    }),
  ).toBeNull();
  fireEvent.click(
    within(backup).getByRole("button", { name: "common.enable" }),
  );
  expect(changed.mock.lastCall?.[0].transport.keys[0].enabled).toBe(true);
  fireEvent.click(
    within(backup).getByRole("button", { name: "providerEditor.removeKey" }),
  );
  expect(changed.mock.lastCall?.[0].transport.keys).toHaveLength(0);
});
it("stages independent key model scopes without changing provider-wide picks", async () => {
  mount({
    ...record,
    source: {
      ...record.source,
      kind: "api",
      baseUrl: "https://fixture.example/v1",
      apiFormat: "openai_chat",
      catalog: [],
      mode: "picked",
      groupsOnly: false,
      models: [{ model: "chat-a" }, { model: "chat-b" }],
    },
    transport: { ...record.transport!, primaryModels: ["chat-a"] },
  });
  const primary = (await screen.findByText("sk-p…1111")).closest("li")!;
  fireEvent.click(
    within(primary).getByRole("button", { name: "providerEditor.keyModels" }),
  );
  const picker = within(primary).getByRole("group", {
    name: "providerEditor.keyModelPicker",
  });
  fireEvent.click(within(picker).getByRole("button", { name: "chat-b" }));
  fireEvent.click(within(picker).getByRole("button", { name: "common.save" }));
  expect(changed.mock.lastCall?.[0].transport.primaryModels).toEqual([
    "chat-a",
    "chat-b",
  ]);
  expect(changed.mock.lastCall?.[0].source.models).toEqual([
    { model: "chat-a" },
    { model: "chat-b" },
  ]);
  expect(changed.mock.lastCall?.[0].transport.keys[0].models).toBeUndefined();
});
it("uses Magpie's single API-key input for a new provider", () => {
  mount({ ...record, revision: 0, hasApiKey: false, transport: null });
  const input = screen.getByLabelText("modelProviders.apiKey");
  expect(input).toHaveAttribute("placeholder", "providerEditor.keyPlaceholder");
  expect(input).toHaveAttribute("type", "password");
  expect(screen.queryByText("providerEditor.primaryKey")).toBeNull();
  expect(screen.queryByText("providerEditor.savedKeyHint")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "providerEditor.addKey" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "common.done" })).toBeNull();
  fireEvent.change(input, { target: { value: "synthetic-new-key" } });
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.showKey" }),
  );
  expect(input).toHaveAttribute("type", "text");
  expect(input).toHaveValue("synthetic-new-key");
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.hideKey" }),
  );
  expect(input).toHaveAttribute("type", "password");
  expect(routingUpstreamsApi.keys).not.toHaveBeenCalled();
  expect(routingUpstreamsApi.revealKey).not.toHaveBeenCalled();
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
  expect(
    within(row).queryByRole("button", { name: "providerEditor.makePreferred" }),
  ).toBeNull();
  fireEvent.click(within(row).getByRole("button", { name: "common.enable" }));
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
it("replaces the selected key label with a focused editor in its own row", async () => {
  mount();
  const primary = (await screen.findByText("sk-p…1111")).closest("li")!;
  const backup = screen.getByText("sk-b…2222").closest("li")!;
  fireEvent.click(
    within(backup).getByRole("button", { name: "providerEditor.editKey" }),
  );
  const replacement = within(backup).getByLabelText("modelProviders.apiKey");
  expect(replacement).toHaveFocus();
  expect(replacement).toHaveValue("");
  expect(replacement).toHaveAttribute("type", "password");
  expect(within(backup).queryByText("sk-b…2222")).toBeNull();
  expect(within(primary).getByText("sk-p…1111")).toBeVisible();
  expect(within(primary).queryByLabelText("modelProviders.apiKey")).toBeNull();
  fireEvent.change(replacement, {
    target: { value: "synthetic-inline-key" },
  });
  fireEvent.click(
    within(backup).getByRole("button", { name: "providerEditor.showKey" }),
  );
  expect(replacement).toHaveAttribute("type", "text");
  fireEvent.click(
    within(primary).getByRole("button", { name: "providerEditor.editKey" }),
  );
  const primaryInput = within(primary).getByLabelText("modelProviders.apiKey");
  expect(primaryInput).toHaveFocus();
  expect(primaryInput).toHaveAttribute("type", "password");
  expect(within(primary).queryByText("sk-p…1111")).toBeNull();
  expect(within(backup).queryByLabelText("modelProviders.apiKey")).toBeNull();
  fireEvent.click(
    within(backup).getByRole("button", { name: "providerEditor.editKey" }),
  );
  expect(within(backup).getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-inline-key",
  );
  fireEvent.click(within(backup).getByRole("button", { name: "common.done" }));
  expect(screen.queryByLabelText("modelProviders.apiKey")).toBeNull();
  expect(within(backup).getByText("synt…-key")).toBeVisible();
  expect(routingUpstreamsApi.revealKey).not.toHaveBeenCalled();
});
it("adds, names and removes a key in the new key's row", async () => {
  mount();
  await screen.findByText("sk-p…1111");
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.addKey" }),
  );
  const rows = screen.getAllByRole("listitem");
  expect(rows).toHaveLength(3);
  const added = rows[2];
  expect(within(added).getByLabelText("modelProviders.apiKey")).toHaveFocus();
  fireEvent.change(within(added).getByLabelText("providerEditor.keyName"), {
    target: { value: "Synthetic backup" },
  });
  expect(changed.mock.lastCall?.[0].transport.keys[1].name).toBe(
    "Synthetic backup",
  );
  fireEvent.click(
    within(added).getByRole("button", { name: "providerEditor.removeKey" }),
  );
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.queryByLabelText("modelProviders.apiKey")).toBeNull();
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

it("stops adding keys at the backend limit", async () => {
  mount({
    ...record,
    transport: {
      ...record.transport!,
      keys: Array.from({ length: 64 }, (_, index) => ({
        id: `key-${index}`,
        name: `Key ${index}`,
        enabled: true,
        key: "synthetic-key",
      })),
    },
  });
  expect(
    await screen.findByRole("button", { name: "providerEditor.addKey" }),
  ).toBeDisabled();
});
