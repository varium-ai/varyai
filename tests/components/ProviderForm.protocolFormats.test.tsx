import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div />,
}));

vi.mock("@/lib/query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/query")>()),
  useSettingsQuery: () => ({ data: { commonConfigConfirmed: true } }),
}));

describe("provider upstream protocol selection", () => {
  beforeAll(() => {
    Element.prototype.scrollIntoView = vi.fn();
    i18n.addResourceBundle("zh", "translation", zh, true, true);
  });

  it("preserves a Gemini provider's Chat upstream while editing", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="gemini"
          providerId="gemini-chat"
          submitLabel="Save protocol"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Gemini via Chat",
            category: "third_party",
            settingsConfig: {
              env: {
                GEMINI_API_KEY: "fixture-key",
                GOOGLE_GEMINI_BASE_URL: "https://relay.example/v1",
                GEMINI_MODEL: "fixture-model",
              },
              config: {},
            },
            meta: { apiFormat: "openai_chat" },
          }}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByLabelText("上游格式")).toHaveTextContent(
      "OpenAI Chat Completions",
    );
    await user.click(screen.getByRole("button", { name: "Save protocol" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onSubmit.mock.calls[0][0].meta.apiFormat).toBe("openai_chat");
  });

  it.each(["openai_responses", "gemini_native"] as const)(
    "supports a Gemini upstream for a Codex provider initially using %s",
    async (initialFormat) => {
      const user = userEvent.setup();
      const onSubmit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="codex"
            providerId="codex-gemini"
            submitLabel="Save protocol"
            onSubmit={onSubmit}
            onCancel={vi.fn()}
            initialData={{
              name: "Codex via Gemini",
              category: "third_party",
              settingsConfig: {
                auth: { OPENAI_API_KEY: "fixture-key" },
                config:
                  'model = "fixture-model"\nmodel_provider = "custom"\n[model_providers.custom]\nname = "Relay"\nbase_url = "https://relay.example/v1beta"\nwire_api = "responses"\n',
              },
              meta: { apiFormat: initialFormat },
            }}
          />
        </QueryClientProvider>,
      );

      await user.click(screen.getByLabelText("上游格式"));
      await user.click(screen.getByRole("option", { name: /Gemini Native/ }));
      await user.click(screen.getByRole("button", { name: "Save protocol" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const submitted = onSubmit.mock.calls[0][0];
      expect(submitted.meta.apiFormat).toBe("gemini_native");
      expect(JSON.parse(submitted.settingsConfig).config).toContain(
        'wire_api = "responses"',
      );
    },
  );
});
