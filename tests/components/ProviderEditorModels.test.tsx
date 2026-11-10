import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ProviderEditorDialog } from "@/components/providers/ProviderEditorDialog";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";
import { SubscriptionProviderFields } from "@/components/providers/SubscriptionProviderFields";
import { globalProviderPresets } from "@/config/globalProviderPresets";

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  getModelContextWindows: vi.fn(async () => ({
    "known-relay": 1_000_000,
    "limited-relay": 1_000_000,
  })),
}));
vi.mock("@/components/providers/NativeSubscriptionAccounts", () => ({
  NativeSubscriptionAccounts: () => null,
}));
vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: () => ({ accounts: [], cancelAuth: vi.fn() }),
}));
vi.mock("@/components/providers/ProviderKeysField", () => ({
  ProviderKeysField: () => null,
}));
vi.mock("@/components/providers/ApiProviderQuota", () => ({
  ApiProviderQuota: () => <div>subscriptionProvider.allowance</div>,
}));

const catalog = [
  { model: "chat-fixture", inputModalities: ["text", "image"] },
  { model: "gpt-image-2" },
  { model: "vendor-drawing", kind: "image" },
  ...Array.from({ length: 25 }, (_, i) => ({ model: `chat-${i}` })),
];

function Editor({
  revision = 1,
  onProbe = vi.fn(),
  discoveryOnly = false,
  presetId = "",
  vendorMode = false,
  initialModels = "[]",
  modelCatalog: suppliedCatalog = catalog,
}) {
  const [models, setModels] = useState(initialModels);
  const [draft, setDraft] = useState<RoutingUpstream>({
    schemaVersion: 1,
    id: "fixture",
    revision,
    name: "Fixture",
    hasApiKey: true,
    ...(presetId ? { presetId } : {}),
    source: {
      kind: "api",
      baseUrl: "https://fixture.example/v1",
      apiFormat: "openai_chat",
      mode: vendorMode ? "vendor" : "picked",
      models: [],
      catalog: discoveryOnly ? [] : suppliedCatalog,
      groupsOnly: false,
    },
  });
  return (
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProviderEditorDialog
        draft={draft}
        models={models}
        discovered={suppliedCatalog.map((row) => ({
          ...row,
          id: row.model,
          ownedBy: null,
        }))}
        apiKey=""
        busy={false}
        error={null}
        region="global"
        regions={[]}
        onDraftChange={setDraft}
        onEnabledChange={vi.fn()}
        onModelsChange={setModels}
        onKeyChange={vi.fn()}
        onProtocolChange={vi.fn()}
        onRegionChange={vi.fn()}
        onDiscover={vi.fn()}
        onFetchProbeModels={vi.fn()}
        onProbe={onProbe}
        onSave={vi.fn()}
        onClose={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onReturnFocus={vi.fn()}
        onError={vi.fn()}
        nestedOpen={false}
        onProbingChange={vi.fn()}
      />
    </QueryClientProvider>
  );
}

it("omits the extra allowance field from the provider upper form", () => {
  render(<Editor />);
  expect(screen.queryByText("subscriptionProvider.allowance")).toBeNull();
});

it.each([0, 1])(
  "supplies Magpie placeholders for custom provider inputs at revision %s",
  (revision) => {
    render(<Editor revision={revision} />);
    expect(screen.getByLabelText("modelProviders.name")).toHaveAttribute(
      "placeholder",
      "providerEditor.customNamePlaceholder",
    );
    expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveAttribute(
      "placeholder",
      "https://…/v1",
    );
    if (revision)
      expect(screen.getByLabelText("ID")).toHaveAttribute(
        "placeholder",
        "fixture",
      );
    expect(
      screen.getByRole("textbox", { name: "providerEditor.addModelId" }),
    ).toHaveAttribute(
      "placeholder",
      revision ? "providerEditor.addModelId" : "providerParity.optionalModels",
    );
  },
);

it.each([0, 1])(
  "separates image generation from chat models at revision %s",
  (revision) => {
    render(<Editor revision={revision} />);
    const chat = screen.getByRole("group", { name: "modelPicker.models" });
    const images = screen.getByRole("group", {
      name: "providerEditor.imageModels",
    });
    expect(
      within(chat).getByRole("button", { name: "chat-fixture" }),
    ).toBeInTheDocument();
    expect(
      within(chat).queryByRole("button", { name: "gpt-image-2" }),
    ).not.toBeInTheDocument();
    expect(
      within(images).getByRole("button", { name: "gpt-image-2" }),
    ).toBeInTheDocument();
    expect(
      within(images).getByRole("button", { name: "vendor-drawing" }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectAll" }),
    );
    expect(
      within(chat).getByRole("button", { name: "chat-fixture" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      within(images).getByRole("button", { name: "gpt-image-2" }),
    ).toHaveAttribute("aria-pressed", "false");
  },
);

it("keeps the filter above the list and manual entry below it, without a custom counter", () => {
  render(<Editor />);
  const filter = screen.getByRole("searchbox");
  const list = screen.getByRole("group", { name: "modelPicker.models" });
  const manual = screen.getByLabelText("providerEditor.addModelId");
  expect(
    filter.compareDocumentPosition(list) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    list.compareDocumentPosition(manual) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    screen.queryByText("providerParity.pickedCount"),
  ).not.toBeInTheDocument();
});

it("shows the supplier catalog without model-view modes", () => {
  render(<Editor />);
  expect(
    screen.queryByRole("radiogroup", { name: "providerEditor.modelView" }),
  ).not.toBeInTheDocument();
  const list = screen.getByRole("group", { name: "modelPicker.models" });
  expect(within(list).getByRole("button", { name: "chat-24" })).toBeVisible();
  fireEvent.click(within(list).getByRole("button", { name: "chat-fixture" }));
  expect(within(list).getByRole("button", { name: "chat-24" })).toBeVisible();
});

it("does not probe selected image models as chat models", () => {
  const onProbe = vi.fn().mockResolvedValue([]);
  render(<Editor onProbe={onProbe} />);
  fireEvent.click(screen.getByRole("button", { name: "gpt-image-2" }));
  expect(
    screen.getByRole("button", { name: "codexProvider.testModels" }),
  ).toBeDisabled();
});

it.each([
  "claude",
  "chatgpt_api",
  "github_copilot",
  "xai_oauth",
  "gemini_cli",
  "antigravity",
] as const)("uses the same model sections for %s subscriptions", (provider) => {
  const draft: RoutingUpstream = {
    schemaVersion: 1,
    id: "fixture",
    revision: 1,
    name: "Fixture",
    hasApiKey: false,
    source: {
      kind: "subscription",
      provider,
      accountId: "",
      models: [],
      catalog,
      mode: "picked",
      groupsOnly: false,
    },
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SubscriptionProviderFields
        draft={draft}
        disabled={false}
        onChange={vi.fn()}
        onError={vi.fn()}
        onBusyChange={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const chat = screen.getByRole("group", { name: "modelPicker.models" });
  expect(
    within(chat).getByRole("button", { name: "chat-fixture" }),
  ).toBeInTheDocument();
  expect(
    within(chat).queryByRole("button", { name: "gpt-image-2" }),
  ).not.toBeInTheDocument();
  expect(
    within(
      screen.getByRole("group", { name: "providerEditor.imageModels" }),
    ).getByRole("button", { name: "gpt-image-2" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("searchbox")).toBeInTheDocument();
  expect(
    screen.queryByRole("radiogroup", { name: "providerEditor.modelView" }),
  ).not.toBeInTheDocument();
});

it("omits advanced controls from the upper provider form", () => {
  const { container } = render(<Editor />);
  expect(container.querySelector(".pv-advanced")).not.toBeInTheDocument();
  expect(screen.queryByText("codexProvider.endpoint")).not.toBeInTheDocument();
});

it("uses vendor image-generation metadata immediately after discovery", () => {
  render(<Editor discoveryOnly />);
  expect(
    within(
      screen.getByRole("group", { name: "providerEditor.imageModels" }),
    ).getByRole("button", { name: "vendor-drawing" }),
  ).toBeInTheDocument();
});

it("filters model names and IDs without treating the provider name as a model match", () => {
  render(<Editor />);
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "Fixture" },
  });
  expect(
    within(
      screen.getByRole("group", { name: "modelPicker.models" }),
    ).getAllByRole("button"),
  ).toHaveLength(1);
});

it("can select none from a vendor-served list even when there are no explicit picks", () => {
  render(<Editor vendorMode />);
  const none = screen.getByRole("button", {
    name: "providerEditor.selectNone",
  });
  expect(none).toBeEnabled();
  fireEvent.click(none);
  expect(screen.getByRole("button", { name: "chat-fixture" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(none).toBeDisabled();
});

it("keeps Magpie's new-provider ID input before discovery controls and its saved-provider entry below chips", () => {
  const { unmount } = render(<Editor revision={0} />);
  const manual = screen.getByLabelText("providerEditor.addModelId");
  expect(
    manual.compareDocumentPosition(screen.getByRole("searchbox")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(screen.getByText("providerParity.pickedCount")).toBeInTheDocument();
  unmount();
  render(<Editor revision={1} />);
  expect(
    screen
      .getByRole("group", { name: "modelPicker.models" })
      .compareDocumentPosition(
        screen.getByLabelText("providerEditor.addModelId"),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    screen.queryByText("providerParity.pickedCount"),
  ).not.toBeInTheDocument();
});

it.each(globalProviderPresets)(
  "keeps custom-only controls out of the $id preset's upper form",
  ({ id }) => {
    render(<Editor presetId={id} />);
    expect(
      screen.queryByLabelText("modelProviders.baseUrl"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "providerEditor.builtinIcons" }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("ID")).not.toHaveAttribute("readonly");
  },
);

it("shows Magpie 1M badges for sparse selected rows and public ID-only facts", async () => {
  const rows = [
    { model: "catalog-long", contextWindow: 1_048_576 },
    { model: "known-relay" },
    { model: "limited-relay", contextWindow: 128_000 },
    { model: "unknown-relay" },
    { model: "already (1M)", contextWindow: 1_000_000 },
  ];
  render(
    <Editor
      initialModels={JSON.stringify(rows.map(({ model }) => ({ model })))}
      modelCatalog={rows}
    />,
  );
  expect(
    within(screen.getByRole("button", { name: /catalog-long/ })).getByText(
      "1.0M",
    ),
  ).toBeInTheDocument();
  expect(
    await within(
      screen.getByRole("button", { name: "known-relay" }),
    ).findByText("1M"),
  ).toBeInTheDocument();
  for (const model of ["limited-relay", "unknown-relay", "already (1M)"]) {
    expect(
      screen
        .getByRole("button", {
          name: new RegExp(model.replace(/[()]/g, "\\$&")),
        })
        .querySelector("small"),
    ).toBeNull();
  }
});
