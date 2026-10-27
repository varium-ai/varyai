import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import i18n from "i18next";

import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConnectionPresetPicker } from "@/components/providers/forms/ConnectionPresetPicker";
import { connectionPresetsApi } from "@/lib/api/connection-presets";
import { detectProviderProtocols } from "@/lib/api/provider-protocol-detection";

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

vi.mock("@/lib/api/provider-protocol-detection", () => ({
  detectProviderProtocols: vi.fn(),
}));
vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/lib/api/connection-presets", () => ({
  connectionPresetsApi: { list: vi.fn(), save: vi.fn(), remove: vi.fn() },
}));

const fixture = {
  id: "fixture-id",
  name: "Fixture relay",
  baseUrl: "https://relay.example/custom/v1",
  apiKey: "synthetic-credential",
};

describe("reusable connection presets", () => {
  it("starts a visibly empty draft and focuses its name when adding a preset", async () => {
    render(<ConnectionPresetPicker onApply={vi.fn()} current={fixture} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    expect(screen.getByLabelText("connectionPresets.name")).toHaveValue(
      fixture.name,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.add" }),
    );
    expect(screen.getByLabelText("connectionPresets.name")).toHaveValue("");
    expect(screen.getByLabelText("connectionPresets.baseUrl")).toHaveValue("");
    expect(screen.getByLabelText("connectionPresets.apiKey")).toHaveValue("");
    expect(screen.getByLabelText("connectionPresets.name")).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent(
      "connectionPresets.newDraft",
    );
    expect(connectionPresetsApi.save).not.toHaveBeenCalled();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    i18n.addResource(
      "zh",
      "translation",
      "connectionPresets.deleteConfirmMessage",
      'Delete preset "{{name}}"?',
    );
    vi.mocked(connectionPresetsApi.list).mockResolvedValue([fixture]);
    vi.mocked(connectionPresetsApi.save).mockResolvedValue(undefined);
    vi.mocked(connectionPresetsApi.remove).mockResolvedValue(undefined);
  });

  it("saves a selected upstream protocol and restores it when editing", async () => {
    vi.mocked(connectionPresetsApi.list).mockResolvedValue([
      { ...fixture, apiFormat: "openai_responses" },
    ]);
    render(<ConnectionPresetPicker onApply={vi.fn()} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    const table = screen.getByRole("table");
    expect(
      within(table).getByRole("columnheader", {
        name: "providerForm.apiFormat",
      }),
    ).toBeVisible();
    expect(
      within(table).getByRole("cell", {
        name: "providerForm.apiFormatOpenAIResponses",
      }),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: `connectionPresets.edit ${fixture.name}`,
      }),
    );
    const protocol = screen.getByLabelText("providerForm.apiFormat");
    expect(protocol).toHaveValue("openai_responses");
    fireEvent.change(protocol, { target: { value: "openai_chat" } });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.save" }),
    );
    await waitFor(() =>
      expect(connectionPresetsApi.save).toHaveBeenCalledWith({
        ...fixture,
        apiFormat: "openai_chat",
      }),
    );
  });

  it("detects only on request and saves the chosen result without changing the agent model", async () => {
    vi.mocked(detectProviderProtocols).mockResolvedValue([
      {
        format: "openai_chat",
        text: { status: "verified" },
        stream: { status: "verified" },
      },
      {
        format: "gemini_native",
        text: { status: "unavailable", statusCode: 503 },
        stream: null,
      },
    ]);
    const apply = vi.fn();
    render(<ConnectionPresetPicker onApply={apply} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: `connectionPresets.edit ${fixture.name}`,
      }),
    );
    fireEvent.change(screen.getByLabelText("connectionPresets.testModel"), {
      target: { value: "fixture-model" },
    });
    expect(detectProviderProtocols).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.check" }),
    );
    await screen.findByText("providerDetection.verified");
    expect(detectProviderProtocols).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: fixture.baseUrl,
        apiKey: fixture.apiKey,
        model: "fixture-model",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "providerDetection.use" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.save" }),
    );
    await waitFor(() =>
      expect(connectionPresetsApi.save).toHaveBeenCalledWith({
        ...fixture,
        apiFormat: "openai_chat",
      }),
    );
    expect(apply).not.toHaveBeenCalled();
  });

  it("does not apply a protocol the target client cannot use", async () => {
    vi.mocked(connectionPresetsApi.list).mockResolvedValue([
      { ...fixture, apiFormat: "gemini_native" },
    ]);
    const apply = vi.fn();
    render(
      <ConnectionPresetPicker
        onApply={apply}
        supportedFormats={["openai_chat"]}
      />,
    );
    expect(
      await screen.findByRole("option", { name: new RegExp(fixture.name) }),
    ).toBeDisabled();
    fireEvent.change(
      screen.getByRole("combobox", { name: "connectionPresets.label" }),
      { target: { value: fixture.id } },
    );
    expect(apply).not.toHaveBeenCalled();
  });

  it("opens above the provider panel and closes without losing its draft", async () => {
    const user = userEvent.setup();
    const closeProvider = vi.fn();
    render(
      <FullScreenPanel isOpen title="Provider fixture" onClose={closeProvider}>
        <label>
          Provider draft
          <input defaultValue="Original name" />
        </label>
        <ConnectionPresetPicker onApply={vi.fn()} />
      </FullScreenPanel>,
    );
    const draft = screen.getByRole("textbox", { name: "Provider draft" });
    await user.clear(draft);
    await user.type(draft, "Unsaved provider name");
    await screen.findByRole("option", { name: fixture.name });
    const manage = screen.getByRole("button", {
      name: "connectionPresets.manage",
    });
    await user.click(manage);
    const dialog = screen.getByRole("dialog", {
      name: "connectionPresets.manage",
    });
    expect(dialog).toHaveClass("z-[110]");
    expect(dialog.parentElement).toBe(document.body);
    expect(dialog).not.toContainElement(draft);
    expect(draft).toHaveValue("Unsaved provider name");
    await user.click(
      within(dialog).getByRole("button", { name: "common.close" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(draft).toBeVisible();
    expect(draft).toHaveValue("Unsaved provider name");
    expect(manage).toHaveFocus();
    expect(closeProvider).not.toHaveBeenCalled();
    await user.click(manage);
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(closeProvider).not.toHaveBeenCalled();
    expect(draft).toHaveValue("Unsaved provider name");
  });

  it("only fills credentials when explicitly selected and does not expose keys in the menu", async () => {
    const apply = vi.fn();
    render(<ConnectionPresetPicker onApply={apply} />);
    await screen.findByRole("option", { name: fixture.name });
    expect(apply).not.toHaveBeenCalled();
    expect(screen.queryByText(fixture.apiKey)).not.toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("combobox", { name: "connectionPresets.label" }),
      { target: { value: fixture.id } },
    );
    expect(apply).toHaveBeenCalledWith(fixture);
  });

  it("saves a named preset without submitting or changing the agent form", async () => {
    const apply = vi.fn();
    const submit = vi.fn((event) => event.preventDefault());
    render(
      <form onSubmit={submit}>
        <ConnectionPresetPicker onApply={apply} />
      </form>,
    );
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("connectionPresets.name"), {
      target: { value: "New relay" },
    });
    fireEvent.change(
      within(dialog).getByLabelText("connectionPresets.baseUrl"),
      { target: { value: fixture.baseUrl } },
    );
    const key = within(dialog).getByLabelText("connectionPresets.apiKey");
    expect(key).toHaveAttribute("type", "password");
    fireEvent.change(key, { target: { value: fixture.apiKey } });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "connectionPresets.save" }),
    );
    await waitFor(() =>
      expect(connectionPresetsApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "New relay",
          baseUrl: fixture.baseUrl,
          apiKey: fixture.apiKey,
        }),
      ),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "connectionPresets.saveSuccess",
    );
    expect(toast.success).toHaveBeenCalledWith(
      "connectionPresets.saveSuccess",
      {
        closeButton: true,
      },
    );
    expect(submit).not.toHaveBeenCalled();
    expect(apply).not.toHaveBeenCalled();
  });

  it("edits and deletes a preset without updating previously applied fields", async () => {
    const apply = vi.fn();
    render(<ConnectionPresetPicker onApply={apply} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.change(
      screen.getByRole("combobox", { name: "connectionPresets.label" }),
      { target: { value: fixture.id } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: `connectionPresets.edit ${fixture.name}`,
      }),
    );
    fireEvent.change(screen.getByLabelText("connectionPresets.name"), {
      target: { value: "Renamed relay" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.save" }),
    );
    await waitFor(() =>
      expect(connectionPresetsApi.save).toHaveBeenCalledWith({
        ...fixture,
        name: "Renamed relay",
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: `connectionPresets.delete Renamed relay`,
      }),
    );
    expect(connectionPresetsApi.remove).not.toHaveBeenCalled();
    const confirmation = screen.getByRole("dialog", {
      name: "connectionPresets.deleteConfirmTitle",
    });
    expect(confirmation).toHaveTextContent('Delete preset "Renamed relay"?');
    fireEvent.click(
      within(confirmation).getByRole("button", {
        name: "connectionPresets.delete",
      }),
    );
    await waitFor(() =>
      expect(connectionPresetsApi.remove).toHaveBeenCalledWith(fixture.id),
    );
    expect(toast.success).toHaveBeenLastCalledWith(
      "connectionPresets.deleteSuccess",
      { closeButton: true },
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "connectionPresets.deleteSuccess",
    );
    expect(apply).toHaveBeenCalledTimes(1);
  });

  it("cancels deletion without changing the preset or the unsaved draft", async () => {
    const user = userEvent.setup();
    render(<ConnectionPresetPicker onApply={vi.fn()} />);
    await screen.findByRole("option", { name: fixture.name });
    await user.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: `connectionPresets.edit ${fixture.name}`,
      }),
    );
    fireEvent.change(screen.getByLabelText("connectionPresets.name"), {
      target: { value: "Unsaved draft" },
    });
    await user.click(
      screen.getByRole("button", {
        name: `connectionPresets.delete ${fixture.name}`,
      }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "connectionPresets.deleteConfirmTitle",
    });
    expect(confirmation).toHaveTextContent(`Delete preset "${fixture.name}"?`);
    expect(confirmation).not.toHaveTextContent(fixture.apiKey);
    expect(confirmation).not.toHaveTextContent(fixture.baseUrl);
    expect(connectionPresetsApi.remove).not.toHaveBeenCalled();
    await user.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    expect(
      screen.getByRole("dialog", { name: "connectionPresets.manage" }),
    ).toBeVisible();
    expect(screen.getByLabelText("connectionPresets.name")).toHaveValue(
      "Unsaved draft",
    );
    expect(connectionPresetsApi.remove).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("prevents repeated confirmation and dismissal while deletion is pending", async () => {
    let finish!: () => void;
    vi.mocked(connectionPresetsApi.remove).mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    const user = userEvent.setup();
    render(<ConnectionPresetPicker onApply={vi.fn()} />);
    await screen.findByRole("option", { name: fixture.name });
    await user.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: `connectionPresets.delete ${fixture.name}`,
      }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "connectionPresets.deleteConfirmTitle",
    });
    const confirm = within(confirmation).getByRole("button", {
      name: "connectionPresets.delete",
    });
    await user.click(confirm);
    expect(confirm).toBeDisabled();
    expect(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    await user.click(confirm);
    await user.keyboard("{Escape}");
    expect(confirmation).toBeVisible();
    expect(connectionPresetsApi.remove).toHaveBeenCalledOnce();
    expect(toast.success).not.toHaveBeenCalled();
    await act(async () => finish());
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", {
          name: "connectionPresets.deleteConfirmTitle",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("dialog", { name: "connectionPresets.manage" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", {
        name: `connectionPresets.delete ${fixture.name}`,
      }),
    ).not.toBeInTheDocument();
    expect(toast.success).toHaveBeenCalledWith(
      "connectionPresets.deleteSuccess",
      { closeButton: true },
    );
  });

  it("reports a safe deletion failure and retains the preset for retry", async () => {
    vi.mocked(connectionPresetsApi.remove).mockRejectedValue(
      new Error("private delete detail"),
    );
    render(<ConnectionPresetPicker onApply={vi.fn()} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: `connectionPresets.delete ${fixture.name}`,
      }),
    );
    const confirmation = screen.getByRole("dialog", {
      name: "connectionPresets.deleteConfirmTitle",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", {
        name: "connectionPresets.delete",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "connectionPresets.deleteFailed",
    );
    expect(toast.error).toHaveBeenCalledWith("connectionPresets.deleteFailed", {
      closeButton: true,
    });
    expect(toast.success).not.toHaveBeenCalled();
    expect(screen.queryByText("private delete detail")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `connectionPresets.delete ${fixture.name}`,
      }),
    ).toBeEnabled();
  });

  it("shows a safe failure and leaves existing choices intact when saving fails", async () => {
    vi.mocked(connectionPresetsApi.save).mockRejectedValue(
      new Error("private error detail"),
    );
    render(<ConnectionPresetPicker onApply={vi.fn()} />);
    await screen.findByRole("option", { name: fixture.name });
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.manage" }),
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: `connectionPresets.edit ${fixture.name}`,
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "connectionPresets.save" }),
    );
    expect(
      await screen.findByText("connectionPresets.saveFailed"),
    ).toBeVisible();
    expect(screen.queryByText("private error detail")).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(toast.success).not.toHaveBeenCalled();
    expect(
      screen.getByRole("option", { name: fixture.name, hidden: true }),
    ).toBeInTheDocument();
  });
});
