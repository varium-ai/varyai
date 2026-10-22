import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { providersApi } from "@/lib/api/providers";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value, onChange, ariaLabel }: any) => (
    <textarea
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    takeoverStatus: { workbuddy: true },
    setTakeoverForApp: vi.fn(),
  }),
}));

describe("WorkBuddyProviderForm", () => {
  it("saves multiple model IDs with shared credentials and rejects duplicates", async () => {
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save WorkBuddy"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Gateway",
          settingsConfig: {
            id: "model-a",
            name: "Gateway",
            url: "https://example.com/v1",
            apiKey: "synthetic-key",
          },
        }}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.form.addModel" }),
    );
    const inputs = screen
      .getAllByRole("textbox")
      .filter((input) => input.id.startsWith("workbuddy-model-id"));
    expect(inputs).toHaveLength(2);
    fireEvent.change(inputs[1], { target: { value: "model-a" } });
    fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "workbuddy.form.duplicateModels",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.change(inputs[1], { target: { value: "model-b" } });
    fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const config = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(config).toMatchObject({
      id: "model-a",
      apiKey: "synthetic-key",
      url: "https://example.com/v1/chat/completions",
    });
    expect(config.models.map((model: any) => model.id)).toEqual([
      "model-a",
      "model-b",
    ]);
  });

  it("keeps a saved model list editable and adds fetched models without duplicates", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "model-a", ownedBy: null },
      { id: "model-c", ownedBy: null },
    ]);
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save WorkBuddy"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Gateway",
          settingsConfig: {
            id: "model-a",
            url: "https://example.com/v1",
            apiKey: "synthetic-key",
            models: [
              { id: "model-a" },
              { id: "model-b", supportsReasoning: true },
            ],
          },
        }}
      />,
    );
    expect(screen.getByDisplayValue("model-b")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "workbuddy.form.addFetchedModels",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    const config = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    expect(config.models.map((model: any) => model.id)).toEqual([
      "model-a",
      "model-b",
      "model-c",
    ]);
    expect(config.models[1].supportsReasoning).toBe(true);
  });

  describe("model discovery", () => {
    function renderCustom(
      url = "https://api.example.com/v1",
      apiKey = "test-key",
    ) {
      const onSubmit = vi.fn();
      render(
        <ProviderForm
          appId="workbuddy"
          submitLabel="Save WorkBuddy"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "API model",
            settingsConfig: { id: "old-model", url, apiKey },
          }}
        />,
      );
      return onSubmit;
    }

    const fetchButton = () =>
      screen.getByRole("button", { name: "providerForm.fetchModels" });

    it.each([
      ["https://api.example.com/", "https://api.example.com"],
      ["https://api.example.com/v1/", "https://api.example.com/v1"],
      [
        "https://api.example.com/v1/chat/completions",
        "https://api.example.com/v1",
      ],
      ["https://api.example.com/chat/completions", "https://api.example.com"],
    ])("fetches from the API base for %s without saving", async (url, base) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "grok-4.5", ownedBy: "xai" },
      ]);
      const onSubmit = renderCustom(url);
      fireEvent.click(fetchButton());
      await screen.findByRole("button", { name: "Select model" });
      expect(fetchModelsForConfig).toHaveBeenLastCalledWith(base, "test-key");
      expect(screen.getByLabelText("workbuddy.form.url")).toHaveValue(url);
      expect(screen.getByLabelText("workbuddy.form.modelId")).toHaveValue(
        "old-model",
      );
      expect(onSubmit).not.toHaveBeenCalled();
    });

    it("fills the selected model ID and clears the list when credentials change", async () => {
      Element.prototype.scrollIntoView = vi.fn();
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "grok-4.5", ownedBy: "xai" },
      ]);
      const onSubmit = renderCustom();
      fireEvent.click(fetchButton());
      fireEvent.click(
        await screen.findByRole("button", { name: "Select model" }),
      );
      fireEvent.click(await screen.findByRole("option", { name: "grok-4.5" }));
      expect(screen.getByLabelText("workbuddy.form.modelId")).toHaveValue(
        "grok-4.5",
      );
      expect(onSubmit).not.toHaveBeenCalled();
      fireEvent.change(screen.getByLabelText("universalProvider.apiKey"), {
        target: { value: "another-key" },
      });
      expect(
        screen.queryByRole("button", { name: "Select model" }),
      ).not.toBeInTheDocument();
    });

    it("ignores an in-flight response after the endpoint changes", async () => {
      let resolve!: (models: FetchedModel[]) => void;
      vi.mocked(fetchModelsForConfig).mockReturnValue(
        new Promise((done) => {
          resolve = done;
        }),
      );
      renderCustom();
      fireEvent.click(fetchButton());
      expect(fetchButton()).toBeDisabled();
      fireEvent.change(screen.getByLabelText("workbuddy.form.url"), {
        target: { value: "https://other.example.com/v1" },
      });
      await act(async () => {
        resolve([{ id: "stale-model", ownedBy: null }]);
      });
      expect(
        screen.queryByRole("button", { name: "Select model" }),
      ).not.toBeInTheDocument();
      expect(fetchButton()).toBeEnabled();
    });

    it("requires a key before fetching", () => {
      vi.mocked(fetchModelsForConfig).mockClear();
      renderCustom("https://api.example.com/v1", "");
      fireEvent.click(fetchButton());
      expect(fetchModelsForConfig).not.toHaveBeenCalled();
      expect(toast.error).toHaveBeenCalledWith(
        "providerForm.fetchModelsNeedApiKey",
      );
    });

    it.each([
      ["", "providerForm.fetchModelsNeedEndpoint"],
      ["not-a-url", "workbuddy.form.invalidUrl"],
      ["file:///tmp/models", "workbuddy.form.invalidUrl"],
    ])("rejects invalid endpoint %s before fetching", (url, message) => {
      vi.mocked(fetchModelsForConfig).mockClear();
      renderCustom(url);
      fireEvent.click(fetchButton());
      expect(fetchModelsForConfig).not.toHaveBeenCalled();
      expect(toast.error).toHaveBeenCalledWith(message);
    });

    it("handles an empty list and reports auth failures without echoing errors", async () => {
      vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([]);
      renderCustom();
      fireEvent.click(fetchButton());
      await waitFor(() =>
        expect(toast.info).toHaveBeenCalledWith(
          "providerForm.fetchModelsEmpty",
        ),
      );
      expect(fetchButton()).toBeEnabled();
      vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
        new Error("HTTP 401: test-key"),
      );
      fireEvent.click(fetchButton());
      await waitFor(() =>
        expect(toast.error).toHaveBeenCalledWith(
          "providerForm.fetchModelsAuthFailed",
        ),
      );
      expect(fetchButton()).toBeEnabled();
    });
  });

  it("edits the official entry without requiring or writing API credentials", async () => {
    vi.spyOn(providersApi, "getWorkBuddySettings").mockResolvedValue({
      path: "/synthetic/workbuddy/settings.json",
      content: '{"theme":"light"}',
      revision: "v1",
    });
    const saveSettings = vi
      .spyOn(providersApi, "saveWorkBuddySettings")
      .mockResolvedValue(undefined);
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save official"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "WorkBuddy Official",
          category: "official",
          settingsConfig: {},
        }}
      />,
    );
    expect(
      screen.getByText("desktopModels.officialLogin"),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText("workbuddy.form.modelId"),
    ).not.toBeInTheDocument();
    const editor = await screen.findByRole("textbox", {
      name: "workbuddy.settings.label",
    });
    expect(editor).toHaveValue('{"theme":"light"}');
    expect(
      screen.queryByRole("switch", { name: "workbuddy.routing.title" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("WO")).not.toBeInTheDocument();
    fireEvent.change(editor, { target: { value: '{"theme":"dark"}' } });
    fireEvent.click(screen.getByRole("button", { name: "Save official" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(saveSettings).toHaveBeenCalledWith("v1", '{"theme":"dark"}');
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      presetCategory: "official",
      settingsConfig: "{}",
    });
  });

  it("saves a native Chat Completions model and preserves unexposed fields", async () => {
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save WorkBuddy"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "My model",
          settingsConfig: {
            id: "model-a",
            name: "My model",
            url: "http://localhost:8080/v1",
            apiKey: "test-key",
            relatedModels: { lite: "model-b" },
            supportsReasoning: true,
          },
        }}
      />,
    );
    fireEvent.change(screen.getByLabelText("workbuddy.form.modelId"), {
      target: { value: "model-new" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig)).toMatchObject({
      id: "model-new",
      url: "http://localhost:8080/v1/chat/completions",
      apiKey: "test-key",
      relatedModels: { lite: "model-b" },
      supportsReasoning: true,
    });
    expect(
      JSON.parse(onSubmit.mock.calls[0][0].settingsConfig),
    ).not.toHaveProperty("supportsImages");
  });

  it.each([
    "https://api.example.com",
    "https://api.example.com/",
    "https://api.example.com/v1/",
  ])(
    "appends chat/completions without a double slash for %s",
    async (baseUrl) => {
      const onSubmit = vi.fn();
      render(
        <ProviderForm
          appId="workbuddy"
          submitLabel="Save WorkBuddy"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "API model",
            settingsConfig: {
              id: "model-a",
              name: "API model",
              url: baseUrl,
              apiKey: "synthetic-key",
            },
          }}
        />,
      );
      fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
      const saved = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
      expect(saved.url).toBe(
        baseUrl.includes("/v1")
          ? "https://api.example.com/v1/chat/completions"
          : "https://api.example.com/chat/completions",
      );
    },
  );

  it("keeps the official editor open when saving the real file fails", async () => {
    vi.spyOn(providersApi, "getWorkBuddySettings").mockResolvedValue({
      path: "/synthetic/workbuddy/settings.json",
      content: "{}",
      revision: "v1",
    });
    const saveSettings = vi
      .spyOn(providersApi, "saveWorkBuddySettings")
      .mockRejectedValue(new Error("Configuration changed externally"));
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save official"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Official",
          category: "official",
          settingsConfig: {},
        }}
      />,
    );
    await screen.findByRole("textbox", { name: "workbuddy.settings.label" });
    fireEvent.click(screen.getByRole("button", { name: "Save official" }));
    await waitFor(() => expect(saveSettings).toHaveBeenCalled());
    expect(onSubmit).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalled();
    expect(
      screen.getByRole("textbox", { name: "workbuddy.settings.label" }),
    ).toBeInTheDocument();
  });

  it("offers a reload after a read failure and rejects invalid settings before writing", async () => {
    const readSettings = vi
      .spyOn(providersApi, "getWorkBuddySettings")
      .mockRejectedValueOnce(new Error("Unavailable"))
      .mockResolvedValue({
        path: "/synthetic/settings.json",
        content: "{}",
        revision: "v1",
      });
    const saveSettings = vi
      .spyOn(providersApi, "saveWorkBuddySettings")
      .mockResolvedValue(undefined);
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save official"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Official",
          category: "official",
          settingsConfig: {},
        }}
      />,
    );
    await screen.findByRole("alert");
    expect(
      screen.getByRole("button", { name: "Save official" }),
    ).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "workbuddy.settings.reload" }),
    );
    const editor = await screen.findByRole("textbox", {
      name: "workbuddy.settings.label",
    });
    expect(readSettings).toHaveBeenCalledTimes(2);
    fireEvent.change(editor, { target: { value: '{"env":null}' } });
    fireEvent.click(screen.getByRole("button", { name: "Save official" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("workbuddy.settings.invalid"),
    );
    expect(saveSettings).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("rejects invalid limits before submitting", async () => {
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId="workbuddy"
        submitLabel="Save WorkBuddy"
        onSubmit={onSubmit}
        onCancel={() => {}}
        initialData={{
          name: "Model",
          settingsConfig: {
            id: "model-a",
            url: "https://example.com/v1/chat/completions",
            apiKey: "test-key",
          },
        }}
      />,
    );
    fireEvent.change(screen.getByLabelText("workbuddy.form.maxOutputTokens"), {
      target: { value: "-5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save WorkBuddy" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("workbuddy.form.invalidTokens"),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
