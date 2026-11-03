import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { ProviderImportDialog } from "@/components/providers/ProviderImportDialog";
import { providerImportApi } from "@/lib/api/providerImport";

vi.mock("@/lib/api/providerImport", () => ({
  providerImportApi: { preview: vi.fn(), import: vi.fn() },
}));
const sources = ["cc-switch", "claude-code", "codex", "opencode", "magpie"].map(
  (id, at) => ({
    id,
    name: ["CC Switch", "Claude Code", "Codex", "OpenCode", "Provider backup"][
      at
    ],
    path: `~/.fixture/${id}`,
    found: true,
    items: at
      ? []
      : [
          {
            reference: "new",
            fingerprint: "digest-new",
            name: "New relay",
            from: "Codex",
            host: "relay.example",
            modelCount: 2,
            status: "new",
            enabled: true,
          },
          {
            reference: "same",
            fingerprint: "digest-same",
            name: "Existing relay",
            host: "old.example",
            modelCount: 1,
            status: "same",
            existing: "Saved relay",
            enabled: true,
          },
          {
            reference: "key",
            fingerprint: "digest-key",
            name: "Another key",
            host: "relay.example",
            modelCount: 2,
            status: "vendor",
            existing: "Vendor",
            enabled: true,
          },
          {
            reference: "oauth",
            fingerprint: "digest-oauth",
            name: "Official account",
            host: "",
            modelCount: 0,
            status: "new",
            skip: "subscription",
            enabled: true,
          },
        ],
  }),
);
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(providerImportApi.preview).mockResolvedValue(sources as never);
  vi.mocked(providerImportApi.import).mockResolvedValue({
    added: 2,
    skipped: 0,
  });
});
function mount(onImported = vi.fn()) {
  return render(
    <ProviderImportDialog open onClose={vi.fn()} onImported={onImported} />,
  );
}
it("shows five sources, pickable counts and disabled duplicate/subscription rows", async () => {
  mount();
  const tabs = await screen.findByRole("tablist");
  expect(within(tabs).getAllByRole("tab")).toHaveLength(5);
  expect(
    within(tabs).getByRole("tab", { name: /Provider backup/ }),
  ).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "New relay" })).toBeChecked();
  expect(
    screen.getByRole("checkbox", { name: "Existing relay" }),
  ).toBeDisabled();
  expect(
    screen.getByRole("checkbox", { name: "Official account" }),
  ).toBeDisabled();
  expect(screen.getByText("providerImport.readOnly")).toBeInTheDocument();
  fireEvent.click(within(tabs).getByRole("tab", { name: /Provider backup/ }));
  expect(screen.getByText("providerImport.empty")).toBeInTheDocument();
  fireEvent.click(within(tabs).getByRole("tab", { name: /CC Switch/ }));
  expect(screen.getByRole("checkbox", { name: "New relay" })).toBeChecked();
});
it("selects only importable rows and sends references with fresh-source fingerprints, never keys", async () => {
  const done = vi.fn();
  mount(done);
  const all = await screen.findByRole("checkbox", {
    name: "providerEditor.selectAll",
  });
  fireEvent.click(all);
  expect(
    screen.getByRole("button", { name: "providerFlow.import" }),
  ).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox", { name: "Another key" }));
  fireEvent.click(screen.getByRole("radio", { name: "providerImport.asNew" }));
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  await waitFor(() =>
    expect(providerImportApi.import).toHaveBeenCalledExactlyOnceWith([
      {
        source: "cc-switch",
        reference: "key",
        fingerprint: "digest-key",
        mode: "add",
      },
    ]),
  );
  expect(done).toHaveBeenCalled();
});
it("keeps the modal open and reports only a safe message on source changes", async () => {
  vi.mocked(providerImportApi.import).mockRejectedValue(
    "providerImport.conflict synthetic-private",
  );
  const done = vi.fn();
  mount(done);
  await screen.findByRole("tablist");
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "providerImport.failed",
  );
  expect(screen.queryByText(/synthetic-private/)).not.toBeInTheDocument();
  expect(done).not.toHaveBeenCalled();
});

it("offers a saved imported Codex subscription in the same picker and submits only its reference", async () => {
  const next = structuredClone(sources);
  next[4].items = [
    {
      reference: "login/0/saved",
      fingerprint: "opaque-account",
      name: "Codex saved account",
      from: "Codex · ChatGPT Plus",
      host: "",
      modelCount: 8,
      status: "new",
      enabled: true,
      subscription: true,
    },
  ] as never;
  vi.mocked(providerImportApi.preview).mockResolvedValue(next as never);
  mount();
  const tabs = await screen.findByRole("tablist");
  fireEvent.click(within(tabs).getByRole("tab", { name: /Provider backup/ }));
  expect(
    screen.getByRole("checkbox", { name: "Codex saved account" }),
  ).toBeChecked();
  expect(screen.getByText("providerImport.subscriptionWarning")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  await waitFor(() =>
    expect(providerImportApi.import).toHaveBeenCalledWith(
      expect.arrayContaining([
        {
          source: "magpie",
          reference: "login/0/saved",
          fingerprint: "opaque-account",
          mode: "add",
        },
      ]),
    ),
  );
});
