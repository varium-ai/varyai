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
import { ProviderEditorDialog } from "@/components/providers/ProviderEditorDialog";
import {
  globalProviderPresets,
  type GlobalProviderPreset,
} from "@/config/globalProviderPresets";
import { settingsApi } from "@/lib/api/settings";

vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));

const keyPages: Record<string, string> = {
  openai: "https://platform.openai.com/api-keys",
  anthropic: "https://console.anthropic.com/settings/keys",
  google: "https://aistudio.google.com/apikey",
  deepseek: "https://platform.deepseek.com/api_keys",
  moonshot: "https://platform.kimi.ai/console/api-keys",
  zai: "https://z.ai/manage-apikey/apikey-list",
  openrouter: "https://openrouter.ai/keys",
  xai: "https://console.x.ai",
  "kimi-code": "https://www.kimi.ai/code/console",
  zhipu: "https://open.bigmodel.cn/usercenter/proj-mgmt/apikeys",
  minimax:
    "https://platform.minimax.io/user-center/basic-information/interface-key",
  qwen: "https://modelstudio.console.alibabacloud.com/?tab=playground#/api-key",
  mistral: "https://console.mistral.ai/api-keys",
  groq: "https://console.groq.com/keys",
  together: "https://api.together.ai/settings/api-keys",
  fireworks: "https://app.fireworks.ai/settings/users/api-keys",
  siliconflow: "https://cloud.siliconflow.com/account/ak",
  nvidia: "https://build.nvidia.com/settings/api-keys",
  modelscope: "https://modelscope.cn/my/myaccesstoken",
  aihubmix: "https://console.aihubmix.com/token",
  "302ai": "https://302.ai/api-keys/list",
};
const onError = vi.fn();
function Harness({ preset }: { preset?: GlobalProviderPreset }) {
  const [region, setRegion] = useState(preset?.regions?.[0]?.id ?? "global");
  const [apiKey, setApiKey] = useState("");
  const choices =
    preset?.regions ??
    (preset?.china
      ? [
          { id: "global", name: "Global", endpoints: preset.endpoints! },
          { id: "china", name: "China", endpoints: preset.china.endpoints },
        ]
      : []);
  const endpoints =
    choices.find((choice) => choice.id === region)?.endpoints ??
    preset?.endpoints ??
    {};
  const apiFormat =
    preset && endpoints[preset.apiFormat] ? preset.apiFormat : "openai_chat";
  return (
    <ProviderEditorDialog
      draft={{
        schemaVersion: 1,
        id: "",
        name: preset?.name ?? "Custom",
        revision: 0,
        hasApiKey: false,
        ...(preset ? { presetId: preset.id } : {}),
        source: {
          kind: "api",
          baseUrl: endpoints[apiFormat] ?? "https://fixture.example/v1",
          apiFormat,
          models: [],
          catalog: [],
          mode: "vendor",
          groupsOnly: false,
        },
      }}
      models="[]"
      discovered={[]}
      apiKey={apiKey}
      busy={false}
      region={region}
      regions={choices}
      allowedFormats={endpoints}
      onDraftChange={vi.fn()}
      onEnabledChange={vi.fn()}
      onModelsChange={vi.fn()}
      onKeyChange={setApiKey}
      onProtocolChange={vi.fn()}
      onRegionChange={setRegion}
      onDiscover={vi.fn()}
      onFetchProbeModels={vi.fn()}
      onProbe={vi.fn()}
      onSave={vi.fn()}
      onClose={vi.fn()}
      onReturnFocus={vi.fn()}
      onError={onError}
      nestedOpen={false}
      onProbingChange={vi.fn()}
    />
  );
}
function mount(preset?: GlobalProviderPreset) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <Harness preset={preset} />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(settingsApi.openExternal).mockResolvedValue(undefined);
});

it.each(globalProviderPresets)(
  "shows $id key acquisition and every supported protocol endpoint",
  async (preset) => {
    mount(preset);
    const key = screen.getByLabelText("modelProviders.apiKey");
    const getKey = screen.getByRole("button", {
      name: "providerEditor.getApiKey",
    });
    expect(key.closest(".pv-key-entry")).toContainElement(getKey);
    const details = screen.getByRole("group", {
      name: "providerEditor.endpoints",
    });
    const endpoints = preset.regions?.[0]?.endpoints ?? preset.endpoints!;
    expect(details.querySelectorAll("dt")).toHaveLength(
      Object.keys(endpoints).length,
    );
    for (const [format, url] of Object.entries(endpoints)) {
      expect(
        details.querySelector(`[data-protocol="${format}"]`),
      ).toHaveTextContent(url);
    }
    expect(within(details).queryByRole("textbox")).toBeNull();
    expect(screen.queryByLabelText("modelProviders.baseUrl")).toBeNull();
    fireEvent.change(key, { target: { value: "synthetic-private-key" } });
    fireEvent.click(getKey);
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledExactlyOnceWith(
        keyPages[preset.id],
      ),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: new URL(preset.website).host,
      }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenLastCalledWith(preset.website),
    );
  },
);

it.each([
  ["moonshot", "https://platform.kimi.com/console/api-keys"],
  ["kimi-code", "https://www.kimi.com/code/console"],
  [
    "minimax",
    "https://platform.minimaxi.com/user-center/basic-information/interface-key",
  ],
  ["qwen", "https://bailian.console.aliyun.com/?tab=model#/api-key"],
  ["siliconflow", "https://cloud.siliconflow.cn/account/ak"],
])(
  "switches %s key links and endpoint details with the region while keeping the typed key",
  async (id, chinaKeyPage) => {
    const preset = globalProviderPresets.find((preset) => preset.id === id)!;
    mount(preset);
    const key = screen.getByLabelText("modelProviders.apiKey");
    fireEvent.change(key, { target: { value: "synthetic-region-key" } });
    fireEvent.click(screen.getByRole("radio", { name: "China" }));
    const details = screen.getByRole("group", {
      name: "providerEditor.endpoints",
    });
    for (const url of Object.values(preset.china!.endpoints))
      expect(details).toHaveTextContent(url);
    expect(key).toHaveValue("synthetic-region-key");
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.getApiKey" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenLastCalledWith(chinaKeyPage),
    );
    fireEvent.click(screen.getByRole("radio", { name: "Global" }));
    for (const url of Object.values(preset.endpoints!))
      expect(details).toHaveTextContent(url);
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.getApiKey" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenLastCalledWith(keyPages[id]),
    );
  },
);

it.each(["zai", "zhipu"])(
  "updates %s endpoint details for its Coding Plan",
  async (id) => {
    const preset = globalProviderPresets.find((preset) => preset.id === id)!;
    mount(preset);
    const details = screen.getByRole("group", {
      name: "providerEditor.endpoints",
    });
    expect(
      details.querySelector('[data-protocol="openai_responses"]'),
    ).toBeNull();
    fireEvent.click(screen.getByRole("radio", { name: "Coding Plan" }));
    for (const [format, url] of Object.entries(preset.regions![1].endpoints))
      expect(
        details.querySelector(`[data-protocol="${format}"]`),
      ).toHaveTextContent(url);
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.getApiKey" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(keyPages[id]),
    );
  },
);

it("keeps vendor-only details out of the custom provider form", () => {
  mount();
  expect(
    screen.queryByRole("group", { name: "providerEditor.endpoints" }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "providerEditor.getApiKey" }),
  ).toBeNull();
  expect(screen.getByLabelText("modelProviders.baseUrl")).toBeInTheDocument();
});

it("uses the existing safe error handler when the key page cannot open", async () => {
  const error = new Error("synthetic-private-open-failure");
  vi.mocked(settingsApi.openExternal).mockRejectedValueOnce(error);
  mount(globalProviderPresets[0]);
  fireEvent.click(
    screen.getByRole("button", { name: "providerEditor.getApiKey" }),
  );
  await waitFor(() =>
    expect(onError).toHaveBeenCalledWith("openWebsite", error),
  );
  expect(screen.queryByText(error.message)).toBeNull();
});
