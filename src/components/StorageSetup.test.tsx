import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { StorageSetup } from "./StorageSetup";

const api = vi.hoisted(() => ({
  invoke: vi.fn(),
  restart: vi.fn(),
  pickDirectory: vi.fn(),
  setOverride: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    restart: api.restart,
    pickDirectory: api.pickDirectory,
    setAppConfigDirOverride: api.setOverride,
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

beforeEach(() => {
  vi.resetAllMocks();
  api.invoke.mockResolvedValue({
    target: "/synthetic/missing",
    needsSetup: true,
  });
  api.setOverride.mockResolvedValue(true);
});

it("offers native storage recovery without a legacy upgrade workflow", async () => {
  render(<StorageSetup />);
  await screen.findByText(/\/synthetic\/missing/);
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  expect(screen.queryByText("storage.migrate")).not.toBeInTheDocument();
  expect(screen.queryByText("storage.resume")).not.toBeInTheDocument();
});

it("creates empty native storage only after an explicit recovery action", async () => {
  render(<StorageSetup />);
  await screen.findByText(/\/synthetic\/missing/);
  await userEvent.click(
    screen.getByRole("button", { name: "storage.startEmpty" }),
  );
  expect(api.invoke).toHaveBeenCalledWith("complete_storage_setup");
  await waitFor(() => expect(api.restart).toHaveBeenCalledOnce());
});

it("can select another native data directory", async () => {
  api.pickDirectory.mockResolvedValue("/synthetic/existing");
  render(<StorageSetup />);
  await userEvent.click(
    screen.getByRole("button", { name: "storage.useExisting" }),
  );
  expect(api.setOverride).toHaveBeenCalledWith("/synthetic/existing");
  await waitFor(() => expect(api.restart).toHaveBeenCalledOnce());
});

it("preserves the specific validation error returned by storage setup", async () => {
  api.invoke.mockRejectedValue("storage.invalidTarget");
  render(<StorageSetup errorKey="storage.invalidTarget" />);
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent(
      "storage.invalidTarget",
    ),
  );
  await waitFor(() =>
    expect(api.invoke).toHaveBeenCalledWith("get_storage_setup"),
  );
  expect(screen.getByRole("alert")).not.toHaveTextContent("storage.readFailed");
});

it("does not show private failure details when storage lookup fails", async () => {
  api.invoke.mockRejectedValue(new Error("synthetic-private-setting"));
  render(<StorageSetup />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "storage.setupReadFailed",
  );
  expect(
    screen.queryByText(/synthetic-private-setting/),
  ).not.toBeInTheDocument();
});
