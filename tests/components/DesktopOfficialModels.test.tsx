import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { providersApi } from "@/lib/api/providers";
import { toast } from "sonner";

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
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
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ takeoverStatus: {}, setTakeoverForApp: vi.fn() }),
}));

vi.mock("@/components/providers/AuthSettingsPanel", () => ({
  AuthSettingsPanel: () => null,
}));
vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ isOpen, children, footer }: any) =>
    isOpen ? (
      <div>
        {children}
        {footer}
      </div>
    ) : null,
}));

it.each(["workbuddy", "qoder"] as const)(
  "%s saves third-party keys and multiple models in the official editor without replacing official login",
  async (appId) => {
    vi.spyOn(
      providersApi,
      appId === "qoder" ? "getQoderSettings" : "getWorkBuddySettings",
    ).mockResolvedValue({
      path: "/synthetic/settings.json",
      content: "{}",
      revision: "v1",
    });
    const saveSettings = vi
      .spyOn(
        providersApi,
        appId === "qoder" ? "saveQoderSettings" : "saveWorkBuddySettings",
      )
      .mockResolvedValue(undefined);
    const onSubmit = vi.fn();
    const onAddThirdParty = vi.fn().mockResolvedValue(undefined);
    render(
      <EditProviderDialog
        open
        appId={appId}
        onSubmit={onSubmit}
        onAddThirdParty={onAddThirdParty}
        onOpenChange={vi.fn()}
        provider={{
          id: `${appId}-official`,
          name: "Official",
          category: "official",
          settingsConfig: {},
        }}
      />,
    );
    await screen.findByRole("textbox", { name: `${appId}.settings.label` });
    fireEvent.change(screen.getByLabelText(`${appId}.form.modelId`), {
      target: { value: "model-a" },
    });
    fireEvent.change(screen.getByLabelText(`${appId}.form.url`), {
      target: { value: "https://example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("universalProvider.apiKey"), {
      target: { value: "synthetic-test-key" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: `${appId}.form.addModel` }),
    );
    const modelInputs = screen
      .getAllByRole("textbox")
      .filter((input) => input.id.startsWith("workbuddy-model-id"));
    fireEvent.change(modelInputs[1], { target: { value: "model-b" } });
    onAddThirdParty.mockRejectedValueOnce(new Error("Synthetic failure"));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("desktopModels.saveFailed"),
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByLabelText("universalProvider.apiKey")).toHaveValue(
      "synthetic-test-key",
    );
    onAddThirdParty.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onAddThirdParty).toHaveBeenCalledOnce();
    const added = onAddThirdParty.mock.calls[0][0];
    expect(added.category).toBe("custom");
    const config = added.settingsConfig;
    expect(config.apiKey).toBe("synthetic-test-key");
    if (appId === "qoder") {
      expect(config).toMatchObject({
        baseUrl: "https://example.com/v1",
        model: "model-a",
      });
      expect(config.models.map((model: any) => model.model)).toEqual([
        "model-a",
        "model-b",
      ]);
      expect(config).not.toHaveProperty("url");
    } else {
      expect(config).toMatchObject({
        url: "https://example.com/v1/chat/completions",
        id: "model-a",
        supportsToolCall: false,
        supportsImages: false,
      });
      expect(config.models.map((model: any) => model.id)).toEqual([
        "model-a",
        "model-b",
      ]);
    }
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      originalId: `${appId}-official`,
      provider: { category: "official", settingsConfig: {} },
    });
    expect(saveSettings).not.toHaveBeenCalled();
  },
);

it.each(["workbuddy", "qoder"] as const)(
  "%s allows official settings to be saved with empty optional key fields",
  async (appId) => {
    vi.spyOn(
      providersApi,
      appId === "qoder" ? "getQoderSettings" : "getWorkBuddySettings",
    ).mockResolvedValue({
      path: "/synthetic/settings.json",
      content: "{}",
      revision: "v1",
    });
    vi.spyOn(
      providersApi,
      appId === "qoder" ? "saveQoderSettings" : "saveWorkBuddySettings",
    ).mockResolvedValue(undefined);
    const onAddThirdParty = vi.fn();
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        appId={appId}
        submitLabel="Save"
        onSubmit={onSubmit}
        onAddThirdParty={onAddThirdParty}
        onCancel={vi.fn()}
        initialData={{
          name: "Official",
          category: "official",
          settingsConfig: {},
        }}
      />,
    );
    await screen.findByRole("textbox", { name: `${appId}.settings.label` });
    expect(screen.getByLabelText("universalProvider.apiKey")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledOnce());
    expect(onAddThirdParty).not.toHaveBeenCalled();
  },
);
