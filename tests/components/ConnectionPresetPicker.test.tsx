import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConnectionPresetPicker } from "@/components/providers/forms/ConnectionPresetPicker";
import { connectionPresetsApi } from "@/lib/api/connection-presets";

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
  beforeEach(() => {
    vi.mocked(connectionPresetsApi.list).mockResolvedValue([fixture]);
    vi.mocked(connectionPresetsApi.save).mockResolvedValue(undefined);
    vi.mocked(connectionPresetsApi.remove).mockResolvedValue(undefined);
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
    await waitFor(() =>
      expect(connectionPresetsApi.remove).toHaveBeenCalledWith(fixture.id),
    );
    expect(apply).toHaveBeenCalledTimes(1);
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
    expect(
      screen.getByRole("option", { name: fixture.name, hidden: true }),
    ).toBeInTheDocument();
  });
});
