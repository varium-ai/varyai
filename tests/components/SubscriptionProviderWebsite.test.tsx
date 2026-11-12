import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ProviderEditorDialog } from "@/components/providers/ProviderEditorDialog";
import { settingsApi } from "@/lib/api/settings";
import type { RoutingUpstream } from "@/lib/api/routingUpstreams";

vi.mock("@/components/providers/SubscriptionProviderFields", () => ({
  SubscriptionProviderFields: () => null,
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn().mockResolvedValue(undefined) },
}));
beforeEach(() => vi.clearAllMocks());

it.each([
  ["claude", "https://claude.ai"],
  ["chatgpt_api", "https://chatgpt.com"],
  ["github_copilot", "https://github.com"],
  ["gemini_cli", "https://geminicli.com"],
  ["antigravity", "https://antigravity.google"],
  ["xai_oauth", "https://grok.com"],
] as const)(
  "opens the correct website for the %s subscription",
  async (provider, website) => {
    const draft: RoutingUpstream = {
      schemaVersion: 1,
      id: "fixture",
      revision: 1,
      name: "Subscription fixture",
      hasApiKey: false,
      source: {
        kind: "subscription",
        provider,
        accountId: "fixture-account",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    };
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ProviderEditorDialog
          draft={draft}
          models="[]"
          discovered={[]}
          apiKey=""
          busy={false}
          region="global"
          regions={[]}
          onDraftChange={vi.fn()}
          onEnabledChange={vi.fn()}
          onModelsChange={vi.fn()}
          onKeyChange={vi.fn()}
          onProtocolChange={vi.fn()}
          onRegionChange={vi.fn()}
          onDiscover={vi.fn()}
          onFetchProbeModels={vi.fn()}
          onProbe={vi.fn()}
          onSave={vi.fn()}
          onClose={vi.fn()}
          onReturnFocus={vi.fn()}
          onError={vi.fn()}
          nestedOpen={false}
          onProbingChange={vi.fn()}
        />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: new URL(website).host }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(website),
    );
  },
);
