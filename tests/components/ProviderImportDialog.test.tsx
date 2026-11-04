import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { ProviderImportDialog } from "@/components/providers/ProviderImportDialog";
import { providerImportApi } from "@/lib/api/providerImport";

vi.mock("@/lib/api/providerImport", () => ({
  providerImportApi: { preview: vi.fn(), import: vi.fn() },
}));
const sources = ["cc-switch", "claude-code", "codex", "opencode", "magpie"].map(
  (id, at) => ({
    id,
    name: ["CC Switch", "Claude Code", "Codex", "OpenCode", "Magpie"][at],
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
  i18n.addResource(
    "zh",
    "translation",
    "providerImport.selected",
    "Selected {{count}}",
  );
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
it("places Magpie first and opens it even when another source has selected providers", async () => {
  mount();
  const tablist = await screen.findByRole("tablist");
  const tabs = within(tablist).getAllByRole("tab");
  expect(tabs.map((tab) => tab.id)).toEqual([
    "import-tab-magpie",
    "import-tab-cc-switch",
    "import-tab-claude-code",
    "import-tab-codex",
    "import-tab-opencode",
  ]);
  expect(tabs[0]).toHaveAccessibleName(/Magpie/);
  expect(tabs[0]).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("tabpanel")).toHaveAttribute(
    "aria-labelledby",
    "import-tab-magpie",
  );
  expect(screen.getByText("providerImport.empty")).toBeInTheDocument();
  expect(screen.getByText("Selected 0")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "providerFlow.import" }),
  ).toBeDisabled();
});
it("moves between source tabs in their displayed order with the keyboard", async () => {
  mount();
  const tablist = await screen.findByRole("tablist");
  const magpie = within(tablist).getByRole("tab", { name: /Magpie/ });
  const ccSwitch = within(tablist).getByRole("tab", { name: /CC Switch/ });
  const openCode = within(tablist).getByRole("tab", { name: /OpenCode/ });
  fireEvent.keyDown(magpie, { key: "ArrowRight" });
  expect(ccSwitch).toHaveAttribute("aria-selected", "true");
  expect(ccSwitch).toHaveFocus();
  expect(screen.getByText("Selected 2")).toBeInTheDocument();
  fireEvent.keyDown(ccSwitch, { key: "Home" });
  expect(magpie).toHaveFocus();
  expect(screen.getByText("Selected 0")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "providerFlow.import" }),
  ).toBeDisabled();
  fireEvent.keyDown(magpie, { key: "ArrowLeft" });
  expect(openCode).toHaveAttribute("aria-selected", "true");
  expect(openCode).toHaveFocus();
});
it("shows five sources, pickable counts and disabled duplicate/subscription rows", async () => {
  mount();
  const tabs = await screen.findByRole("tablist");
  expect(within(tabs).getAllByRole("tab")).toHaveLength(5);
  expect(within(tabs).getByRole("tab", { name: /Magpie/ })).toBeInTheDocument();
  fireEvent.click(within(tabs).getByRole("tab", { name: /CC Switch/ }));
  expect(screen.getByRole("checkbox", { name: "New relay" })).toBeChecked();
  expect(
    screen.getByRole("checkbox", { name: "Existing relay" }),
  ).toBeDisabled();
  expect(
    screen.getByRole("checkbox", { name: "Official account" }),
  ).toBeDisabled();
  expect(screen.getByText("providerImport.readOnly")).toBeInTheDocument();
  fireEvent.click(within(tabs).getByRole("tab", { name: /Magpie/ }));
  expect(screen.getByText("providerImport.empty")).toBeInTheDocument();
  fireEvent.click(within(tabs).getByRole("tab", { name: /CC Switch/ }));
  expect(screen.getByRole("checkbox", { name: "New relay" })).toBeChecked();
});
it("selects only importable rows and sends references with fresh-source fingerprints, never keys", async () => {
  const done = vi.fn();
  mount(done);
  const tabs = await screen.findByRole("tablist");
  fireEvent.click(within(tabs).getByRole("tab", { name: /CC Switch/ }));
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
  const tabs = await screen.findByRole("tablist");
  fireEvent.click(within(tabs).getByRole("tab", { name: /CC Switch/ }));
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
  fireEvent.click(within(tabs).getByRole("tab", { name: /Magpie/ }));
  expect(
    screen.getByRole("checkbox", { name: "Codex saved account" }),
  ).toBeChecked();
  expect(screen.getByText("providerImport.subscriptionWarning")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  await waitFor(() =>
    expect(providerImportApi.import).toHaveBeenCalledExactlyOnceWith([
      {
        source: "magpie",
        reference: "login/0/saved",
        fingerprint: "opaque-account",
        mode: "add",
      },
    ]),
  );
});

it("counts and imports only the active source while preserving choices across tabs", async () => {
  const next = structuredClone(sources);
  next[2].items = [
    {
      ...next[0].items[0],
      name: "Codex relay",
      fingerprint: "digest-codex",
    },
  ];
  vi.mocked(providerImportApi.preview).mockResolvedValue(next as never);
  mount();
  const tabs = await screen.findByRole("tablist");
  const ccSwitch = within(tabs).getByRole("tab", { name: /CC Switch/ });
  const codex = within(tabs).getByRole("tab", { name: /Codex/ });
  const submit = screen.getByRole("button", { name: "providerFlow.import" });

  fireEvent.click(ccSwitch);
  expect(screen.getByText("Selected 2")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "New relay" }));
  fireEvent.click(screen.getByRole("radio", { name: "providerImport.asNew" }));
  expect(screen.getByText("Selected 1")).toBeInTheDocument();

  fireEvent.click(codex);
  expect(screen.getByText("Selected 1")).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("checkbox", { name: "providerEditor.selectAll" }),
  );
  expect(screen.getByText("Selected 0")).toBeInTheDocument();
  expect(submit).toBeDisabled();
  fireEvent.click(submit);
  expect(providerImportApi.import).not.toHaveBeenCalled();

  fireEvent.click(ccSwitch);
  expect(screen.getByRole("checkbox", { name: "New relay" })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Another key" })).toBeChecked();
  expect(
    screen.getByRole("radio", { name: "providerImport.asNew" }),
  ).toHaveAttribute("aria-checked", "true");
  expect(screen.getByText("Selected 1")).toBeInTheDocument();

  fireEvent.click(codex);
  expect(
    screen.getByRole("checkbox", { name: "Codex relay" }),
  ).not.toBeChecked();
  fireEvent.click(screen.getByRole("checkbox", { name: "Codex relay" }));
  fireEvent.click(submit);
  await waitFor(() =>
    expect(providerImportApi.import).toHaveBeenCalledExactlyOnceWith([
      {
        source: "codex",
        reference: "new",
        fingerprint: "digest-codex",
        mode: "add",
      },
    ]),
  );
});
