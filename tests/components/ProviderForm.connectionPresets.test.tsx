import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import type { AppId } from "@/lib/api";

const fixture = {
  id: "fixture",
  name: "Saved relay",
  baseUrl: "https://relay.example/custom/v1",
  apiKey: "synthetic-preset-key",
};
vi.mock("@/components/providers/forms/ConnectionPresetPicker", () => ({
  ConnectionPresetPicker: ({ onApply, disabled }: any) => (
    <button type="button" disabled={disabled} onClick={() => onApply(fixture)}>
      Apply fixture preset
    </button>
  ),
}));
vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: any) => <pre>{value}</pre>,
}));
vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div />,
}));
vi.mock("@/lib/query", async (original) => ({
  ...(await original<typeof import("@/lib/query")>()),
  useSettingsQuery: () => ({ data: { commonConfigConfirmed: true } }),
}));

const cases: [AppId, Record<string, unknown>][] = [
  [
    "claude",
    {
      env: {
        ANTHROPIC_BASE_URL: "https://old.example/v1",
        ANTHROPIC_API_KEY: "old-fixture",
        ANTHROPIC_MODEL: "keep-model",
      },
    },
  ],
  [
    "codex",
    {
      auth: { OPENAI_API_KEY: "old-fixture" },
      config:
        'model="keep-model"\nmodel_provider="custom"\n[model_providers.custom]\nname="Relay"\nwire_api="responses"\nbase_url="https://old.example/v1"',
    },
  ],
  [
    "gemini",
    {
      env: {
        GOOGLE_GEMINI_BASE_URL: "https://old.example/v1",
        GEMINI_API_KEY: "old-fixture",
        GEMINI_MODEL: "keep-model",
      },
      config: {},
    },
  ],
  [
    "opencode",
    {
      options: { baseURL: "https://old.example/v1", apiKey: "old-fixture" },
      models: {},
    },
  ],
  [
    "openclaw",
    { baseUrl: "https://old.example/v1", apiKey: "old-fixture", models: [] },
  ],
  ["hermes", { base_url: "https://old.example/v1", api_key: "old-fixture" }],
  [
    "pi",
    {
      baseUrl: "https://old.example/v1",
      apiKey: "old-fixture",
      models: [{ id: "keep-model", name: "Keep" }],
    },
  ],
  [
    "claude-desktop",
    {
      env: {
        ANTHROPIC_BASE_URL: "https://old.example/v1",
        ANTHROPIC_API_KEY: "old-fixture",
      },
    },
  ],
  [
    "grokbuild",
    {
      config:
        'model="keep-model"\napi_key="old-fixture"\nbase_url="https://old.example/v1"',
    },
  ],
  [
    "workbuddy",
    { id: "keep-model", url: "https://old.example/v1", apiKey: "old-fixture" },
  ],
  [
    "qoder",
    {
      model: "keep-model",
      baseUrl: "https://old.example/v1",
      apiKey: "old-fixture",
      models: [{ model: "keep-model" }],
    },
  ],
  ...(["kimi", "qwen", "zcode", "trae"] as const).map(
    (appId): [AppId, Record<string, unknown>] => [
      appId,
      {
        baseUrl: "https://old.example/v1",
        apiKey: "old-fixture",
        models: [{ model: "keep-model" }],
      },
    ],
  ),
];

describe("connection presets in every agent form", () => {
  it.each(cases.slice(0, 3))(
    "persists %s credentials independently and preserves its model",
    async (appId, settingsConfig) => {
      const onSubmit = vi.fn();
      const view = render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            providerId="fixture-provider"
            initialData={{
              name: "Existing relay",
              category: "custom",
              settingsConfig,
            }}
            submitLabel="Save fixture"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "Apply fixture preset" }),
      );
      await screen.findByDisplayValue(fixture.apiKey);
      fireEvent.change(screen.getByDisplayValue(fixture.apiKey), {
        target: { value: "independent-fixture-key" },
      });
      fireEvent.click(screen.getByRole("button", { name: "Save fixture" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
      const serialized = JSON.stringify(saved);
      expect(serialized).toContain(fixture.baseUrl);
      expect(serialized).toContain("independent-fixture-key");
      expect(serialized).toContain("keep-model");
      expect(serialized).not.toContain("old-fixture");
      view.unmount();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            providerId="fixture-provider"
            initialData={{
              name: fixture.name,
              category: "custom",
              settingsConfig: saved,
            }}
            submitLabel="Save fixture"
            onSubmit={vi.fn()}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      expect(screen.getByDisplayValue(fixture.baseUrl)).toBeInTheDocument();
      expect(
        screen.getByDisplayValue("independent-fixture-key"),
      ).toBeInTheDocument();
    },
  );

  it.each(
    cases.flatMap(([appId, settingsConfig]) => [
      { appId, mode: "edit", settingsConfig },
      { appId, mode: "add", settingsConfig: undefined },
    ]),
  )(
    "fills $appId $mode fields and keeps them editable",
    async ({ appId, settingsConfig }) => {
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId={appId}
            providerId="fixture-provider"
            initialData={
              settingsConfig
                ? {
                    name: "Existing relay",
                    category: "custom",
                    settingsConfig,
                  }
                : undefined
            }
            submitLabel="Save fixture"
            onSubmit={vi.fn()}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "Apply fixture preset" }),
      );
      await waitFor(() =>
        expect(screen.getByDisplayValue(fixture.baseUrl)).toBeInTheDocument(),
      );
      expect(screen.getByDisplayValue(fixture.apiKey)).toBeInTheDocument();
      expect(screen.getByDisplayValue(fixture.name)).toBeInTheDocument();
      fireEvent.change(screen.getByDisplayValue(fixture.apiKey), {
        target: { value: "independent-fixture-key" },
      });
      expect(
        screen.getByDisplayValue("independent-fixture-key"),
      ).toBeInTheDocument();
    },
  );
});
